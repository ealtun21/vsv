/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 25, 2022
 * License: MIT
 */

/*!
 * Config context variable and various constants for vsv.
 */

use std::env;
use std::fmt;
use std::io;
use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::arguments::{Args, Commands};

// default values
pub const DEFAULT_SVDIR: &str = "/var/service";
pub const DEFAULT_PROC_DIR: &str = "/proc";
pub const DEFAULT_USER_DIR: &str = "runit/service";

// env var name
pub const ENV_NO_COLOR: &str = "NO_COLOR";
pub const ENV_SVDIR: &str = "SVDIR";
pub const ENV_PROC_DIR: &str = "PROC_DIR";

/// vsv execution modes (subcommands).
#[derive(Debug)]
pub enum ProgramMode {
    Status,
    Enable,
    Disable,
    Control,
    Log,
}

impl fmt::Display for ProgramMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            ProgramMode::Status => "status",
            ProgramMode::Enable => "enable",
            ProgramMode::Disable => "disable",
            ProgramMode::Control => "control",
            ProgramMode::Log => "log",
        };

        s.fmt(f)
    }
}

#[derive(Debug)]
pub struct Config {
    pub mode: ProgramMode,
    pub colorize: bool,
    pub svdir: PathBuf,
    pub tree: bool,
    pub log: bool,
    pub verbose: usize,
    pub operands: Vec<String>,
    pub proc_path: PathBuf,
}

impl Config {
    pub fn from_args(args: &Args) -> Result<Self> {
        let mut tree = args.tree;
        let mut log = args.log;
        let mut operands = vec![];

        let svdir = get_svdir(&args.dir, args.user)
            .context("failed to determine SVDIR")?;

        // check mode
        let mode = if let Some(cmd) = &args.command {
            match cmd {
                Commands::Status { tree: t, filter, log: l } => {
                    if *t {
                        tree = true;
                    }
                    if *l {
                        log = true;
                    }
                    operands = filter.to_vec();
                    ProgramMode::Status
                }
                Commands::Enable { services } => {
                    operands = services.to_vec();
                    ProgramMode::Enable
                }
                Commands::Disable { services } => {
                    operands = services.to_vec();
                    ProgramMode::Disable
                }
                Commands::Log { service, args: _ } => {
                    operands = vec![service.to_string()];
                    ProgramMode::Log
                }
                _ => {
                    // Control commands
                    match cmd {
                        Commands::Start { services }
                        | Commands::Stop { services }
                        | Commands::Restart { services }
                        | Commands::Reload { services }
                        | Commands::Once { services }
                        | Commands::Pause { services }
                        | Commands::Cont { services }
                        | Commands::Hup { services }
                        | Commands::Alarm { services }
                        | Commands::Interrupt { services }
                        | Commands::Quit { services }
                        | Commands::Term { services }
                        | Commands::Kill { services }
                        | Commands::Exit { services } => {
                            operands = services.to_vec();
                        }
                        _ => {}
                    }
                    ProgramMode::Control
                }
            }
        } else {
            // Default to Status if no subcommand is given
            ProgramMode::Status
        };

        let colorize = should_colorize_output(&args.color)?;
        let verbose = args.verbose as usize;
        let proc_path = env::var_os(ENV_PROC_DIR)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PROC_DIR));

        let o = Self {
            mode,
            colorize,
            svdir,
            tree,
            log,
            verbose,
            operands,
            proc_path,
        };

        Ok(o)
    }
}

/**
 * Check if the output should be colorized.
 */
fn should_colorize_output(color_arg: &Option<String>) -> Result<bool> {
    // check CLI option first
    if let Some(s) = color_arg {
        match s.as_str() {
            "yes" | "on" | "always" => return Ok(true),
            "no" | "off" | "never" => return Ok(false),
            "auto" => (), // fall through
            _ => bail!("unknown color option: '{}'", s),
        }
    }

    // check env var next
    if env::var_os(ENV_NO_COLOR).is_some() {
        return Ok(false);
    }

    // lastly check if stdout is a tty
    let isatty = io::stdout().is_terminal();

    Ok(isatty)
}

/**
 * Determine the `SVDIR` the user wants.
 */
fn get_svdir(dir_arg: &Option<PathBuf>, user_arg: bool) -> Result<PathBuf> {
    // `-d <dir>`
    if let Some(dir) = dir_arg {
        return Ok(dir.to_path_buf());
    }

    // `-u`
    if user_arg {
        let home = env::var_os("HOME")
            .context("failed to determine home directory (no HOME env var)")?;
        let path = PathBuf::from(home).join(DEFAULT_USER_DIR);
        return Ok(path);
    }

    // `SVDIR` env
    if let Some(dir) = env::var_os(ENV_SVDIR) {
        return Ok(PathBuf::from(dir));
    }

    // default
    Ok(PathBuf::from(DEFAULT_SVDIR))
}
