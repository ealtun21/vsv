/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Contains various util functions for vsv.

use libc::pid_t;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use yansi::{Paint, Style};

use crate::config;

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
    // We add a "  " suffix to enforce a gap between columns.
    let data = [
        (status_char.0.as_ref(), status_char.1, 1, "  "),
        (name.0.as_ref(), name.1, 20, "  "),
        (state.0.as_ref(), state.1, 7, "  "),
        (enabled.0.as_ref(), enabled.1, 9, "  "),
        (pid.0.as_ref(), pid.1, 8, "  "),
        (command.0.as_ref(), command.1, 17, "  "),
        (time.0.as_ref(), time.1, 9, "  "),
        (log.0.as_ref(), log.1, 7, ""), // Last column has no suffix
    ];

    let mut line = String::new();

    for (_i, (s, style, width, suffix)) in data.iter().enumerate() {
        let mut s = s.to_string();
        let char_count = s.chars().count();

        // truncate long strings safely (by character count, not bytes)
        if char_count > *width {
             // Find the byte index where the *width*-th character starts
             if let Some((idx, _)) = s.char_indices().nth(*width) {
                 s.truncate(idx);
             }
        }

        // Recalculate char_count after truncation for padding logic
        let char_count = s.chars().count();

        // construct the string with the style
        let s_painted = s.paint(*style).to_string();

        // calculate the padding safely
        // We want 'width' visual columns.
        let padding = if *width > char_count {
            *width - char_count
        } else {
            0
        };

        // Left Align: String first, then Padding
        // This ensures headers ("SERVICE") and values ("NetworkManager") start 
        // at the same column.
        line.push_str(&s_painted);
        let pad_str = " ".repeat(padding);
        line.push_str(&pad_str);

        // append the suffix (the gap)
        line.push_str(suffix);
    }

    line
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
        (secs / 60 / 60 / 24, "day"),
        (secs / 60 / 60, "hour"),
        (secs / 60, "minute"),
        (secs, "second"),
    ];

    let mut s = String::new();

    for (num, name) in v.iter() {
        if *num > 1 {
            s = format!("{} {}s", num, name);
        } else if *num > 0 {
            s = format!("{} {}", num, name);
        }

        if !s.is_empty() {
            break;
        }
    }

    if s.is_empty() {
        s = String::from("0 seconds");
    }

    s
}

/// Get the command line for a PID from /proc
pub fn get_command_from_pid(pid: pid_t, proc_path: &Path) -> Result<String> {
    let path = proc_path.join(pid.to_string()).join("cmdline");
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {:?}", path))?;

    // cmdline is null-separated, replace nulls with spaces
    let cmd = content.replace('\0', " ");
    Ok(cmd.trim().to_string())
}

/// Helper struct to hold process information
#[derive(Debug, Clone)]
struct ProcNode {
    pid: pid_t,
    ppid: pid_t,
    name: String,
    is_thread: bool,
}

/// Generate a process tree string for a given PID by reading /proc manually.
pub fn get_pstree(root_pid: pid_t, proc_path: &Path) -> Result<String> {
    let mut procs: HashMap<pid_t, ProcNode> = HashMap::new();
    let mut children_map: HashMap<pid_t, Vec<pid_t>> = HashMap::new();

    let proc_dir = fs::read_dir(proc_path).context("failed to read /proc")?;

    for entry in proc_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        let pid_str = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) if s.chars().all(char::is_numeric) => s,
            _ => continue,
        };
        let pid: pid_t = pid_str.parse().unwrap_or(0);

        let stat_path = path.join("stat");
        let stat_content = match fs::read_to_string(&stat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let ppid = if let Some(r_paren) = stat_content.rfind(')') {
            let rest = &stat_content[r_paren + 2..];
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                parts[1].parse::<pid_t>().unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        let cmdline_path = path.join("cmdline");
        let name = if let Ok(mut cmd) = fs::read_to_string(&cmdline_path) {
            if cmd.is_empty() {
                if let (Some(l), Some(r)) =
                    (stat_content.find('('), stat_content.rfind(')'))
                {
                    stat_content[l + 1..r].to_string()
                } else {
                    format!("{}", pid)
                }
            } else {
                cmd = cmd.replace('\0', " ");
                cmd.trim().to_string()
            }
        } else {
            format!("{}", pid)
        };

        procs.insert(
            pid,
            ProcNode { pid, ppid, name: name.clone(), is_thread: false },
        );
        children_map.entry(ppid).or_default().push(pid);

        let task_path = path.join("task");
        if let Ok(task_dir) = fs::read_dir(task_path) {
            for task_entry in task_dir.flatten() {
                let t_path = task_entry.path();
                let tid_str = match t_path.file_name().and_then(|s| s.to_str())
                {
                    Some(s) if s.chars().all(char::is_numeric) => s,
                    _ => continue,
                };
                let tid: pid_t = tid_str.parse().unwrap_or(0);

                if tid == pid {
                    continue;
                }

                let t_stat_path = t_path.join("stat");
                if let Ok(t_stat) = fs::read_to_string(t_stat_path) {
                    if let (Some(l), Some(r)) =
                        (t_stat.find('('), t_stat.rfind(')'))
                    {
                        let t_comm = t_stat[l + 1..r].to_string();
                        let t_name = format!("{{{}}}", t_comm);

                        procs.insert(
                            tid,
                            ProcNode {
                                pid: tid,
                                ppid: pid,
                                name: t_name,
                                is_thread: true,
                            },
                        );
                        children_map.entry(pid).or_default().push(tid);
                    }
                }
            }
        }
    }

    let mut out = String::new();
    if let Some(root_node) = procs.get(&root_pid) {
        out.push_str(&root_node.name);
        out.push('\n');

        let mut seen = HashSet::new();
        seen.insert(root_pid);

        build_tree_recursive(
            root_pid,
            &procs,
            &children_map,
            &mut out,
            "",
            &mut seen,
        );
    } else {
        return Ok(String::new());
    }

    Ok(out.trim_end().to_string())
}

