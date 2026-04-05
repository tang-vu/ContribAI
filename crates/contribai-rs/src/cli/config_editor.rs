//! Config get/set — read and write individual config values by dotted key.
//!
//! Supports: `contribai config get llm.provider`
//!           `contribai config set llm.api_key sk-xxx`
//!           `contribai config set github.max_prs_per_day 20`

use console::style;
use std::path::{Path, PathBuf};

// ── Known keys with validation ────────────────────────────────────────────────

/// All supported dotted config keys with descriptions and optional valid values.
pub struct KeyDef {
    pub key: &'static str,
    pub description: &'static str,
    pub valid_values: Option<&'static [&'static str]>,
}

pub const KNOWN_KEYS: &[KeyDef] = &[
    KeyDef {
        key: "llm.provider",
        description: "LLM provider",
        valid_values: Some(&["gemini", "vertex", "openai", "anthropic", "ollama"]),
    },
    KeyDef {
        key: "llm.model",
        description: "Model name",
        valid_values: None,
    },
    KeyDef {
        key: "llm.api_key",
        description: "LLM API key (secret)",
        valid_values: None,
    },
    KeyDef {
        key: "llm.base_url",
        description: "Custom endpoint URL (for OpenAI/Anthropic-compatible APIs)",
        valid_values: None,
    },
    KeyDef {
        key: "llm.vertex_project",
        description: "Vertex AI GCP project ID",
        valid_values: None,
    },
    KeyDef {
        key: "llm.vertex_location",
        description: "Vertex AI location",
        valid_values: None,
    },
    KeyDef {
        key: "llm.temperature",
        description: "LLM temperature (0.0-1.0)",
        valid_values: None,
    },
    KeyDef {
        key: "llm.max_tokens",
        description: "Max tokens per LLM call",
        valid_values: None,
    },
    KeyDef {
        key: "github.token",
        description: "GitHub personal access token",
        valid_values: None,
    },
    KeyDef {
        key: "github.max_prs_per_day",
        description: "Max PRs to create per day",
        valid_values: None,
    },
    KeyDef {
        key: "github.max_repos_per_run",
        description: "Max repos per pipeline run",
        valid_values: None,
    },
    KeyDef {
        key: "github.rate_limit_buffer",
        description: "GitHub rate limit buffer",
        valid_values: None,
    },
    KeyDef {
        key: "web.host",
        description: "Web dashboard host",
        valid_values: None,
    },
    KeyDef {
        key: "web.port",
        description: "Web dashboard port",
        valid_values: None,
    },
    KeyDef {
        key: "web.enabled",
        description: "Web dashboard enabled",
        valid_values: Some(&["true", "false"]),
    },
    KeyDef {
        key: "scheduler.enabled",
        description: "Scheduler enabled",
        valid_values: Some(&["true", "false"]),
    },
    KeyDef {
        key: "scheduler.cron",
        description: "Cron expression",
        valid_values: None,
    },
    KeyDef {
        key: "pipeline.max_concurrent_repos",
        description: "Concurrent repos",
        valid_values: None,
    },
    KeyDef {
        key: "multi_model.enabled",
        description: "Multi-model routing enabled",
        valid_values: Some(&["true", "false"]),
    },
    KeyDef {
        key: "multi_model.strategy",
        description: "Routing strategy",
        valid_values: Some(&["performance", "balanced", "economy"]),
    },
    KeyDef {
        key: "discovery.languages",
        description: "Languages to discover (comma-separated in YAML)",
        valid_values: None,
    },
    KeyDef {
        key: "discovery.stars_range",
        description: "Stars range [min, max]",
        valid_values: None,
    },
    KeyDef {
        key: "discovery.max_results",
        description: "Max repos per search",
        valid_values: None,
    },
    KeyDef {
        key: "pipeline.risk_tolerance",
        description: "Risk tolerance (low/medium/high)",
        valid_values: Some(&["low", "medium", "high"]),
    },
];

const SECRET_KEYS: &[&str] = &["llm.api_key", "github.token", "web.webhook_secret"];

// ── Get ───────────────────────────────────────────────────────────────────────

/// Read a dotted key from a YAML config file.
pub fn get_config_value(path: &Path, key: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| anyhow::anyhow!("Config file not found: {}", path.display()))?;

    match extract_yaml_value(&content, key) {
        Some(val) => {
            let display = if SECRET_KEYS.contains(&key) && !val.is_empty() {
                crate::cli::wizard::mask_secret(&val)
            } else {
                val.clone()
            };
            println!("{}", style(display).cyan());
            Ok(())
        }
        None => anyhow::bail!("Key '{}' not found in config", key),
    }
}

