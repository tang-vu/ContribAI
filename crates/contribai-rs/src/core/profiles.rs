//! Named contribution profiles for quick configuration.
//!
//! Port from Python `core/profiles.py`.

use serde::{Deserialize, Serialize};
use tracing::info;

/// A named contribution profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContribProfile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub analyzers: Vec<String>,
    #[serde(default)]
    pub contribution_types: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity_threshold: String,
    #[serde(default = "default_max_prs")]
    pub max_prs_per_day: i32,
    #[serde(default = "default_max_repos")]
    pub max_repos_per_run: i32,
    #[serde(default)]
    pub dry_run: bool,
}

fn default_severity() -> String { "medium".into() }
fn default_max_prs() -> i32 { 10 }
fn default_max_repos() -> i32 { 5 }

/// Built-in profiles.
pub fn builtin_profiles() -> Vec<ContribProfile> {
    vec![
        ContribProfile {
            name: "security-focused".into(),
            description: "Focus on security vulnerabilities and fixes".into(),
            analyzers: vec!["security".into()],
            contribution_types: vec!["security_fix".into(), "code_quality".into()],
            severity_threshold: "high".into(),
            max_prs_per_day: 5,
            max_repos_per_run: 5,
            dry_run: false,
        },
        ContribProfile {
            name: "docs-focused".into(),
            description: "Focus on documentation improvements".into(),
            analyzers: vec!["docs".into()],
            contribution_types: vec!["docs_improve".into()],
            severity_threshold: "low".into(),
            max_prs_per_day: 10,
            max_repos_per_run: 5,
            dry_run: false,
        },
        ContribProfile {
            name: "full-scan".into(),
            description: "Run all analyzers with low threshold".into(),
            analyzers: vec!["security".into(), "code_quality".into(), "docs".into(), "ui_ux".into()],
            contribution_types: vec![
                "security_fix".into(), "docs_improve".into(), "code_quality".into(),
                "feature_add".into(), "ui_ux_fix".into(), "performance_opt".into(),
                "refactor".into(),
            ],
            severity_threshold: "low".into(),
            max_prs_per_day: 10,
            max_repos_per_run: 10,
            dry_run: false,
        },
        ContribProfile {
            name: "gentle".into(),
            description: "Low-impact mode: small fixes, dry run by default".into(),
            analyzers: vec!["docs".into(), "code_quality".into()],
            contribution_types: vec!["docs_improve".into(), "code_quality".into()],
            severity_threshold: "high".into(),
            max_prs_per_day: 3,
            max_repos_per_run: 2,
            dry_run: true,
        },
    ]
}

/// Get a profile by name.
pub fn get_profile(name: &str) -> Option<ContribProfile> {
    builtin_profiles().into_iter().find(|p| p.name == name)
}

/// List all available profiles.
pub fn list_profiles() -> Vec<ContribProfile> {
    let profiles = builtin_profiles();
    info!(count = profiles.len(), "Available profiles");
    profiles
}

/// Load a profile from YAML string.
pub fn load_profile_yaml(yaml: &str) -> Option<ContribProfile> {
    serde_yaml::from_str(yaml).ok()
}

/// Apply a profile's settings to a mutable config map.
///
/// Port of Python `apply_profile()`. Merges profile values into a
/// serde_json::Value config (typically loaded from TOML/JSON).
pub fn apply_profile(config: &mut serde_json::Value, profile: &ContribProfile) {
    // Ensure config is an object before taking a mutable reference to its map.
    if !config.is_object() {
        *config = serde_json::json!({});
    }
    let obj = config.as_object_mut().expect("config must be an object");

    if !profile.analyzers.is_empty() {
        let analysis = obj
            .entry("analysis")
            .or_insert_with(|| serde_json::json!({}));
        analysis["enabled_analyzers"] = serde_json::json!(profile.analyzers);
    }
    if !profile.contribution_types.is_empty() {
        let contribution = obj
            .entry("contribution")
            .or_insert_with(|| serde_json::json!({}));
        contribution["enabled_types"] = serde_json::json!(profile.contribution_types);
    }

    let analysis = obj
        .entry("analysis")
        .or_insert_with(|| serde_json::json!({}));
    analysis["severity_threshold"] = serde_json::json!(profile.severity_threshold);

    let github = obj
        .entry("github")
        .or_insert_with(|| serde_json::json!({}));
    github["max_prs_per_day"] = serde_json::json!(profile.max_prs_per_day);
    github["max_repos_per_run"] = serde_json::json!(profile.max_repos_per_run);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles() {
        let profiles = builtin_profiles();
        assert_eq!(profiles.len(), 4);
    }

    #[test]
    fn test_get_profile() {
        let p = get_profile("security-focused").unwrap();
        assert_eq!(p.severity_threshold, "high");
        assert!(p.analyzers.contains(&"security".to_string()));
    }

    #[test]
    fn test_get_profile_missing() {
        assert!(get_profile("nonexistent").is_none());
    }

    #[test]
    fn test_gentle_dry_run() {
        let p = get_profile("gentle").unwrap();
        assert!(p.dry_run);
        assert_eq!(p.max_prs_per_day, 3);
    }

    #[test]
    fn test_apply_profile() {
        let profile = get_profile("security-focused").unwrap();
        let mut config = serde_json::json!({});
        apply_profile(&mut config, &profile);
        assert_eq!(
            config["analysis"]["severity_threshold"],
            serde_json::json!("high")
        );
        assert_eq!(
            config["github"]["max_prs_per_day"],
            serde_json::json!(5)
        );
        assert!(config["analysis"]["enabled_analyzers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("security")));
    }

    #[test]
    fn test_load_yaml() {
        let yaml = r#"
name: custom
description: Custom profile
analyzers: [security]
severity_threshold: high
max_prs_per_day: 2
"#;
        let p = load_profile_yaml(yaml).unwrap();
        assert_eq!(p.name, "custom");
        assert_eq!(p.max_prs_per_day, 2);
    }
}
