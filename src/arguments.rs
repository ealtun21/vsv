/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 25, 2022
 * License: MIT
 */

//! Argument parsing logic (via `clap`) for vsv.

use std::path;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[clap(author, version, about, verbatim_doc_comment, long_about = None)]
#[clap(before_help = r" __   _______   __
 \ \ / / __\ \ / /   Void Service Manager
  \ V /\__ \\ V /    Source: https://github.com/bahamas10/vsv
   \_/ |___/ \_/     MIT License
   -------------
    Manage and view runit services
    Made specifically for Void Linux but should work anywhere
    Author: Dave Eddy <dave@daveeddy.com> (bahamas10)")]
#[clap(after_help = "All commands are implemented natively in Rust.
Common subcommands:

    start <service>           Start the service
    stop <service>            Stop the service
    restart <service>         Restart the service
    reload <service>          Reload the service (send SIGHUP)
    log <service>             View service logs (tail -f)
")]
pub struct Args {
    /// Enable or disable color output.
    #[clap(short, long, value_name = "yes|no|auto")]
    pub color: Option<String>,

    /// Directory to look into, defaults to env SVDIR or /var/service if unset.
    #[clap(short, long, value_parser, value_name = "dir")]
    pub dir: Option<path::PathBuf>,

    /// Show log processes, this is a shortcut for `status -l`.
    #[clap(short, long)]
    pub log: bool,

    /// Tree view, this is a shortcut for `status -t`.
    #[clap(short, long)]
    pub tree: bool,

    /// User mode, this is a shortcut for `-d ~/runit/service`.
    #[clap(short, long)]
    pub user: bool,

    /// Increase Verbosity.
    #[clap(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Subcommand.
    #[clap(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show process status.
    Status {
        /// Show associated log processes.
        #[clap(short, long)]
        log: bool,

        /// Tree view (calls pstree(1) on PIDs found).
        #[clap(short, long)]
        tree: bool,

        filter: Vec<String>,
    },

    /// Enable service(s).
    Enable { services: Vec<String> },

    /// Disable service(s).
    Disable { services: Vec<String> },

    /// Start service(s) (up).
    Start { services: Vec<String> },

    /// Stop service(s) (down).
    Stop { services: Vec<String> },

    /// Restart service(s) (term, cont, up).
    Restart { services: Vec<String> },

    /// Reload service(s) (send SIGHUP).
    Reload { services: Vec<String> },

    /// View service log (tail -f).
    Log {
        service: String,
        /// Arguments passed to tail (e.g. -n 100)
        #[clap(allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Start if service is not running. Do not restart if it stops (once).
    Once { services: Vec<String> },

    /// Send SIGSTOP (pause).
    Pause { services: Vec<String> },

    /// Send SIGCONT (continue).
    Cont { services: Vec<String> },

    /// Send SIGHUP.
    Hup { services: Vec<String> },

    /// Send SIGALRM.
    Alarm { services: Vec<String> },

    /// Send SIGINT.
    Interrupt { services: Vec<String> },

    /// Send SIGQUIT.
    Quit { services: Vec<String> },

    /// Send SIGTERM.
    Term { services: Vec<String> },

    /// Send SIGKILL.
    Kill { services: Vec<String> },

    /// Send SIGTERM and exit (exit).
    Exit { services: Vec<String> },
}

pub fn parse() -> Args {
    Args::parse()
}