// ── Set ───────────────────────────────────────────────────────────────────────

/// Update a dotted key in a YAML config file.
pub fn set_config_value(path: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    // Validate known keys
    if let Some(def) = KNOWN_KEYS.iter().find(|k| k.key == key) {
        if let Some(valid) = def.valid_values {
            if !valid.contains(&value) {
                anyhow::bail!(
                    "Invalid value '{}' for '{}'. Valid: {}",
                    value,
                    key,
                    valid.join(", ")
                );
            }
        }
    }

    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        anyhow::bail!(
            "Config not found at {}. Run 'contribai init' first.",
            path.display()
        )
    };

    let (updated, found) = replace_yaml_value(&content, key, value);

    if !found {
        anyhow::bail!(
            "Key '{}' not found in config. Add it manually or check 'contribai config list'.",
            key
        );
    }

    std::fs::write(path, &updated)?;

    let display = if SECRET_KEYS.contains(&key) && !value.is_empty() {
        crate::cli::wizard::mask_secret(value)
    } else {
        value.to_string()
    };
    println!(
        "  {} {} = {}",
        style("✅").green(),
        style(key).bold(),
        style(display).cyan()
    );
    Ok(())
}

// ── List ──────────────────────────────────────────────────────────────────────

/// Show all known config keys with current values from file.
pub fn list_config(path: &Path) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path).unwrap_or_default();

    println!(
        "\n  {:<35} {:<20} {}",
        style("Key").bold(),
        style("Value").bold(),
        style("Description").bold()
    );
    println!("  {}", "─".repeat(85));

    for def in KNOWN_KEYS {
        let val = extract_yaml_value(&content, def.key).unwrap_or_default();
        let display = if SECRET_KEYS.contains(&def.key) && !val.is_empty() {
            crate::cli::wizard::mask_secret(&val)
        } else {
            val.clone()
        };
        let val_styled = if display.is_empty() {
            style("(not set)".to_string()).dim().to_string()
        } else {
            style(display).cyan().to_string()
        };
        println!(
            "  {:<35} {:<30} {}",
            def.key,
            val_styled,
            style(def.description).dim()
        );
    }
    println!();
    Ok(())
}

// ── YAML helpers ──────────────────────────────────────────────────────────────

/// Extract a value by dotted key from YAML content (simple line-based parser).
/// Handles: `llm.provider` → looks for `provider:` under `llm:` section.
pub fn extract_yaml_value(yaml: &str, dotted_key: &str) -> Option<String> {
    let parts: Vec<&str> = dotted_key.splitn(2, '.').collect();
    if parts.len() == 1 {
        // Top-level key
        return find_inline_value(yaml, parts[0], 0);
    }

    let section = parts[0];
    let subkey = parts[1];

    // Find section start
    let mut in_section = false;
    let section_marker = format!("{}:", section);

    for line in yaml.lines() {
        let indent = leading_spaces(line);
        let trimmed = line.trim();

        if !in_section {
            if trimmed == section_marker {
                in_section = true;
            }
            continue;
        }

        // We're in the section — look for the subkey
        if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // New top-level section — exit
            break;
        }

        if indent > 0 || trimmed.starts_with('#') {
            let subkey_prefix = format!("{}:", subkey);
            if trimmed.starts_with(&subkey_prefix) {
                let val = trimmed[subkey_prefix.len()..].trim();
                return Some(strip_quotes(val).to_string());
            }
        }
    }

    None
}

/// Replace a value by dotted key in YAML content.
/// Returns (new_content, was_found).
pub fn replace_yaml_value(yaml: &str, dotted_key: &str, new_value: &str) -> (String, bool) {
    let parts: Vec<&str> = dotted_key.splitn(2, '.').collect();
    let section = if parts.len() > 1 {
        Some(parts[0])
    } else {
        None
    };
    let subkey = *parts.last().unwrap();
    let subkey_prefix = format!("{}:", subkey);

    let mut in_section = section.is_none(); // if no section, already "in" it
    let mut found = false;
    let mut result_lines = Vec::new();

    for line in yaml.lines() {
        let indent = leading_spaces(line);
        let trimmed = line.trim();

        // Track section entry/exit
        if let Some(sec) = section {
            if !in_section {
                if trimmed == format!("{}:", sec) {
                    in_section = true;
                }
                result_lines.push(line.to_string());
                continue;
            } else if indent == 0
                && !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && trimmed != format!("{}:", sec)
            {
                in_section = false;
                result_lines.push(line.to_string());
                continue;
            }
        }

        if in_section && trimmed.starts_with(&subkey_prefix) && !found {
            // Determine indentation from original line
            let spaces = " ".repeat(indent);
            // Format value: quote strings, leave numbers/booleans bare
            let formatted = format_yaml_value(new_value);
            result_lines.push(format!("{}{}: {}", spaces, subkey, formatted));
            found = true;
        } else {
            result_lines.push(line.to_string());
        }
    }

    (result_lines.join("\n") + "\n", found)
}

