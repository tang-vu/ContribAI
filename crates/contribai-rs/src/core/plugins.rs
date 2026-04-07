//! Plugin system for ContribAI.
//!
//! Plugins can hook into pipeline lifecycle events:
//! - `on_analysis_complete` — after code analysis finishes
//! - `on_pr_created` — after a PR is submitted
//! - `on_pr_merged` — after a PR is merged
//! - `on_pr_closed` — after a PR is closed (not merged)
//! - `on_error` — when a pipeline error occurs
//!
//! Plugins are defined in config:
//! ```yaml
//! plugins:
//!   - name: "slack-notifier"
//!     hooks: ["on_pr_created", "on_error"]
//!   - name: "custom-audit"
//!     hooks: ["on_analysis_complete"]
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Plugin lifecycle hooks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PluginHook {
    OnAnalysisComplete,
    OnPrCreated,
    OnPrMerged,
    OnPrClosed,
    OnError,
}

/// Plugin definition from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSpec {
    /// Unique plugin name.
    pub name: String,
    /// Hooks this plugin subscribes to.
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
    /// Plugin-specific configuration.
    #[serde(default)]
    pub config: HashMap<String, String>,
}

/// Plugin manager — stores and dispatches to plugins.
pub struct PluginManager {
    plugins: Vec<PluginSpec>,
}

impl PluginManager {
    pub fn new(plugins: Vec<PluginSpec>) -> Self {
        info!(plugins = plugins.len(), "Plugin manager initialized");
        Self { plugins }
    }

    /// Dispatch an event to all subscribed plugins.
    pub fn dispatch(&self, hook: &PluginHook, payload: &serde_json::Value) {
        for plugin in &self.plugins {
            if plugin.hooks.contains(hook) {
                debug!(
                    plugin = %plugin.name,
                    hook = ?hook,
                    "Plugin hook dispatched"
                );
                // In a full implementation, this would call the plugin's
                // executable or HTTP endpoint with the payload.
                // For now, we just log the dispatch.
                let _ = payload;
            }
        }
    }

    /// Get count of active plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self { plugins: vec![] }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manager_default() {
        let mgr = PluginManager::default();
        assert_eq!(mgr.plugin_count(), 0);
    }

    #[test]
    fn test_plugin_manager_with_plugins() {
        let plugins = vec![
            PluginSpec {
                name: "slack".to_string(),
                hooks: vec![PluginHook::OnPrCreated, PluginHook::OnError],
                config: HashMap::new(),
            },
            PluginSpec {
                name: "audit".to_string(),
                hooks: vec![PluginHook::OnAnalysisComplete],
                config: HashMap::new(),
            },
        ];
        let mgr = PluginManager::new(plugins);
        assert_eq!(mgr.plugin_count(), 2);
    }

    #[test]
    fn test_plugin_hook_dispatch() {
        let plugins = vec![PluginSpec {
            name: "test-plugin".to_string(),
            hooks: vec![PluginHook::OnPrCreated],
            config: HashMap::new(),
        }];
        let mgr = PluginManager::new(plugins);

        // Should dispatch to subscribed plugin
        mgr.dispatch(&PluginHook::OnPrCreated, &serde_json::json!({}));

        // Should not dispatch to non-subscribed hook
        mgr.dispatch(&PluginHook::OnError, &serde_json::json!({}));
    }

    #[test]
    fn test_plugin_config_deser() {
        let yaml = r##"
- name: "slack-notifier"
  hooks: [on_pr_created, on_error]
  config:
    webhook_url: "https://hooks.slack.com/test"
    channel: "#devops"
"##;
        let plugins: Vec<PluginSpec> = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "slack-notifier");
        assert_eq!(plugins[0].hooks.len(), 2);
        assert!(plugins[0].config.contains_key("webhook_url"));
        assert!(plugins[0].config.contains_key("channel"));
    }
}
