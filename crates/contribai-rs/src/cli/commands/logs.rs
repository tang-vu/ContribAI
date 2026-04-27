//! Handles `Commands::Logs` — view the events.jsonl log.
//!
//! Reads `~/.contribai/events.jsonl` and shows recent events with
//! pretty formatting. Supports filtering by event type and JSON output
//! mode for scripting.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use contribai::core::events::Event;

/// Path to the events log file (`~/.contribai/events.jsonl`).
fn events_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".contribai")
        .join("events.jsonl")
}

/// Run `contribai logs`.
///
/// - `tail`: number of most recent events to show (default 20)
/// - `filter`: optional substring to filter event_type by (case-insensitive)
/// - `json`: emit one raw JSON object per line instead of pretty output
pub fn run_logs(tail: usize, filter: Option<&str>, json: bool) -> Result<()> {
    let path = events_log_path();
    if !path.exists() {
        anyhow::bail!(
            "No events log found at {}. Run `contribai run` or `contribai hunt` first to generate events.",
            path.display()
        );
    }

    let file =
        File::open(&path).with_context(|| format!("opening events log at {}", path.display()))?;
    let reader = BufReader::new(file);

    // Stream-parse lines and keep only the last `tail` matches in a ring buffer.
    // Avoids loading huge logs into memory.
    let filter_lc = filter.map(|s| s.to_lowercase());
    let mut keep: VecDeque<Event> = VecDeque::with_capacity(tail);
    let mut total = 0usize;
    let mut matched = 0usize;
    let mut parse_errors = 0usize;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let event: Event = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };
        if let Some(ref f) = filter_lc {
            if !event.event_type.to_string().to_lowercase().contains(f) {
                continue;
            }
        }
        matched += 1;
        if keep.len() == tail {
            keep.pop_front();
        }
        keep.push_back(event);
    }

    if json {
        // Re-serialize each kept event as a single JSON line for scripting.
        for event in &keep {
            let line = serde_json::to_string(event)?;
            println!("{}", line);
        }
        return Ok(());
    }

    // Pretty mode: header, table-style listing, summary footer.
    println!("{}", "📋 ContribAI Events".cyan().bold());
    println!("{}", "━".repeat(60).dimmed());
    println!(
        "  {:<25} {}",
        "Path:".dimmed(),
        path.display().to_string().cyan()
    );
    println!(
        "  {:<25} {}",
        "Total events:".dimmed(),
        total.to_string().cyan()
    );
    if let Some(f) = filter {
        println!(
            "  {:<25} {} ({} matched)",
            "Filter:".dimmed(),
            f.cyan(),
            matched
        );
    }
    if parse_errors > 0 {
        println!(
            "  {:<25} {} (skipped)",
            "Parse errors:".dimmed(),
            parse_errors.to_string().yellow()
        );
    }
    println!();

    if keep.is_empty() {
        println!("  {}", "No events match the current filter.".dimmed());
        return Ok(());
    }

    println!(
        "  {} {}",
        "Showing last".dimmed(),
        keep.len().to_string().cyan()
    );
    println!("{}", "─".repeat(60).dimmed());
    for event in &keep {
        let ts = event.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        let kind = event.event_type.to_string();
        let kind_colored = if kind.contains("error") || kind.contains("Error") {
            kind.red()
        } else if kind.contains("complete") || kind.contains("merged") {
            kind.green()
        } else if kind.contains("start") {
            kind.cyan()
        } else {
            kind.normal()
        };

        let summary = compact_data(&event.data);
        if summary.is_empty() {
            println!(
                "  {} {} {}",
                ts.dimmed(),
                kind_colored,
                event.source.dimmed()
            );
        } else {
            println!(
                "  {} {} {} {}",
                ts.dimmed(),
                kind_colored,
                event.source.dimmed(),
                summary
            );
        }
    }
    println!();

    Ok(())
}

/// Render the event's data map as a single short line (key=value pairs).
/// Long string values are truncated to keep output readable in a terminal.
fn compact_data(data: &std::collections::HashMap<String, serde_json::Value>) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::with_capacity(data.len());
    for (k, v) in data {
        let val_str = match v {
            serde_json::Value::String(s) => {
                if s.len() > 40 {
                    format!("\"{}...\"", &s[..40])
                } else {
                    format!("\"{}\"", s)
                }
            }
            other => other.to_string(),
        };
        parts.push(format!("{}={}", k, val_str));
    }
    // Stable-ish ordering for readability across runs.
    parts.sort();
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_data_empty() {
        let m = std::collections::HashMap::new();
        assert_eq!(compact_data(&m), "");
    }

    #[test]
    fn compact_data_truncates_long_strings() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "msg".to_string(),
            serde_json::Value::String("a".repeat(100)),
        );
        let out = compact_data(&m);
        assert!(out.contains("..."));
        assert!(out.len() < 100);
    }

    #[test]
    fn compact_data_pairs_sorted() {
        let mut m = std::collections::HashMap::new();
        m.insert("z".to_string(), serde_json::Value::from(1));
        m.insert("a".to_string(), serde_json::Value::from(2));
        let out = compact_data(&m);
        assert!(out.starts_with("a="));
    }
}
