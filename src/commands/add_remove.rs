/*
 * Author: Dave Eddy <dave@daveeddy.com>
 * Date: February 15, 2022
 * License: MIT
 */

//! `vsv add`, `vsv remove`, and `vsv avail`.

use std::fs;
use std::os::unix::fs::symlink;

use anyhow::{ensure, Context, Result};
use yansi::Paint;

use crate::config::Config;
use crate::runit::get_services;

/// Handle `vsv add`.
pub fn do_add(cfg: &Config) -> Result<()> {
    ensure!(!cfg.operands.is_empty(), "at least one (1) service required");

    let mut had_error = false;

    for name in &cfg.operands {
        let source = cfg.avail_dir.join(name);
        let target = cfg.svdir.join(name);

        print!("{} service {}... ", "adding".bold(), name.bold());

        if !source.exists() {
            println!(
                "{}",
                format!("failed! {} does not exist", source.display()).red()
            );
            had_error = true;
            continue;
        }

        if target.exists() {
            println!(
                "{}",
                "failed! service already added (target exists)".red()
            );
            had_error = true;
            continue;
        }

        // Create the symlink: /etc/sv/<name> -> /var/service/<name>
        match symlink(&source, &target) {
            Ok(()) => println!("{}", "done".green()),
            Err(err) => {
                println!("{}", format!("failed! {}", err).red());
                had_error = true;
            }
        }
    }

    ensure!(!had_error, "failed to add service(s)");

    Ok(())
}

/// Handle `vsv remove`.
pub fn do_remove(cfg: &Config) -> Result<()> {
    ensure!(!cfg.operands.is_empty(), "at least one (1) service required");

    let mut had_error = false;

    for name in &cfg.operands {
        let target = cfg.svdir.join(name);

        print!("{} service {}... ", "removing".bold(), name.bold());

        if !target.exists() {
            println!("{}", "failed! service not found".red());
            had_error = true;
            continue;
        }

        // Check if it is actually a symlink
        match fs::symlink_metadata(&target) {
            Ok(meta) => {
                if !meta.file_type().is_symlink() {
                    println!(
                        "{}",
                        format!(
                            "failed! {} is not a symlink",
                            target.display()
                        )
                        .red()
                    );
                    had_error = true;
                    continue;
                }
            }
            Err(err) => {
                println!(
                    "{}",
                    format!("failed! to stat {}: {}", target.display(), err)
                        .red()
                );
                had_error = true;
                continue;
            }
        }

        match fs::remove_file(&target) {
            Ok(()) => println!("{}", "done".green()),
            Err(err) => {
                println!("{}", format!("failed! {}", err).red());
                had_error = true;
            }
        }
    }

    ensure!(!had_error, "failed to remove service(s)");

    Ok(())
}

/// Handle `vsv avail`.
pub fn do_avail(cfg: &Config) -> Result<()> {
    // Get list of services in /etc/sv (avail_dir)
    // We pass `None::<&str>` to explicitly tell the compiler the type of the filter is &str
    let services = get_services(&cfg.avail_dir, false, None::<&str>)
        .context(format!("failed to list services in {:?}", cfg.avail_dir))?;

    println!(
        "{}",
        format!("Available services in {:?}:", cfg.avail_dir).bold()
    );

    // Calculate max length for alignment, ensuring a minimum of 20
    let name_width =
        services.iter().map(|s| s.name.len()).max().unwrap_or(0).max(20);

    println!("{: <width$} {: <10}", "SERVICE", "STATUS", width = name_width);

    for svc in services {
        let target = cfg.svdir.join(&svc.name);
        let status =
            if target.exists() { "added".green() } else { "avail".dim() };

        println!("{: <width$} {}", svc.name, status, width = name_width);
    }

    Ok(())
}
