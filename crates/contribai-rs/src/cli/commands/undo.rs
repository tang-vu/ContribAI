//! Handles `Commands::Undo` — rollback last generated file changes.

use crate::cli::{load_config, print_banner};
use colored::Colorize;
use console::style;

pub fn run_undo(config_path: Option<&str>, yes: bool) -> anyhow::Result<()> {
    print_banner();
    let config = load_config(config_path)?;

    let db_path = config.storage.resolved_db_path();
    let snap_db_path = db_path.with_file_name("snapshots.db");

    if !snap_db_path.exists() {
        println!("{}", style("📭 No snapshots found — nothing to undo").dim());
        return Ok(());
    }

    let mgr = contribai::core::snapshots::SnapshotManager::new(&snap_db_path)?;
    let total = mgr.count()?;

    if total == 0 {
        println!(
            "{}",
            style("📭 No snapshots recorded — nothing to undo").dim()
        );
        return Ok(());
    }

    println!(
        "{} {} snapshot(s) recorded",
        style("📋").bold(),
        total.to_string().cyan()
    );

    // Get latest snapshot
    let latest = mgr.get_latest("latest", None)?;
    if let Some(snap) = latest {
        println!();
        println!("Latest snapshot:");
        println!("  Repo:     {}", style(&snap.repo).cyan());
        println!("  Path:     {}", style(&snap.path).cyan());
        println!("  Time:     {}", style(&snap.timestamp).dim());
        if let Some(ref before) = snap.before {
            println!("  Before:   {} chars", before.len());
        } else {
            println!("  Before:   (new file)");
        }
        println!("  After:    {} chars", snap.after.len());

        if !yes {
            let confirmed = dialoguer::Confirm::new()
                .with_prompt("Rollback this change?")
                .default(false)
                .interact()?;
            if !confirmed {
                println!("{}", style("Cancelled").dim());
                return Ok(());
            }
        }

        // Restore the before content if we have a file to write to
        if let Some(ref before) = snap.before {
            let file_path = std::path::Path::new(&snap.path);
            if let Err(e) = std::fs::write(file_path, before) {
                println!(
                    "{} Failed to restore {}: {}",
                    style("❌").red(),
                    snap.path,
                    e
                );
            } else {
                println!("{} Restored {}", style("✅").green(), snap.path);
            }
        } else {
            // New file — delete it
            let file_path = std::path::Path::new(&snap.path);
            if file_path.exists() {
                if let Err(e) = std::fs::remove_file(file_path) {
                    println!(
                        "{} Failed to delete new file {}: {}",
                        style("❌").red(),
                        snap.path,
                        e
                    );
                } else {
                    println!("{} Deleted new file {}", style("✅").green(), snap.path);
                }
            }
        }

        // Clear snapshot from DB
        mgr.clear_repo(&snap.repo)?;
        println!("{} Snapshot removed", style("🗑️").dim());
    } else {
        println!("{}", style("📭 No snapshots found").dim());
    }

    Ok(())
}