fn build_tree_recursive(
    pid: pid_t,
    procs: &HashMap<pid_t, ProcNode>,
    children_map: &HashMap<pid_t, Vec<pid_t>>,
    out: &mut String,
    prefix: &str,
    seen: &mut HashSet<pid_t>,
) {
    if let Some(children) = children_map.get(&pid) {
        let mut sorted_children = children.clone();
        sorted_children.sort_by_key(|&p| p);

        let count = sorted_children.len();
        for (i, &child_pid) in sorted_children.iter().enumerate() {
            if seen.contains(&child_pid) {
                continue;
            }
            seen.insert(child_pid);

            if let Some(child_node) = procs.get(&child_pid) {
                let is_last = i == count - 1;
                let connector = if is_last { "└─" } else { "├─" };
                let child_prefix = if is_last { "  " } else { "│ " };

                out.push_str(&format!(
                    "{}{}{}\n",
                    prefix, connector, child_node.name
                ));

                let new_prefix = format!("{}{}", prefix, child_prefix);
                build_tree_recursive(
                    child_pid,
                    procs,
                    children_map,
                    out,
                    &new_prefix,
                    seen,
                );
            }
        }
    }
}

/// Helper: seek to end minus estimated size (or 0 if reading all), read content.
fn get_tail_content(
    path: &Path,
    n_lines: usize,
    read_all: bool,
) -> Result<(File, String)> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open log file {:?}", path))?;

    let file_len = file.metadata()?.len();

    // Estimate bytes needed: avg 200 bytes per line + standard buffer
    let estimated_bytes = (n_lines as u64) * 200;
    let initial_read_size = std::cmp::max(8192, estimated_bytes);

    // If reading all, start at 0. Else, try to be smart.
    let start_pos = if read_all {
        0
    } else if file_len > initial_read_size {
        file_len - initial_read_size
    } else {
        0
    };

    file.seek(SeekFrom::Start(start_pos))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    Ok((file, String::from_utf8_lossy(&buf).to_string()))
}

/**
 * Tail a file and print the lines to stdout.
 *
 * This function will run forever until interrupted.
 */
pub fn follow_file(
    path: &Path,
    n_lines: usize,
    read_all: bool,
) -> Result<()> {
    follow_file_filtered(path, "", n_lines, read_all)
}

/**
 * Tail a file and print the lines to stdout, filtered by a string.
 */
pub fn follow_file_filtered(
    path: &Path,
    filter_str: &str,
    n_lines: usize,
    read_all: bool,
) -> Result<()> {
    let (mut file, content) = get_tail_content(path, n_lines, read_all)?;

    let lines: Vec<&str> = content.lines().collect();
    let mut matching_lines = Vec::new();

    for line in lines {
        if line.contains(filter_str) {
            matching_lines.push(line);
        }
    }

    let start_line = if !read_all && matching_lines.len() > n_lines {
        matching_lines.len() - n_lines
    } else {
        0
    };

    for line in &matching_lines[start_line..] {
        println!("{}", line);
    }

    // Follow
    let mut pos = file.seek(SeekFrom::End(0))?;
    let mut buffer = [0; 1024];
    let mut partial_line = String::new();

    loop {
        let read_bytes = file.read(&mut buffer)?;
        if read_bytes > 0 {
            let chunk = String::from_utf8_lossy(&buffer[..read_bytes]);
            partial_line.push_str(&chunk);

            // Process full lines
            while let Some(idx) = partial_line.find('\n') {
                let line: String = partial_line.drain(..idx + 1).collect();
                let trimmed = line.trim_end();

                if trimmed.contains(filter_str) {
                    println!("{}", trimmed);
                }
            }
            pos += read_bytes as u64;
        } else {
            if let Ok(meta) = fs::metadata(path) {
                if meta.len() < pos {
                    // Truncated
                    pos = 0;
                    file = File::open(path)?;
                    file.seek(SeekFrom::Start(0))?;
                    partial_line.truncate(0); 
                    println!("\n*** Log truncated ***\n");
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

// --- NEW COMPLETION UTILS ---

/**
 * Get a list of service names in a directory.
 * Used for dynamic autocompletion.
 */
pub fn get_service_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // We only care about directories or symlinks to directories
            if path.is_dir() || path.is_symlink() {
                if let Some(name) = path.file_name().and_then(OsStr::to_str) {
                    if !name.starts_with('.') {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    
    names.sort();
    names
}

/**
 * Get available services from default locations.
 * Uses SVDIR env var or defaults.
 */
pub fn get_running_services() -> Vec<String> {
    let svdir = std::env::var_os(config::ENV_SVDIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(config::DEFAULT_SVDIR));
    
    get_service_names(&svdir)
}

/**
 * Get available services from /etc/sv (or default avail dir).
 */
pub fn get_avail_services() -> Vec<String> {
    // Note: We don't have a specific env var for AVAIL_DIR in standard vsv,
    // but config.rs defines it. We'll use the constant default here for completion.
    let avail_dir = PathBuf::from(config::DEFAULT_AVAIL_DIR);
    get_service_names(&avail_dir)
}
