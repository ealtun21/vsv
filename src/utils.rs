/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Contains various util functions for vsv.

use libc::pid_t;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use yansi::{Paint, Style};

/**
 * A `println!()`-like macro that will only print if `-v` is set.
 */
macro_rules! verbose {
    ($cfg:expr, $fmt:expr $(, $args:expr )* $(,)? ) => {
        if $cfg.verbose > 0 {
            let s = format!($fmt $(, $args)*);
            eprintln!(">  {}", s.dim());
        }
    };
}
pub(crate) use verbose;

/**
 * Format a status line - made specifically for vsv.
 */
pub fn format_status_line<T: AsRef<str>>(
    status_char: (T, Style),
    name: (T, Style),
    state: (T, Style),
    enabled: (T, Style),
    pid: (T, Style),
    command: (T, Style),
    time: (T, Style),
    log: (T, Style),
) -> String {
    // ( data + style to print, max width, suffix )
    let data = [
        (status_char, 1, ""),
        (name, 20, "..."),
        (state, 7, "..."),
        (enabled, 9, "..."),
        (pid, 8, "..."),
        (command, 17, "..."),
        (time, 14, "..."),
        (log, 0, "..."),
    ];

    let mut line = String::new();

    for ((text, style), max, suffix) in data {
        let column = if max == 0 {
            format!(" {}", text.as_ref().paint(style))
        } else {
            let text = trim_long_string(text.as_ref(), max, suffix);
            format!(" {0:1$}", text.paint(style), max)
        };

        line.push_str(&column);
    }

    line
}

/**
 * Get the program name (arg0) for a PID.
 */
pub fn cmd_from_pid(pid: pid_t, proc_path: &Path) -> Result<String> {
    // /<proc_path>/<pid>/cmdline
    let p = proc_path.join(pid.to_string()).join("cmdline");

    let data = fs::read_to_string(&p)
        .with_context(|| format!("failed to read pid file: {:?}", p))?;

    let first = data.split('\0').next();

    match first {
        Some(f) => Ok(f.to_string()),
        None => Err(anyhow!("failed to split cmdline data: {:?}", first)),
    }
}

/**
 * Run a program and get stdout.
 */
pub fn run_program_get_output<T1, T2>(cmd: &T1, args: &[T2]) -> Result<String>
where
    T1: AsRef<str>,
    T2: AsRef<str>,
{
    let output = make_command(cmd, args).output()?;

    if !output.status.success() {
        return Err(anyhow!("program '{}' returned non-zero", cmd.as_ref()));
    }

    let stdout = String::from_utf8(output.stdout)?;

    Ok(stdout)
}

/**
 * Run a program and get the exit status.
 */
pub fn run_program_get_status<T1, T2>(
    cmd: &T1,
    args: &[T2],
) -> Result<ExitStatus>
where
    T1: AsRef<str>,
    T2: AsRef<str>,
{
    let p = make_command(cmd, args).status()?;

    Ok(p)
}

/**
 * Create a `std::process::Command` from a given command name and argument
 * slice.
 */
fn make_command<T1, T2>(cmd: &T1, args: &[T2]) -> Command
where
    T1: AsRef<str>,
    T2: AsRef<str>,
{
    let mut c = Command::new(cmd.as_ref());

    for arg in args {
        c.arg(arg.as_ref());
    }

    c
}

/**
 * Convert a duration to a human-readable string like "5 minutes", "2 hours",
 * etc.
 */
pub fn relative_duration(t: &Duration) -> String {
    let secs = t.as_secs();

    let v = [
        (secs / 60 / 60 / 24 / 365, "year"),
        (secs / 60 / 60 / 24 / 30, "month"),
        (secs / 60 / 60 / 24 / 7, "week"),
        (secs / 60 / 60 / 24, "day"),
        (secs / 60 / 60, "hour"),
        (secs / 60, "minute"),
        (secs, "second"),
    ];

    let mut plural = "";
    for (num, name) in v {
        if num > 1 {
            plural = "s"
        }

        if num > 0 {
            return format!("{} {}{}", num, name, plural);
        }
    }

    String::from("0 seconds")
}

/**
 * Trim a string to be (at most) a certain number of characters with an
 * optional suffix.
 */
pub fn trim_long_string(s: &str, limit: usize, suffix: &str) -> String {
    let suffix_len = suffix.len();

    assert!(limit > suffix_len, "number too small");

    let len = s.len();

    // don't do anything if string is smaller than limit
    if len < limit {
        return s.to_string();
    }

    // make new string (without formatting)
    format!(
        "{}{}",
        s.chars().take(limit - suffix_len).collect::<String>(),
        suffix
    )
}
