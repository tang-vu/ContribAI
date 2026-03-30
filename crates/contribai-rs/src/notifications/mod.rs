//! Notification system for Slack, Discord, and Telegram.
//!
//! Port from Python `notifications/notifier.py`.
//! Sends webhooks when PRs are merged/closed or pipeline runs complete.

use reqwest::Client;
use serde_json::json;
use std::collections::HashMap;
use tracing::{debug, warn};

use crate::core::error::{ContribError, Result};

/// An event to notify about.
#[derive(Debug, Clone)]
pub struct NotificationEvent {
    pub event_type: String,
    pub title: String,
    pub message: String,
    pub url: String,
    pub repo: String,
    pub extra: HashMap<String, String>,
}

impl NotificationEvent {
    pub fn new(event_type: &str, title: &str, message: &str) -> Self {
        Self {
            event_type: event_type.into(),
            title: title.into(),
            message: message.into(),
            url: String::new(),
            repo: String::new(),
            extra: HashMap::new(),
        }
    }

    pub fn with_url(mut self, url: &str) -> Self {
        self.url = url.into();
        self
    }

    pub fn with_repo(mut self, repo: &str) -> Self {
        self.repo = repo.into();
        self
    }
}

/// Multi-channel notification dispatcher.
pub struct Notifier {
    slack_webhook: String,
    discord_webhook: String,
    telegram_token: String,
    telegram_chat_id: String,
    client: Client,
}

impl Notifier {
    pub fn new(
        slack_webhook: &str,
        discord_webhook: &str,
        telegram_token: &str,
        telegram_chat_id: &str,
    ) -> Self {
        Self {
            slack_webhook: slack_webhook.into(),
            discord_webhook: discord_webhook.into(),
            telegram_token: telegram_token.into(),
            telegram_chat_id: telegram_chat_id.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Check if any notification channel is configured.
    pub fn is_configured(&self) -> bool {
        !self.slack_webhook.is_empty()
            || !self.discord_webhook.is_empty()
            || (!self.telegram_token.is_empty() && !self.telegram_chat_id.is_empty())
    }

    /// Send notification to all configured channels.
    pub async fn notify(&self, event: &NotificationEvent) {
        if !self.is_configured() {
            return;
        }

        if !self.slack_webhook.is_empty() {
            self.send_slack_with_retry(event).await;
        }
        if !self.discord_webhook.is_empty() {
            self.send_discord_with_retry(event).await;
        }
        if !self.telegram_token.is_empty() && !self.telegram_chat_id.is_empty() {
            self.send_telegram_with_retry(event).await;
        }
    }

    async fn send_slack_with_retry(&self, event: &NotificationEvent) {
        for attempt in 0..=2u32 {
            match self.send_slack(event).await {
                Ok(()) => return,
                Err(e) if attempt == 2 => warn!(channel = "Slack", error = %e, "Notification failed"),
                Err(_) => {
                    let delay = 1u64 << attempt;
                    debug!(channel = "Slack", attempt = attempt + 1, "Retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
            }
        }
    }

    async fn send_discord_with_retry(&self, event: &NotificationEvent) {
        for attempt in 0..=2u32 {
            match self.send_discord(event).await {
                Ok(()) => return,
                Err(e) if attempt == 2 => warn!(channel = "Discord", error = %e, "Notification failed"),
                Err(_) => {
                    let delay = 1u64 << attempt;
                    debug!(channel = "Discord", attempt = attempt + 1, "Retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
            }
        }
    }

    async fn send_telegram_with_retry(&self, event: &NotificationEvent) {
        for attempt in 0..=2u32 {
            match self.send_telegram(event).await {
                Ok(()) => return,
                Err(e) if attempt == 2 => warn!(channel = "Telegram", error = %e, "Notification failed"),
                Err(_) => {
                    let delay = 1u64 << attempt;
                    debug!(channel = "Telegram", attempt = attempt + 1, "Retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
            }
        }
    }

    // ── Slack ─────────────────────────────────────────

    async fn send_slack(&self, event: &NotificationEvent) -> Result<()> {
        let emoji = get_emoji(&event.event_type);
        let mut blocks = vec![json!({
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": format!("{} *{}*\n{}", emoji, event.title, event.message)
            }
        })];

        if !event.url.is_empty() {
            blocks.push(json!({
                "type": "actions",
                "elements": [{
                    "type": "button",
                    "text": { "type": "plain_text", "text": "View on GitHub" },
                    "url": event.url
                }]
            }));
        }

        let payload = json!({
            "text": format!("{} *{}*\n{}", emoji, event.title, event.message),
            "blocks": blocks
        });

        let resp = self
            .client
            .post(&self.slack_webhook)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ContribError::GitHub(format!("Slack webhook failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ContribError::GitHub(format!(
                "Slack webhook returned {}",
                resp.status()
            )));
        }
        Ok(())
    }

    // ── Discord ───────────────────────────────────────

    async fn send_discord(&self, event: &NotificationEvent) -> Result<()> {
        let emoji = get_emoji(&event.event_type);
        let color = get_color(&event.event_type);

        let payload = json!({
            "embeds": [{
                "title": format!("{} {}", emoji, event.title),
                "description": event.message,
                "color": color,
                "url": if event.url.is_empty() { None } else { Some(&event.url) },
                "footer": { "text": "ContribAI" }
            }]
        });

        let resp = self
            .client
            .post(&self.discord_webhook)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ContribError::GitHub(format!("Discord webhook failed: {e}")))?;

        let status = resp.status().as_u16();
        if status != 200 && status != 204 {
            return Err(ContribError::GitHub(format!(
                "Discord webhook returned {}",
                status
            )));
        }
        Ok(())
    }

    // ── Telegram ──────────────────────────────────────

    async fn send_telegram(&self, event: &NotificationEvent) -> Result<()> {
        let emoji = get_emoji(&event.event_type);
        let mut text = format!("{} <b>{}</b>\n{}", emoji, event.title, event.message);
        if !event.url.is_empty() {
            text.push_str(&format!("\n<a href=\"{}\">View on GitHub</a>", event.url));
        }

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.telegram_token
        );
        let payload = json!({
            "chat_id": self.telegram_chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true
        });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ContribError::GitHub(format!("Telegram failed: {e}")))?;

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ContribError::GitHub(format!("Telegram parse error: {e}")))?;

        if !data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(ContribError::GitHub(format!(
                "Telegram failed: {}",
                data.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            )));
        }
        Ok(())
    }

