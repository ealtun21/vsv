/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Generic service related structs and enums.

use libc::pid_t;
use std::fmt;
use std::path::Path;
use std::time;

use anyhow::{anyhow, Result};
use yansi::{Color, Style};

use crate::runit::{RunitService, RunitServiceState, RunitStatus};
use crate::utils;

/// Possible states for a service.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ServiceState {
    Run,
    Down,
    Finish,
    Unknown,
}

impl ServiceState {
    /// Get a suitable `yansi::Style` for the state.
    pub fn get_style(&self) -> Style {
        let style = Style::default();

        let color = match self {
            ServiceState::Run => Color::Green,
            ServiceState::Down => Color::Red,
            ServiceState::Finish => Color::Yellow,
            ServiceState::Unknown => Color::Yellow,
        };

        style.fg(color)
    }

    /// Get a suitable char for the state (as a `String`).
    pub fn get_char(&self) -> String {
        let s = match self {
            ServiceState::Run => "✔",
            ServiceState::Down => "X",
            ServiceState::Finish => "X",
            ServiceState::Unknown => "?",
        };

        s.to_string()
    }
}

impl fmt::Display for ServiceState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            ServiceState::Run => "run",
            ServiceState::Down => "down",
            ServiceState::Finish => "finish",
            ServiceState::Unknown => "n/a",
        };

        s.fmt(f)
    }
}

/**
 * A struct suitable for describing an abstract service.
 *
 * This struct itself doesn't do much - it just stores information about a
 * service and knows how to format it to look pretty.
 */
pub struct Service {
    name: String,
    state: ServiceState,
    enabled: bool,
    command: Option<String>,
    pid: Option<pid_t>,
    start_time: Result<time::SystemTime>,
    pstree: Option<Result<String>>,
    want: char,
    paused: bool,
    log_status: Option<(RunitStatus, bool)>, // (status, enabled)
    print_log_column: bool,
}

impl Service {
    /// Create a new service from a `RunitService`.
    pub fn from_runit_service(
        service: &RunitService,
        want_pstree: bool,
        want_log_status: bool,
        proc_path: &Path,
        pstree_prog: &str,
    ) -> (Self, Vec<String>) {
        let mut messages: Vec<String> = vec![];
        let name = service.name.to_string();
        let enabled = service.enabled();

        let status_result = service.get_status();

        let (state, pid, start_time, want, paused) = match status_result {
            Ok(status) => {
                let state = match status.state {
                    RunitServiceState::Run => ServiceState::Run,
                    RunitServiceState::Down => ServiceState::Down,
                    RunitServiceState::Finish => ServiceState::Finish,
                    RunitServiceState::Unknown => ServiceState::Unknown,
                };

                let time_res = status
                    .start_time
                    .ok_or_else(|| anyhow!("invalid timestamp"));

                (state, status.pid, time_res, status.want, status.paused)
            }
            Err(err) => (ServiceState::Unknown, None, Err(err), 'u', false),
        };

        // Check for log service status
        let log_path = service.path.join("log");
        let log_status = if log_path.exists() {
            let log_svc = RunitService::new("log", &log_path);
            match log_svc.get_status() {
                Ok(st) => Some((st, log_svc.enabled())),
                Err(_) => None,
            }
        } else {
            None
        };

        let mut command = None;
        if let Some(p) = pid {
            match utils::cmd_from_pid(p, proc_path) {
                Ok(cmd) => {
                    command = Some(cmd);
                }
                Err(err) => {
                    messages.push(format!(
                        "{:?}: failed to get command for pid {}: {:?}",
                        service.path, p, err
                    ));
                }
            };
        }

        let pstree = if want_pstree {
            pid.map(|pid| get_pstree(pid, pstree_prog))
        } else {
            None
        };

        let svc = Self {
            name,
            state,
            enabled,
            command,
            pid,
            start_time,
            pstree,
            want,
            paused,
            log_status,
            print_log_column: want_log_status,
        };

        (svc, messages)
    }

    /// Format the service name as a string.
    fn format_name(&self) -> (String, Style) {
        (self.name.to_string(), Style::default())
    }

    /// Format the service char as a string.
    fn format_status_char(&self) -> (String, Style) {
        let style = Style::default();

        match self.state {
            ServiceState::Run => {
                if self.paused {
                    ("⏸".to_string(), style.fg(Color::Yellow))
                } else if self.want == 'd' {
                    ("▼".to_string(), style.fg(Color::Yellow))
                } else {
                    ("✔".to_string(), style.fg(Color::Green))
                }
            }
            ServiceState::Down => {
                if self.want == 'u' {
                    ("X".to_string(), style.fg(Color::Red))
                } else {
                    ("■".to_string(), style.fg(Color::Yellow))
                }
            }
            ServiceState::Finish => {
                if self.paused {
                    ("⏸".to_string(), style.fg(Color::Magenta))
                } else if self.want == 'd' {
                    ("▼".to_string(), style.fg(Color::Magenta))
                } else {
                    ("▽".to_string(), style.fg(Color::Magenta))
                }
            }
            ServiceState::Unknown => ("?".to_string(), style.fg(Color::Yellow)),
        }
    }

