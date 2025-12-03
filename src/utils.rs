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
                let limit = if *max_width > suffix_len { 
                    *max_width - suffix_len 
                } else { 
                    0 
                };
                
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
            let width = if *max_width > 0 { *max_width } else { s.chars().count() };
            let current_len = s.chars().count();
            let pad = if width > current_len { width - current_len } else { 0 };
            
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
                if let (Some(l), Some(r)) = (stat_content.find('('), stat_content.rfind(')')) {
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

        procs.insert(pid, ProcNode { pid, ppid, name: name.clone(), is_thread: false });
        children_map.entry(ppid).or_default().push(pid);

        let task_path = path.join("task");
        if let Ok(task_dir) = fs::read_dir(task_path) {
            for task_entry in task_dir.flatten() {
                let t_path = task_entry.path();
                let tid_str = match t_path.file_name().and_then(|s| s.to_str()) {
                    Some(s) if s.chars().all(char::is_numeric) => s,
                    _ => continue,
                };
                let tid: pid_t = tid_str.parse().unwrap_or(0);

                if tid == pid { continue; } 

                let t_stat_path = t_path.join("stat");
                if let Ok(t_stat) = fs::read_to_string(t_stat_path) {
                    if let (Some(l), Some(r)) = (t_stat.find('('), t_stat.rfind(')')) {
                        let t_comm = t_stat[l + 1..r].to_string();
                        let t_name = format!("{{{}}}", t_comm);

                        procs.insert(tid, ProcNode { pid: tid, ppid: pid, name: t_name, is_thread: true });
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
        
        build_tree_recursive(root_pid, &procs, &children_map, &mut out, "", &mut seen);
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
            if seen.contains(&child_pid) { continue; }
            seen.insert(child_pid);

            if let Some(child_node) = procs.get(&child_pid) {
                let is_last = i == count - 1;
                let connector = if is_last { "└─" } else { "├─" };
                let child_prefix = if is_last { "  " } else { "│ " };
                
                out.push_str(&format!("{}{}{}\n", prefix, connector, child_node.name));

                let new_prefix = format!("{}{}", prefix, child_prefix);
                build_tree_recursive(child_pid, procs, children_map, out, &new_prefix, seen);
            }
        }
    }
}

/// Helper: seek to end minus estimated size (or 0 if reading all), read content.
fn get_tail_content(path: &Path, n_lines: usize, read_all: bool) -> Result<(File, String)> {
    let mut file = File::open(path).with_context(|| format!("failed to open log file {:?}", path))?;

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

/// Follow a file (tail -f) and print to stdout.
pub fn follow_file(path: &Path, n_lines: usize, read_all: bool) -> Result<()> {
    let (mut file, content) = get_tail_content(path, n_lines, read_all)?;
    let lines: Vec<&str> = content.lines().collect();
    
    // If not reading all, only show the last N lines
    let start_line = if !read_all && lines.len() > n_lines { 
        lines.len() - n_lines 
    } else { 
        0 
    };

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
                    // Truncated
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

/// Follow a file (tail -f) but only print lines containing `filter_str`.
pub fn follow_file_filtered(path: &Path, filter_str: &str, n_lines: usize, read_all: bool) -> Result<()> {
    let (mut file, content) = get_tail_content(path, n_lines, read_all)?;
    
    // Scan existing content for the filter
    let mut matching_lines = Vec::new();
    for line in content.lines() {
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
                    partial_line.truncate(0); // Fixed deprecated clear() call
                    println!("\n*** Log rotated ***\n");
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
