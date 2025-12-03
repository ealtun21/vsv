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

pub struct Service {
    name: String,
    state: ServiceState,
    enabled: bool,
    command: Option<String>,
    pid: Option<pid_t>,
    start_time: Result<time::SystemTime>,
    pstree: Option<String>,
    want: char,
    paused: bool,
    log_status: Option<(RunitStatus, bool)>, // (status, enabled)
    print_log_column: bool,
}

impl Service {
    pub fn from_runit_service(
        service: &RunitService,
        want_pstree: bool,
        want_log_status: bool,
        proc_path: &Path,
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

                (
                    state,
                    status.pid,
                    status.start_time.ok_or_else(|| anyhow!("no start time")),
                    status.want,
                    status.paused,
                )
            }
            Err(err) => {
                messages.push(format!("failed to get status: {}", err));
                (
                    ServiceState::Unknown,
                    None,
                    Err(anyhow!("failed to get status")),
                    ' ',
                    false,
                )
            }
        };

        let log_status = if want_log_status {
            match service.get_log_status() {
                Ok(status) => Some((status, service.log_running())),
                Err(err) => {
                    messages.push(format!("failed to get log status: {}", err));
                    None
                }
            }
        } else {
            None
        };

        let command = match pid {
            Some(pid) => match utils::get_command_from_pid(pid, proc_path) {
                Ok(cmd) => Some(cmd),
                Err(err) => {
                    messages.push(format!(
                        "failed to get command for pid {}: {}",
                        pid, err
                    ));
                    None
                }
            },
            None => None,
        };

        let pstree = if want_pstree {
            match pid {
                Some(pid) => match utils::get_pstree(pid, proc_path) {
                    Ok(tree) => Some(tree),
                    Err(err) => {
                        messages.push(format!("failed to get pstree: {}", err));
                        None
                    }
                },
                None => None,
            }
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

    fn format_name(&self) -> (String, Style) {
        (self.name.to_string(), Style::default())
    }

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
            ServiceState::Down => ("X".to_string(), style.fg(Color::Red)),
            ServiceState::Finish => ("X".to_string(), style.fg(Color::Red)),
            ServiceState::Unknown => ("?".to_string(), style.fg(Color::Yellow)),
        }
    }

    fn format_state(&self) -> (String, Style) {
        let style = Style::default();
        match self.state {
            ServiceState::Run => ("run".to_string(), style.fg(Color::Green)),
            ServiceState::Down => ("down".to_string(), style.fg(Color::Red)),
            ServiceState::Finish => {
                ("finish".to_string(), style.fg(Color::Yellow))
            }
            ServiceState::Unknown => {
                ("n/a".to_string(), style.fg(Color::Yellow))
            }
        }
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
            None => "---".to_string(),
        };
        (s, style)
    }

    fn format_command(&self) -> (String, Style) {
        let style = Style::default().fg(Color::Green);
        let s = match &self.command {
            Some(cmd) => cmd.clone(),
            None => "---".to_string(),
        };
        (s, style)
    }

    fn format_time(&self) -> (String, Style) {
        let style = Style::default().fg(Color::Cyan);
        // Fix for "cannot move out of self.start_time"
        let t = match &self.start_time {
            Ok(t) => match t.elapsed() {
                Ok(t) => t,
                Err(err) => return (err.to_string(), style.fg(Color::Red)),
            },
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

    fn format_log(&self) -> (String, Style) {
        if !self.print_log_column {
            return ("".to_string(), Style::default());
        }

        let style = Style::default();

        let (status, enabled) = match &self.log_status {
            Some(val) => val,
            None => return ("---".to_string(), style.dim()),
        };

        if !enabled {
            return ("off".to_string(), style.fg(Color::Red));
        }

        match status.pid {
            Some(pid) => (format!("✔ ({})", pid), style.fg(Color::Green)),
            None => ("X".to_string(), style.fg(Color::Red)),
        }
    }

    pub fn format_pstree(&self) -> (String, Style) {
        let style = Style::default();
        let tree_s = match &self.pstree {
            Some(tree) => tree.trim(),
            None => return ("".into(), style),
        };

        if tree_s.is_empty() {
            return ("".into(), style);
        }

        (format!("\n{}\n", tree_s), style.dim())
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
