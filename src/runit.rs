/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Runit service related structs and enums.

use libc::pid_t;
use path::{Path, PathBuf};
use std::convert::TryInto;
use std::fs;
use std::io;
use std::io::Read;
use std::path;
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
 *
 * This struct defines an object that can represent an individual service for
 * Runit.
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
        // "/<svdir>/<service>/down"
        let p = self.path.join("down");

        !p.exists()
    }

    /// Enable the service.
    pub fn enable(&self) -> Result<()> {
        // "/<svdir>/<service>/down"
        let p = self.path.join("down");

        if let Err(err) = fs::remove_file(p) {
            // allow ENOENT to be considered success as well
            match err.kind() {
                io::ErrorKind::NotFound => return Ok(()),
                _ => return Err(err.into()),
            };
        };

        Ok(())
    }

    /// Disable the service.
    pub fn disable(&self) -> Result<()> {
        // "/<svdir>/<service>/down"
        let p = self.path.join("down");

        fs::File::create(p)?;

        Ok(())
    }

    /// Parse the binary status file "supervise/status"
    pub fn get_status(&self) -> Result<RunitStatus> {
        let p = self.path.join("supervise").join("status");
        let mut f = fs::File::open(&p)?;
        let mut buf = [0u8; 20];
        f.read_exact(&mut buf)?;

        // Byte 19: State (0: Down, 1: Run, 2: Finish)
        let state = match buf[19] {
            0 => RunitServiceState::Down,
            1 => RunitServiceState::Run,
            2 => RunitServiceState::Finish,
            _ => RunitServiceState::Unknown,
        };

        // Bytes 12-15: PID (Little Endian)
        let pid_raw = u32::from_le_bytes(buf[12..16].try_into()?);
        let pid = if pid_raw > 0 { Some(pid_raw as pid_t) } else { None };

        // Byte 16: Paused
        let paused = buf[16] == 1;

        // Byte 17: Want ('u': up, 'd': down)
        let want = buf[17] as char;

        // Bytes 0-7: TAI64 Timestamp (Big Endian)
        // TAI64 is 64-bit seconds. Runit uses an offset of 4611686018427387914ULL
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
}

/**
 * List the services in a given runit service directory.
 *
 * This function optionally allows you to specify the `log` boolean.  If set,
 * this will return the correponding log service for each base-level service
 * found.
 *
 * You may also specify an optional filter to only allow services that contain a
 * given string.
 */
pub fn get_services<T>(
    path: &Path,
    log: bool,
    filter: Option<T>,
) -> Result<Vec<RunitService>>
where
    T: AsRef<str>,
{
    // loop services directory and collect service names
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
            let name = "- log";
            let service = RunitService::new(name, &p);
            dirs.push(service);
        }
    }

    dirs.sort();

    Ok(dirs)
}