fn format_yaml_value(v: &str) -> String {
    // Booleans and numbers: no quotes
    if v == "true" || v == "false" || v == "null" {
        return v.to_string();
    }
    if v.parse::<f64>().is_ok() {
        return v.to_string();
    }
    // YAML flow sequences/maps: pass through as-is (e.g. [100, 50000])
    if (v.starts_with('[') && v.ends_with(']')) || (v.starts_with('{') && v.ends_with('}')) {
        return v.to_string();
    }
    // Strings: wrap in quotes
    format!("\"{}\"", v)
}

fn find_inline_value(yaml: &str, key: &str, indent: usize) -> Option<String> {
    let prefix = format!("{}:", key);
    for line in yaml.lines() {
        let trimmed = line.trim();
        if leading_spaces(line) == indent && trimmed.starts_with(&prefix) {
            let val = trimmed[prefix.len()..].trim();
            return Some(strip_quotes(val).to_string());
        }
    }
    None
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Resolve the config file path (same logic as ContribAIConfig::load).
pub fn resolve_config_path(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    let candidates = [
        PathBuf::from("config.yaml"),
        PathBuf::from("config.yml"),
        dirs::home_dir()
            .unwrap_or_default()
            .join(".contribai")
            .join("config.yaml"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    PathBuf::from("config.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"github:
  token: ""
  max_prs_per_day: 15

llm:
  provider: "gemini"
  model: "gemini-3-flash-preview"
  api_key: "sk-test"
  vertex_project: ""

web:
  port: 8787
  enabled: true
"#;

    #[test]
    fn test_extract_nested_value() {
        assert_eq!(
            extract_yaml_value(SAMPLE_YAML, "llm.provider"),
            Some("gemini".to_string())
        );
        assert_eq!(
            extract_yaml_value(SAMPLE_YAML, "llm.api_key"),
            Some("sk-test".to_string())
        );
        assert_eq!(
            extract_yaml_value(SAMPLE_YAML, "github.max_prs_per_day"),
            Some("15".to_string())
        );
        assert_eq!(
            extract_yaml_value(SAMPLE_YAML, "web.port"),
            Some("8787".to_string())
        );
    }

    #[test]
    fn test_replace_string_value() {
        let (updated, found) = replace_yaml_value(SAMPLE_YAML, "llm.provider", "openai");
        assert!(found);
        assert!(updated.contains("provider: \"openai\""));
        assert!(!updated.contains("provider: \"gemini\""));
    }

    #[test]
    fn test_replace_numeric_value() {
        let (updated, found) = replace_yaml_value(SAMPLE_YAML, "github.max_prs_per_day", "25");
        assert!(found);
        assert!(updated.contains("max_prs_per_day: 25"));
    }

    #[test]
    fn test_replace_bool_value() {
        let (updated, found) = replace_yaml_value(SAMPLE_YAML, "web.enabled", "false");
        assert!(found);
        assert!(updated.contains("enabled: false"));
    }

    #[test]
    fn test_replace_not_found() {
        let (_, found) = replace_yaml_value(SAMPLE_YAML, "llm.nonexistent", "val");
        assert!(!found);
    }

    #[test]
    fn test_known_keys_validate() {
        // provider must be in the allowed list
        let valid = KNOWN_KEYS.iter().find(|k| k.key == "llm.provider").unwrap();
        let allowed = valid.valid_values.unwrap();
        assert!(allowed.contains(&"gemini"));
        assert!(allowed.contains(&"vertex"));
        assert!(!allowed.contains(&"badprovider"));
    }

    #[test]
    fn test_format_yaml_value() {
        assert_eq!(format_yaml_value("true"), "true");
        assert_eq!(format_yaml_value("42"), "42");
        assert_eq!(format_yaml_value("gemini"), "\"gemini\"");
        // YAML lists/maps should NOT be quoted
        assert_eq!(format_yaml_value("[100, 50000]"), "[100, 50000]");
        assert_eq!(format_yaml_value("[50, 5000]"), "[50, 5000]");
        assert_eq!(format_yaml_value("{key: val}"), "{key: val}");
    }
}
