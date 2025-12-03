/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 25, 2022
 * License: MIT
 */

/*!
 * A rust port of `vsv`
 *
 * Original: <https://github.com/bahamas10/vsv>
 */

#![allow(clippy::uninlined_format_args)]

use anyhow::{bail, Context, Result};
use yansi::Paint;

mod arguments;
mod commands;
mod config;
mod die;
mod runit;
mod service;
mod utils;

use arguments::Commands;
use config::Config;
use die::die;
use utils::verbose;

fn do_main() -> Result<()> {
    // disable color until we absolutely know we want it
    yansi::disable();

    // parse CLI options + env vars
    let args = arguments::parse();
    let cfg =
        Config::from_args(&args).context("failed to parse args into config")?;

    // toggle color if the user wants it or the env dictates
    if cfg.colorize {
        yansi::enable();
    }

    verbose!(
        cfg,
        "program_mode={} num_threads={} color_output={}",
        cfg.mode,
        rayon::current_num_threads(),
        cfg.colorize
    );

    // check for root permissions
    check_root_permissions(&cfg);

    // figure out subcommand to run
    if let Some(ref cmd) = args.command {
        match cmd {
            Commands::Status { .. } => commands::status::do_status(&cfg),
            Commands::Enable { .. } => {
                commands::enable_disable::do_enable(&cfg)
            }
            Commands::Disable { .. } => {
                commands::enable_disable::do_disable(&cfg)
            }
            Commands::Log { service, args: _ } => {
                // Log command logic directly here using utils
                let log_path = cfg.svdir.join(service).join("log/current");
                if !log_path.exists() {
                    bail!("log file does not exist: {:?}", log_path);
                }
                println!("{} {}...", "viewing log for".green(), service.bold());
                utils::follow_file(&log_path)
            }
            // Pass all other commands to the control handler
            _ => commands::control::run(&cfg, cmd),
        }
    } else {
        // Default to status if no command provided
        commands::status::do_status(&cfg)
    }
}

fn check_root_permissions(cfg: &Config) {
    if cfg.svdir.to_str() == Some(config::DEFAULT_SVDIR) {
        let is_root = unsafe { libc::geteuid() } == 0;
        if !is_root {
            die!(
                1,
                "Root permissions required to manage services in {}. Please run with sudo.",
                config::DEFAULT_SVDIR
            );
        }
    }
}

fn main() {
    let ret = do_main();

    if let Err(err) = ret {
        die!(1, "{}: {:?}", "error".red().bold(), err);
    }
}
