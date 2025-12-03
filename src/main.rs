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

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
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
            // --- NEW COMMANDS START ---
            Commands::Add { .. } => commands::add_remove::do_add(&cfg),
            Commands::Remove { .. } => commands::add_remove::do_remove(&cfg),
            Commands::Avail => commands::add_remove::do_avail(&cfg),
            // --- NEW COMMANDS END ---
            Commands::Log { service, lines, all } => {
                // Log command logic
                let svdir_log = cfg.svdir.join(service).join("log");
                let log_current = svdir_log.join("current");

                let num_lines = lines.unwrap_or(10);
                // "all" overrides lines
                let (lines_to_show, read_all) = if *all {
                    (0, true) // lines ignored if read_all
                } else {
                    (num_lines, false)
                };

                let desc = if read_all {
                    "all".to_string()
                } else {
                    num_lines.to_string()
                };

                // 1. Try standard runit log/current
                if log_current.exists() {
                    println!(
                        "{} {} ({} lines)...",
                        "viewing log for".green(),
                        service.bold(),
                        desc
                    );
                    return utils::follow_file(
                        &log_current,
                        lines_to_show,
                        read_all,
                    );
                }

                // 2. Try to deduce if it uses syslog/vlogger
                // We check the 'run' script in the log directory
                let log_run = svdir_log.join("run");
                if log_run.exists() {
                    if let Ok(content) = fs::read_to_string(&log_run) {
                        // Look for "-t TAG" or just assume service name if vlogger is used
                        let mut tag = String::new();

                        // Simple parser for "vlogger -t tag" or "logger -t tag"
                        for line in content.lines() {
                            if line.contains("vlogger")
                                || line.contains("logger")
                            {
                                let parts: Vec<&str> =
                                    line.split_whitespace().collect();
                                for (i, part) in parts.iter().enumerate() {
                                    if *part == "-t" && i + 1 < parts.len() {
                                        tag = parts[i + 1].to_string();
                                        break;
                                    }
                                }
                                // If found vlogger but no tag, default to service name
                                if tag.is_empty() && line.contains("vlogger") {
                                    tag = service.to_string();
                                }
                            }
                        }

                        if !tag.is_empty() {
                            // We found a tag, now look for a system log file
                            // Priority: Void socklog -> Standard syslog -> Messages
                            let syslogs = [
                                "/var/log/socklog/everything/current",
                                "/var/log/syslog",
                                "/var/log/messages",
                            ];

                            for sys_log_path_str in syslogs {
                                let p = PathBuf::from(sys_log_path_str);
                                if p.exists() {
                                    println!(
                                        "{} {} in {} ({} lines)...",
                                        "viewing syslog for tag".green(),
                                        tag.bold(),
                                        sys_log_path_str.dim(),
                                        desc
                                    );

                                    return utils::follow_file_filtered(
                                        &p,
                                        &tag,
                                        lines_to_show,
                                        read_all,
                                    );
                                }
                            }
                        }
                    }
                }

                // 3. Give up
                println!(
                    "{} {}",
                    "Log file not found at:".red(),
                    log_current.display()
                );
                println!("This service likely uses a logger (like vlogger/logger) that writes to syslog.");
                println!("Check /var/log/socklog/, /var/log/syslog, or use 'logread'.");
                Ok(())
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
