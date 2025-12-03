/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: December 3, 2025
 * License: MIT
 */

//! `vsv` control commands (start, stop, etc.).

use anyhow::{ensure, Result};
use yansi::Paint;

use crate::arguments::Commands;
use crate::config::Config;
use crate::runit::{RunitCommand, RunitService};

/// Handle all control subcommands.
pub fn run(cfg: &Config, cmd: &Commands) -> Result<()> {
    // Determine the action and the service list
    let (services, command, verb) = match cmd {
        Commands::Start { services } => {
            (services, Some(RunitCommand::Up), "starting")
        }
        Commands::Stop { services } => {
            (services, Some(RunitCommand::Down), "stopping")
        }
        // Restart sends Term, Continue, Up (handled below)
        Commands::Restart { services } => (services, None, "restarting"),
        Commands::Reload { services } => {
            (services, Some(RunitCommand::Hup), "reloading")
        }
        Commands::Once { services } => {
            (services, Some(RunitCommand::Once), "running once")
        }
        Commands::Pause { services } => {
            (services, Some(RunitCommand::Pause), "pausing")
        }
        Commands::Cont { services } => {
            (services, Some(RunitCommand::Cont), "resuming")
        }
        Commands::Hup { services } => {
            (services, Some(RunitCommand::Hup), "sending HUP")
        }
        Commands::Alarm { services } => {
            (services, Some(RunitCommand::Alarm), "sending ALARM")
        }
        Commands::Interrupt { services } => {
            (services, Some(RunitCommand::Interrupt), "sending INT")
        }
        Commands::Quit { services } => {
            (services, Some(RunitCommand::Quit), "sending QUIT")
        }
        Commands::Term { services } => {
            (services, Some(RunitCommand::Term), "sending TERM")
        }
        Commands::Kill { services } => {
            (services, Some(RunitCommand::Kill), "sending KILL")
        }
        Commands::Exit { services } => {
            (services, Some(RunitCommand::Exit), "exiting")
        }
        _ => return Ok(()), // Should not happen given the dispatch in main
    };

    ensure!(!services.is_empty(), "at least one (1) service required");

    for name in services {
        let p = cfg.svdir.join(name);
        let svc = RunitService::new(name, &p);

        print!("{} service {}... ", verb, name.bold());

        if !svc.valid() {
            println!("{}", "failed! service not valid".red());
            continue;
        }

        let result = if let Some(c) = command {
            // Standard single command
            svc.control(c)
        } else {
            // Restart sequence: Terminate -> Continue -> Up
            svc.control(RunitCommand::Term)
                .and_then(|_| svc.control(RunitCommand::Cont))
                .and_then(|_| svc.control(RunitCommand::Up))
        };

        match result {
            Ok(_) => println!("{}", "ok".green()),
            Err(e) => println!("{}: {}", "failed".red(), e),
        }
    }

    Ok(())
}