    /// Helper to determine icon and style for a RunitStatus (used for log)
    fn get_runit_status_char(&self, status: &RunitStatus) -> (String, Style) {
        let style = Style::default();
        match status.state {
            RunitServiceState::Run => {
                if status.paused {
                    ("⏸".to_string(), style.fg(Color::Yellow))
                } else if status.want == 'd' {
                    ("▼".to_string(), style.fg(Color::Yellow))
                } else {
                    ("✔".to_string(), style.fg(Color::Green))
                }
            }
            RunitServiceState::Down => {
                if status.want == 'u' {
                    ("X".to_string(), style.fg(Color::Red))
                } else {
                    ("■".to_string(), style.fg(Color::Yellow))
                }
            }
            RunitServiceState::Finish => {
                if status.paused {
                    ("⏸".to_string(), style.fg(Color::Magenta))
                } else if status.want == 'd' {
                    ("▼".to_string(), style.fg(Color::Magenta))
                } else {
                    ("▽".to_string(), style.fg(Color::Magenta))
                }
            }
            RunitServiceState::Unknown => {
                ("?".to_string(), style.fg(Color::Yellow))
            }
        }
    }

    fn format_log(&self) -> (String, Style) {
        if !self.print_log_column {
            return ("".to_string(), Style::default());
        }

        let style = Style::default();
        match &self.log_status {
            Some((status, enabled)) => {
                let (mut icon, s) = self.get_runit_status_char(status);
                if !enabled {
                    icon.push_str(" -");
                }
                (icon, s)
            }
            None => ("-".to_string(), style.dim()),
        }
    }

    /// Format the service state as a string.
    fn format_state(&self) -> (String, Style) {
        let s = self.state.to_string();
        let style = Style::default();

        let color = match self.state {
            ServiceState::Run => {
                if self.paused || self.want == 'd' {
                    Color::Yellow
                } else {
                    Color::Green
                }
            }
            ServiceState::Down => {
                if self.want == 'u' {
                    Color::Red
                } else {
                    Color::Yellow
                }
            }
            ServiceState::Finish => Color::Magenta,
            ServiceState::Unknown => Color::Yellow,
        };

        (s, style.fg(color))
    }

    fn format_enabled(&self) -> (String, Style) {
        let style = match self.enabled {
            true => Style::default().fg(Color::Green),
            false => Style::default().fg(Color::Red),
        };
        (self.enabled.to_string(), style)
    }

    fn format_pid(&self) -> (String, Style) {
        let style = Style::default().fg(Color::Magenta);
        let s = match self.pid {
            Some(pid) => pid.to_string(),
            None => String::from("---"),
        };
        (s, style)
    }

    fn format_command(&self) -> (String, Style) {
        let style = Style::default().fg(Color::Green);
        let s = match &self.command {
            Some(cmd) => cmd.clone(),
            None => String::from("---"),
        };
        (s, style)
    }

    fn format_time(&self) -> (String, Style) {
        let style = Style::default();
        let time = match &self.start_time {
            Ok(time) => time,
            Err(err) => return (err.to_string(), style.fg(Color::Red)),
        };

        let t = match time.elapsed() {
            Ok(t) => t,
            Err(err) => return (err.to_string(), style.fg(Color::Red)),
        };

        let s = utils::relative_duration(&t);
        let style = match t.as_secs() {
            t if t < 5 => style.fg(Color::Red),
            t if t < 30 => style.fg(Color::Yellow),
            _ => style.dim(),
        };

        (s, style)
    }

    pub fn format_pstree(&self) -> (String, Style) {
        let style = Style::default();
        let tree = match &self.pstree {
            Some(tree) => tree,
            None => return ("".into(), style),
        };

        let (tree_s, style) = match tree {
            Ok(stdout) => (stdout.trim().into(), style.dim()),
            Err(err) => {
                (format!("pstree call failed: {}", err), style.fg(Color::Red))
            }
        };

        (format!("\n{}\n", tree_s), style)
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let base = utils::format_status_line(
            self.format_status_char(),
            self.format_name(),
            self.format_state(),
            self.format_enabled(),
            self.format_pid(),
            self.format_command(),
            self.format_time(),
            self.format_log(),
        );

        base.fmt(f)
    }
}

fn get_pstree(pid: pid_t, pstree_prog: &str) -> Result<String> {
    let cmd = pstree_prog.to_string();
    let args = ["-ac".to_string(), pid.to_string()];
    utils::run_program_get_output(&cmd, &args)
}
