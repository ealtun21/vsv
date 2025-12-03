/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: January 26, 2022
 * License: MIT
 */

//! Contains various util functions for vsv.

use libc::pid_t;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
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

    for (i, (field, max_width, suffix)) in data.iter().enumerate() {
        let (text, style) = field;
        let text = text.as_ref();

        let char_count = text.chars().count();
        let suffix_len = suffix.chars().count();

        // Truncate logic: ensure the resulting string (including suffix)
        // fits within max_width to maintain alignment.
        let s = if *max_width > 0 {
            if char_count > *max_width {
                // Calculate how much text we can keep
                let limit = (*max_width).saturating_sub(suffix_len);

                let mut s: String = text.chars().take(limit).collect();
                s.push_str(suffix);
                s
            } else {
                String::from(text)
            }
        } else {
            String::from(text)
        };

        // don't paint spaces
        line.push_str(&s.paint(*style).to_string());

        // spacing
        if i < data.len() - 1 {
            // Calculate padding based on the formatted string 's'
            let width =
                if *max_width > 0 { *max_width } else { s.chars().count() };
            let current_len = s.chars().count();
            let pad = width.saturating_sub(current_len);

            // Use +2 spacing to match the original vsv look and separate columns clearly
            let spacing = " ".repeat(pad + 2);
            line.push_str(&spacing);
        }
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
/// Replaces the external `pstree` command completely.
/// Includes support for displaying threads and full arguments.
pub fn get_pstree(root_pid: pid_t, proc_path: &Path) -> Result<String> {
    let mut procs: HashMap<pid_t, ProcNode> = HashMap::new();
    let mut children_map: HashMap<pid_t, Vec<pid_t>> = HashMap::new();

    // 1. Scan /proc to build the world of processes
    // We scan everything because we need to find children of the root and their children.
    let proc_dir = fs::read_dir(proc_path).context("failed to read /proc")?;

    for entry in proc_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // Check if directory name is a PID
        let pid_str = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) if s.chars().all(char::is_numeric) => s,
            _ => continue,
        };
        let pid: pid_t = pid_str.parse().unwrap_or(0);

        // Read /proc/<pid>/stat to get PPID
        let stat_path = path.join("stat");
        let stat_content = match fs::read_to_string(&stat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Parse PPID (field 4)
        // Format: pid (comm) state ppid ...
        // We handle 'comm' potentially containing spaces and parenthesis by finding the last ')'
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

        // Get Command / Name
        // Try cmdline first (full args), fall back to stat comm (short name)
        let cmdline_path = path.join("cmdline");
        let name = if let Ok(mut cmd) = fs::read_to_string(&cmdline_path) {
            if cmd.is_empty() {
                // Fallback to comm from stat
                if let (Some(l), Some(r)) =
                    (stat_content.find('('), stat_content.rfind(')'))
                {
                    stat_content[l + 1..r].to_string()
                } else {
                    format!("{}", pid)
                }
            } else {
                // Replace nulls with space, trim
                cmd = cmd.replace('\0', " ");
                cmd.trim().to_string()
            }
        } else {
            // Fallback
            format!("{}", pid)
        };

        procs.insert(
            pid,
            ProcNode { pid, ppid, name: name.clone(), is_thread: false },
        );
        children_map.entry(ppid).or_default().push(pid);

        // Scan Tasks (Threads)
        // /proc/<pid>/task/ contains threads.
        // Threads appear as processes but we want to mark them visually.
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
                } // Skip the main thread (already added as process)

                // Get thread name (comm)
                let t_stat_path = t_path.join("stat");
                if let Ok(t_stat) = fs::read_to_string(t_stat_path) {
                    if let (Some(l), Some(r)) =
                        (t_stat.find('('), t_stat.rfind(')'))
                    {
                        let t_comm = t_stat[l + 1..r].to_string();
                        let t_name = format!("{{{}}}", t_comm); // Wrapped in {}

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

    // 2. Build Tree String recursively
    let mut out = String::new();

    // Check if root exists
    if let Some(root_node) = procs.get(&root_pid) {
        // Print the root node first (without prefix)
        out.push_str(&root_node.name);
        out.push('\n');

        // Recurse children
        // We need to pass a "seen" set to avoid infinite loops if PID cycles exist (unlikely in /proc but good practice)
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
        // Sort children: Threads last, then by PID usually, or alphabetical.
        // pstree usually sorts by name? Let's sort by PID for stability.
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

                // Add current child line
                out.push_str(&format!(
                    "{}{}{}\n",
                    prefix, connector, child_node.name
                ));

                // Recurse
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

/// Follow a file (tail -f) and print to stdout.
pub fn follow_file(path: &Path) -> Result<()> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open log file {:?}", path))?;

    // Print last 10 lines initially
    let file_len = file.metadata()?.len();
    let initial_read_size = 4096;
    let start_pos = file_len.saturating_sub(initial_read_size);

    file.seek(SeekFrom::Start(start_pos))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    let content = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = content.lines().collect();
    let start_line = if lines.len() > 10 { lines.len() - 10 } else { 0 };

    for line in &lines[start_line..] {
        println!("{}", line);
    }

    // Follow
    let mut pos = file.seek(SeekFrom::End(0))?;
    let mut buffer = [0; 1024];

    loop {
        let read_bytes = file.read(&mut buffer)?;
        if read_bytes > 0 {
            print!("{}", String::from_utf8_lossy(&buffer[..read_bytes]));
            pos += read_bytes as u64;
        } else {
            if let Ok(meta) = fs::metadata(path) {
                if meta.len() < pos {
                    // Truncated, reset
                    pos = 0;
                    file = File::open(path)?;
                    file.seek(SeekFrom::Start(0))?;
                    println!("\n*** Log rotated ***\n");
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
