//! Handles `Commands::Prs` — list submitted PRs from local memory.
//!
//! Reads the `submitted_prs` table via `Memory::get_prs`. The `stats`
//! command shows only the most recent 5; this command exposes the full
//! list with status filtering and a JSON mode for scripting.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;

use crate::cli::{create_memory, load_config};

/// Run `contribai prs`.
///
/// - `status`: "open" | "merged" | "closed" | "all" (case-insensitive). Default "all".
/// - `limit`: max rows to fetch from memory (default 20)
/// - `json`: emit JSON array instead of pretty output
pub fn run_prs(config_path: Option<&str>, status: &str, limit: usize, json: bool) -> Result<()> {
    let config = load_config(config_path)?;
    let memory = create_memory(&config)?;

    let normalized = status.trim().to_lowercase();
    let status_filter = match normalized.as_str() {
        "all" | "" => None,
        other => Some(other),
    };

    let prs = memory
        .get_prs(status_filter, limit)
        .context("reading submitted_prs from memory")?;

    if json {
        let arr = serde_json::Value::Array(prs.iter().map(map_to_json).collect::<Vec<_>>());
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }

    println!("{}", "📦 ContribAI Submitted PRs".cyan().bold());
    println!("{}", "━".repeat(60).dimmed());
    println!(
        "  {:<20} {}",
        "Status filter:".dimmed(),
        if status_filter.is_some() {
            normalized.cyan()
        } else {
            "all".cyan()
        }
    );
    println!(
        "  {:<20} {}",
        "Rows:".dimmed(),
        prs.len().to_string().cyan()
    );
    println!();

    if prs.is_empty() {
        println!("  {}", "No PRs match the current filter.".dimmed());
        return Ok(());
    }

    for pr in &prs {
        let status_str = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
        let pr_number = pr.get("pr_number").cloned().unwrap_or_default();
        let repo = pr.get("repo").cloned().unwrap_or_default();
        let title = pr.get("title").cloned().unwrap_or_default();
        let url = pr.get("pr_url").cloned().unwrap_or_default();
        let created = pr.get("created_at").cloned().unwrap_or_default();

        let status_colored = match status_str {
            "merged" => status_str.green().bold(),
            "open" => status_str.cyan().bold(),
            "closed" => status_str.red().bold(),
            "failed" => status_str.yellow().bold(),
            _ => status_str.dimmed().bold(),
        };

        println!(
            "  {} #{} {} [{}]",
            short_date(&created).dimmed(),
            pr_number.cyan(),
            repo.dimmed(),
            status_colored
        );
        println!("    {}", title);
        if !url.is_empty() {
            println!("    {}", url.blue().underline());
        }
        println!();
    }

    Ok(())
}

/// Convert a memory row (HashMap<String,String>) into a JSON object.
/// PR number is parsed to a number when possible so JSON consumers can
/// sort/compare numerically.
fn map_to_json(row: &HashMap<String, String>) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(row.len());
    for (k, v) in row {
        if k == "pr_number" {
            if let Ok(n) = v.parse::<i64>() {
                obj.insert(k.clone(), serde_json::Value::from(n));
                continue;
            }
        }
        obj.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    serde_json::Value::Object(obj)
}

/// Trim a YYYY-MM-DDTHH:MM:SS timestamp down to YYYY-MM-DD HH:MM for terminal display.
/// Returns the input unchanged if it's shorter than expected.
fn short_date(s: &str) -> String {
    // Tolerate either "T" or " " between date and time.
    let s = s.replace('T', " ");
    if s.len() >= 16 {
        s[..16].to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_to_json_parses_pr_number_as_int() {
        let mut row = HashMap::new();
        row.insert("pr_number".to_string(), "42".to_string());
        row.insert("title".to_string(), "fix bug".to_string());
        let v = map_to_json(&row);
        assert_eq!(v["pr_number"], serde_json::json!(42));
        assert_eq!(v["title"], serde_json::json!("fix bug"));
    }

    #[test]
    fn map_to_json_keeps_pr_number_as_string_when_unparseable() {
        let mut row = HashMap::new();
        row.insert("pr_number".to_string(), "n/a".to_string());
        let v = map_to_json(&row);
        assert_eq!(v["pr_number"], serde_json::json!("n/a"));
    }

    #[test]
    fn short_date_trims_iso_timestamp() {
        assert_eq!(short_date("2026-04-27T10:23:58Z"), "2026-04-27 10:23");
    }

    #[test]
    fn short_date_passthrough_when_shorter_than_expected() {
        assert_eq!(short_date("2026-04-27"), "2026-04-27");
    }
}
