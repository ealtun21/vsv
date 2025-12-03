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
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Command, CommandFactory};
use clap_complete::env::CompleteEnv;
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

/// Construct the Clap Command with dynamic completions attached.
/// This runs every time the shell requests completions.
fn build_cli_with_completions() -> Command {
    let mut cmd = arguments::Args::command();

    // 1. Fetch dynamic values
    let running_services = utils::get_running_services();
    let avail_services = utils::get_avail_services();

    // 2. Attach running services to standard commands (using "services" argument)
    let running_svc_cmds = [
        "start",
        "stop",
        "restart",
        "reload",
        "once",
        "pause",
        "cont",
        "hup",
        "alarm",
        "interrupt",
        "quit",
        "term",
        "kill",
        "exit",
        "remove",
        "enable",
        "disable",
    ];

    for sub_name in running_svc_cmds {
        // Clone the services list for this subcommand closure
        let values = running_services.clone();

        // We use `move` to transfer ownership of `values` into the closure
        cmd = cmd.mut_subcommand(sub_name, move |sub| {
            sub.mut_arg("services", move |arg| {
                // Fix: explicit map to PossibleValue with leaked static lifetime
                let vals = values.iter().map(|s| {
                    let static_str: &'static str =
                        Box::leak(s.clone().into_boxed_str());
                    PossibleValue::new(static_str)
                });
                arg.value_parser(PossibleValuesParser::new(vals))
            })
        });
    }

    // 3. Attach running services to Log command (uses "service" singular argument)
    {
        let values = running_services.clone();
        cmd = cmd.mut_subcommand("log", move |sub| {
            sub.mut_arg("service", move |arg| {
                let vals = values.iter().map(|s| {
                    let static_str: &'static str =
                        Box::leak(s.clone().into_boxed_str());
                    PossibleValue::new(static_str)
                });
                arg.value_parser(PossibleValuesParser::new(vals))
            })
        });
    }

    // 4. Attach available services to Add command
    {
        let values = avail_services.clone();
        cmd = cmd.mut_subcommand("add", move |sub| {
            sub.mut_arg("services", move |arg| {
                let vals = values.iter().map(|s| {
                    let static_str: &'static str =
                        Box::leak(s.clone().into_boxed_str());
                    PossibleValue::new(static_str)
                });
                arg.value_parser(PossibleValuesParser::new(vals))
            })
        });
    }

    cmd
}

fn do_main() -> Result<()> {
    // disable color until we absolutely know we want it
    yansi::disable();

    // 1. Handle Dynamic Completion
    // If the shell is asking for completions (e.g. COMPLETE=bash is set),
    // this will run the completion logic, print results, and EXIT the process.
    CompleteEnv::with_factory(build_cli_with_completions).complete();

    // 2. Normal Execution
    let args = arguments::parse();

    // Handle the "Completions" command specifically to print the setup instructions
    if let Some(Commands::Completions { shell }) = &args.command {
        let bin_name = env!("CARGO_PKG_NAME");
        let shell_name = shell.to_string();

        match shell {
            clap_complete::Shell::Bash => {
                println!("source <(COMPLETE=bash {})", bin_name);
            }
            clap_complete::Shell::Zsh => {
                println!("source <(COMPLETE=zsh {})", bin_name);
            }
            clap_complete::Shell::Fish => {
                println!("COMPLETE=fish {} | source", bin_name);
            }
            _ => {
                println!(
                    "# Dynamic completion not fully tested for {}. Try:",
                    shell_name
                );
                println!("source <(COMPLETE={} {})", shell_name, bin_name);
            }
        }
        return Ok(());
    }

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
            Commands::Add { .. } => commands::add_remove::do_add(&cfg),
            Commands::Remove { .. } => commands::add_remove::do_remove(&cfg),
            Commands::Avail => commands::add_remove::do_avail(&cfg),
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
                let log_run = svdir_log.join("run");
                if log_run.exists() {
                    if let Ok(content) = fs::read_to_string(&log_run) {
                        let mut tag = String::new();

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
                                if tag.is_empty() && line.contains("vlogger") {
                                    tag = service.to_string();
                                }
                            }
                        }

                        if !tag.is_empty() {
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
            Commands::Completions { .. } => Ok(()), // Handled above
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
