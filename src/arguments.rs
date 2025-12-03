/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 25, 2022
 * License: MIT
 */

//! Argument parsing logic (via `clap`) for vsv.

use std::path;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

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
pub struct Args {
    /// Enable or disable color output.
    #[clap(short, long, value_name = "yes|no|auto")]
    pub color: Option<String>,

    /// Directory to look into, defaults to env SVDIR or /var/service if unset.
    #[clap(short, long, value_parser, value_name = "dir")]
    pub dir: Option<path::PathBuf>,

    /// Turn on verbose output.
    #[clap(short, long, action = clap::ArgAction::Count)]
    pub verbose: usize,

    /// Turn on tree output.
    #[clap(short, long)]
    pub tree: bool,

    /// Show log status (in status mode).
    #[clap(short, long)]
    pub log: bool,

    /// Run in user mode (SVDIR = ~/runit/service).
    #[clap(short, long)]
    pub user: bool,

    #[clap(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, PartialEq, Debug)]
pub enum Commands {
    /// Show process status.
    Status {
        /// Tree view (calls pstree(1) on PIDs found).
        #[clap(short, long)]
        tree: bool,

        /// Show log status.
        #[clap(short, long)]
        log: bool,

        filter: Vec<String>,
    },

    /// Enable service(s).
    Enable { services: Vec<String> },

    /// Disable service(s).
    Disable { services: Vec<String> },

    /// Add service(s) (symlink from /etc/sv).
    Add { services: Vec<String> },

    /// Remove service(s) (remove symlink).
    Remove { services: Vec<String> },

    /// List all available services in /etc/sv.
    Avail,

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

        /// Number of lines to show (default: 10).
        #[clap(short = 'n', long)]
        lines: Option<usize>,

        /// Show the whole file (start from beginning).
        #[clap(short = 'a', long, conflicts_with = "lines")]
        all: bool,
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

    /// Exit the service immediately.
    Exit { services: Vec<String> },

    /// Generate shell completions.
    Completions {
        /// The shell to generate the completions for.
        #[clap(value_enum)]
        shell: Shell,
    },
}

pub fn parse() -> Args {
    Args::parse()
}
