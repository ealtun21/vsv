/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Runit service related structs and enums.

use libc::pid_t;
use std::convert::TryInto;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time;

use anyhow::{anyhow, Context, Result};

/// Possible states for a runit service.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Copy, Clone)]
pub enum RunitServiceState {
    Run,
    Down,
    Finish,
    Unknown,
}

/// Control commands (merged from rsv logic)
#[derive(Debug, Copy, Clone)]
pub enum RunitCommand {
    Up,
    Down,
    Once,
    Pause,
    Cont,
    Hup,
    Alarm,
    Interrupt,
    Quit,
    Term,
    Kill,
    Exit,
}

impl RunitCommand {
    pub fn to_char(&self) -> char {
        match self {
            RunitCommand::Up => 'u',
            RunitCommand::Down => 'd',
            RunitCommand::Once => 'o',
            RunitCommand::Pause => 'p',
            RunitCommand::Cont => 'c',
            RunitCommand::Hup => 'h',
            RunitCommand::Alarm => 'a',
            RunitCommand::Interrupt => 'i',
            RunitCommand::Quit => 'q',
            RunitCommand::Term => 't',
            RunitCommand::Kill => 'k',
            RunitCommand::Exit => 'x',
        }
    }
}

/// Struct representing the parsed binary status
#[derive(Debug)]
pub struct RunitStatus {
    pub state: RunitServiceState,
    pub pid: Option<pid_t>,
    pub start_time: Option<time::SystemTime>,
    pub want: char,
    pub paused: bool,
}

/**
 * A runit service.
 */
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RunitService {
    pub path: PathBuf,
    pub name: String,
}

impl RunitService {
    /// Create a new runit service object from a given path and name.
    pub fn new(name: &str, path: &Path) -> Self {
        let name = name.to_string();
        let path = path.to_path_buf();
        Self { path, name }
    }

    /// Check if service is valid.
    pub fn valid(&self) -> bool {
        let p = self.path.join("supervise");
        p.exists()
    }

    /// Check if a service is enabled.
    pub fn enabled(&self) -> bool {
        let p = self.path.join("down");
        !p.exists()
    }

    /// Enable the service.
    pub fn enable(&self) -> Result<()> {
        let p = self.path.join("down");
        if let Err(err) = fs::remove_file(p) {
            match err.kind() {
                io::ErrorKind::NotFound => return Ok(()),
                _ => return Err(err.into()),
            };
        };
        Ok(())
    }

    /// Disable the service.
    pub fn disable(&self) -> Result<()> {
        let p = self.path.join("down");
        fs::File::create(p)?;
        Ok(())
    }

    /// Send a control command to the service pipe.
    pub fn control(&self, cmd: RunitCommand) -> Result<()> {
        let pipe_path = self.path.join("supervise").join("control");

        // rsv checks for supervise/ok, but supervise/control existing usually implies ok.
        if !pipe_path.exists() {
            return Err(anyhow!(
                "control pipe does not exist (service not supervised?)"
            ));
        }

        let mut f =
            OpenOptions::new().write(true).open(&pipe_path).with_context(
                || format!("failed to open control pipe {:?}", pipe_path),
            )?;

        let c = cmd.to_char();
        f.write_all(&[c as u8])?;
        Ok(())
    }

    /// Parse the binary status file "supervise/status"
    pub fn get_status(&self) -> Result<RunitStatus> {
        let p = self.path.join("supervise").join("status");
        let mut f = fs::File::open(&p)?;
        let mut buf = [0u8; 20];
        f.read_exact(&mut buf)?;

        let state = match buf[19] {
            0 => RunitServiceState::Down,
            1 => RunitServiceState::Run,
            2 => RunitServiceState::Finish,
            _ => RunitServiceState::Unknown,
        };

        let pid_raw = u32::from_le_bytes(buf[12..16].try_into()?);
        let pid = if pid_raw > 0 { Some(pid_raw as pid_t) } else { None };

        let paused = buf[16] == 1;
        let want = buf[17] as char;

        let tai = u64::from_be_bytes(buf[0..8].try_into()?);
        let offset = 4611686018427387914u64;

        let start_time = if tai >= offset {
            let secs = tai - offset;
            Some(time::SystemTime::UNIX_EPOCH + time::Duration::from_secs(secs))
        } else {
            None
        };

        Ok(RunitStatus { state, pid, start_time, want, paused })
    }

    /// Get status of the associated log service if it exists
    pub fn get_log_status(&self) -> Result<RunitStatus> {
        let log_path = self.path.join("log");
        if !log_path.exists() {
            return Err(anyhow!("no log service found"));
        }
        let log_svc = RunitService::new("log", &log_path);
        log_svc.get_status()
    }

    /// Check if the associated log service is running
    pub fn log_running(&self) -> bool {
        match self.get_log_status() {
            Ok(s) => s.pid.is_some(),
            Err(_) => false,
        }
    }
}

/**
 * List the services in a given runit service directory.
 */
pub fn get_services<T>(
    path: &Path,
    log: bool,
    filter: Option<T>,
) -> Result<Vec<RunitService>>
where
    T: AsRef<str>,
{
    let mut dirs = Vec::new();

    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read dir {:?}", path))?
    {
        let entry = entry?;
        let p = entry.path();

        if !p.is_dir() {
            continue;
        }

        let name = p
            .file_name()
            .ok_or_else(|| anyhow!("{:?}: failed to get service name", p))?
            .to_str()
            .ok_or_else(|| anyhow!("{:?}: failed to parse service name", p))?
            .to_string();

        if let Some(ref filter) = filter {
            if !name.contains(filter.as_ref()) {
                continue;
            }
        }

        let service = RunitService::new(&name, &p);
        dirs.push(service);

        if log {
            let p = entry.path().join("log");
            if p.exists() {
                let name = "- log";
                let service = RunitService::new(name, &p);
                dirs.push(service);
            }
        }
    }

    dirs.sort();

    Ok(dirs)
}