    // ── Convenience methods ──────────────────────────

    pub async fn notify_pr_merged(&self, repo: &str, pr_number: i64, title: &str, pr_url: &str) {
        self.notify(
            &NotificationEvent::new(
                "pr_merged",
                &format!("PR Merged: {}#{}", repo, pr_number),
                title,
            )
            .with_url(pr_url)
            .with_repo(repo),
        )
        .await;
    }

    pub async fn notify_pr_closed(&self, repo: &str, pr_number: i64, title: &str, pr_url: &str) {
        self.notify(
            &NotificationEvent::new(
                "pr_closed",
                &format!("PR Closed: {}#{}", repo, pr_number),
                title,
            )
            .with_url(pr_url)
            .with_repo(repo),
        )
        .await;
    }

    pub async fn notify_run_complete(
        &self,
        repos_analyzed: usize,
        prs_created: usize,
        errors: usize,
    ) {
        self.notify(&NotificationEvent::new(
            "run_complete",
            "Pipeline Run Complete",
            &format!(
                "Repos: {} | PRs: {} | Errors: {}",
                repos_analyzed, prs_created, errors
            ),
        ))
        .await;
    }
}

fn get_emoji(event_type: &str) -> &'static str {
    match event_type {
        "pr_merged" => "🎉",
        "pr_closed" => "❌",
        "run_complete" => "✅",
        "error" => "🚨",
        _ => "📢",
    }
}

fn get_color(event_type: &str) -> u32 {
    match event_type {
        "pr_merged" => 0x22C55E,
        "pr_closed" => 0xEF4444,
        "run_complete" => 0x38BDF8,
        "error" => 0xF59E0B,
        _ => 0x94A3B8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_event_builder() {
        let event = NotificationEvent::new("pr_merged", "Test Title", "Test message")
            .with_url("https://github.com/test/pr/1")
            .with_repo("test/repo");
        assert_eq!(event.event_type, "pr_merged");
        assert_eq!(event.url, "https://github.com/test/pr/1");
        assert_eq!(event.repo, "test/repo");
    }

    #[test]
    fn test_notifier_not_configured() {
        let n = Notifier::new("", "", "", "");
        assert!(!n.is_configured());
    }

    #[test]
    fn test_notifier_slack_configured() {
        let n = Notifier::new("https://hooks.slack.com/test", "", "", "");
        assert!(n.is_configured());
    }

    #[test]
    fn test_notifier_discord_configured() {
        let n = Notifier::new("", "https://discord.com/api/webhooks/test", "", "");
        assert!(n.is_configured());
    }

    #[test]
    fn test_notifier_telegram_needs_both() {
        let n = Notifier::new("", "", "token", "");
        assert!(!n.is_configured());
        let n2 = Notifier::new("", "", "token", "12345");
        assert!(n2.is_configured());
    }

    #[test]
    fn test_emoji_mapping() {
        assert_eq!(get_emoji("pr_merged"), "🎉");
        assert_eq!(get_emoji("error"), "🚨");
        assert_eq!(get_emoji("unknown"), "📢");
    }

    #[test]
    fn test_color_mapping() {
        assert_eq!(get_color("pr_merged"), 0x22C55E);
        assert_eq!(get_color("pr_closed"), 0xEF4444);
    }

    #[tokio::test]
    async fn test_notify_not_configured_noop() {
        let n = Notifier::new("", "", "", "");
        n.notify(&NotificationEvent::new("test", "title", "msg"))
            .await;
        // Should not panic
    }
}
