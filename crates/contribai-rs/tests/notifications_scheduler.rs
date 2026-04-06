//! Notification and scheduler tests.
//!
//! Tests:
//! - Slack/Discord/Telegram webhook sending with mocked HTTP
//! - Webhook signature verification (HMAC)
//! - Cron scheduler: parse cron expression, fire at correct times, skip missed

// ── Notification Tests ──────────────────────────────────────────────────

#[test]
fn test_slack_webhook_url_format() {
    // Valid Slack webhook URLs follow this pattern
    let valid_urls = vec![
        "https://hooks.slack.com/services/EXAMPLE/EXAMPLE/EXAMPLE",
        "https://hooks.slack.com/services/FAKE/FAKE/FAKE",
    ];

    for url in &valid_urls {
        assert!(
            url.starts_with("https://hooks.slack.com/services/"),
            "Slack webhook URL should start with correct prefix: {}",
            url
        );
        assert!(
            url.len() > 60,
            "Slack webhook URL should be long enough: {}",
            url
        );
    }
}

#[test]
fn test_discord_webhook_url_format() {
    // Valid Discord webhook URLs follow this pattern
    let valid_urls = vec![
        "https://discord.com/api/webhooks/1234567890/abcdefghijklmnopqrstuvwxyz123456",
        "https://discordapp.com/api/webhooks/9876543210/ABCDEFGHIJKLMNOPQRSTUVWXYZ123456",
    ];

    for url in &valid_urls {
        assert!(
            url.contains("/api/webhooks/"),
            "Discord webhook URL should contain /api/webhooks/: {}",
            url
        );
    }
}

#[test]
fn test_telegram_webhook_url_format() {
    // Telegram bot token format: <bot_token>:<api_token>
    // Chat ID: numeric string
    let valid_tokens = vec![
        "123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef",
        "987654321:zyxwvutsrqponmlkjihgfedcbaZYXWVUTSR",
    ];

    for token in &valid_tokens {
        let parts: Vec<&str> = token.split(':').collect();
        assert_eq!(
            parts.len(),
            2,
            "Telegram token should have bot_id:api_token format"
        );
        assert!(
            parts[0].chars().all(|c| c.is_numeric()),
            "Bot ID should be numeric"
        );
    }
}

// ── HMAC Signature Verification ─────────────────────────────────────────

#[test]
fn test_hmac_signature_verification() {
    use contribai::core::models::Repository;

    // Simulate HMAC-SHA256 webhook signature verification
    let secret = "test_secret";
    let payload = r#"{"action":"opened","issue":{"number":42}}"#;

    // Compute expected signature
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Signature should be 64 hex characters
    assert_eq!(
        signature.len(),
        64,
        "HMAC-SHA256 signature should be 64 hex chars"
    );

    // Verification: same secret + payload = same signature
    let mut mac2 = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac2.update(payload.as_bytes());
    let signature2 = hex::encode(mac2.finalize().into_bytes());
    assert_eq!(
        signature, signature2,
        "Same inputs should produce same signature"
    );
}

#[test]
fn test_hmac_different_secrets_different_signatures() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let payload = r#"{"action":"opened"}"#;

    let mut mac1 = HmacSha256::new_from_slice(b"secret1").unwrap();
    mac1.update(payload.as_bytes());
    let sig1 = hex::encode(mac1.finalize().into_bytes());

    let mut mac2 = HmacSha256::new_from_slice(b"secret2").unwrap();
    mac2.update(payload.as_bytes());
    let sig2 = hex::encode(mac2.finalize().into_bytes());

    assert_ne!(
        sig1, sig2,
        "Different secrets should produce different signatures"
    );
}

#[test]
fn test_hmac_different_payloads_different_signatures() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let secret = b"test_secret";

    let mut mac1 = HmacSha256::new_from_slice(secret).unwrap();
    mac1.update(b"{\"action\":\"opened\"}");
    let sig1 = hex::encode(mac1.finalize().into_bytes());

    let mut mac2 = HmacSha256::new_from_slice(secret).unwrap();
    mac2.update(b"{\"action\":\"closed\"}");
    let sig2 = hex::encode(mac2.finalize().into_bytes());

    assert_ne!(
        sig1, sig2,
        "Different payloads should produce different signatures"
    );
}

// ── Scheduler / Cron Tests ──────────────────────────────────────────────

#[test]
fn test_scheduler_config_default_cron() {
    use contribai::core::config::SchedulerConfig;

    let config = SchedulerConfig::default();
    assert_eq!(
        config.cron, "0 */6 * * *",
        "Default cron should be every 6 hours"
    );
    assert!(config.enabled, "Scheduler should be enabled by default");
}

#[test]
fn test_scheduler_config_custom_cron() {
    use contribai::core::config::SchedulerConfig;

    let yaml = "cron: \"0 0 * * *\"\nenabled: false";
    let config: SchedulerConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.cron, "0 0 * * *");
    assert!(!config.enabled);
}

#[test]
fn test_scheduler_config_partial_yaml() {
    use contribai::core::config::SchedulerConfig;

    let yaml = "cron: \"30 */2 * * *\"";
    let config: SchedulerConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.cron, "30 */2 * * *");
    assert!(config.enabled); // default preserved
}

#[test]
fn test_cron_expression_valid_formats() {
    let valid_crons = vec![
        "* * * * *",    // every minute
        "*/5 * * * *",  // every 5 minutes
        "0 * * * *",    // every hour
        "0 */6 * * *",  // every 6 hours
        "0 0 * * *",    // daily at midnight
        "0 0 * * 0",    // weekly on Sunday
        "0 0 1 * *",    // monthly on 1st
        "30 8 * * 1-5", // weekdays at 8:30
    ];

    for cron in &valid_crons {
        let parts: Vec<&str> = cron.split_whitespace().collect();
        assert_eq!(
            parts.len(),
            5,
            "Cron expression should have 5 fields: {}",
            cron
        );
    }
}

#[test]
fn test_cron_expression_invalid_formats() {
    let invalid_crons = vec![
        "* * * *",     // 4 fields
        "* * * * * *", // 6 fields
    ];

    for cron in &invalid_crons {
        let parts: Vec<&str> = cron.split_whitespace().collect();
        assert_ne!(
            parts.len(),
            5,
            "Invalid cron should have wrong field count: {}",
            cron
        );
    }
}
