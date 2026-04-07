//! GitHub Copilot LLM provider.
//!
//! Exchanges a GitHub token for a Copilot token, then uses the Copilot token
//! to call the OpenAI-compatible API at api.githubcopilot.com.
//!
//! Flow:
//! 1. Get GitHub token (from gh CLI or GITHUB_TOKEN env)
//! 2. Exchange for Copilot token via https://api.github.com/copilot_internal/v2/token
//! 3. Use Copilot token to call OpenAI-compatible endpoint at https://api.githubcopilot.com
//! 4. Auto-refresh token before expiry (5-minute TTL)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, header};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::core::config::LlmConfig;
use crate::core::error::{ContribError, Result};
use crate::llm::provider::{ChatMessage, LlmProvider};

/// Cached Copilot token with expiry.
struct CopilotTokenCache {
    token: String,
    expires_at: DateTime<Utc>,
}

impl CopilotTokenCache {
    fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at - chrono::Duration::seconds(30) // 30s buffer
    }
}

/// GitHub Copilot LLM provider.
pub struct CopilotProvider {
    client: Client,
    model: String,
    temperature: f64,
    max_tokens: u32,
    base_url: String,
    /// Cached Copilot token (auto-refreshed).
    token_cache: Arc<Mutex<Option<CopilotTokenCache>>>,
}

impl CopilotProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.githubcopilot.com".to_string());

        info!(model = %config.model, base_url = %base_url, "Copilot provider initialized");

        Ok(Self {
            client: Client::new(),
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            base_url,
            token_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Get a Copilot token, using cache if still valid.
    async fn get_token(&self) -> Result<String> {
        let mut cache = self.token_cache.lock().await;
        if let Some(ref cached) = *cache {
            if cached.is_valid() {
                return Ok(cached.token.clone());
            }
        }

        // Token expired or missing — exchange GitHub token for Copilot token
        let github_token = Self::get_github_token()?;
        let token_response = self
            .client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header(header::AUTHORIZATION, format!("token {}", github_token))
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Copilot token exchange HTTP error: {}", e)))?;

        let status = token_response.status();
        let body = token_response
            .text()
            .await
            .map_err(|e| ContribError::Llm(format!("Copilot token exchange read error: {}", e)))?;

        if !status.is_success() {
            let error_msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(ContribError::Llm(format!(
                "Copilot token exchange failed {}: {}",
                status, error_msg
            )));
        }

        let data: Value = serde_json::from_str(&body).map_err(|e| {
            ContribError::Llm(format!("Copilot token response parse error: {}", e))
        })?;

        let token = data["token"].as_str().ok_or_else(|| {
            ContribError::Llm("Copilot token response missing 'token' field".into())
        })?;

        // Token expires in 5 minutes per GitHub API
        let expires_at = Utc::now() + chrono::Duration::minutes(5);

        *cache = Some(CopilotTokenCache {
            token: token.to_string(),
            expires_at,
        });

        info!("Copilot token refreshed (expires in 5 min)");
        Ok(token.to_string())
    }

    /// Get GitHub token from env or gh CLI.
    fn get_github_token() -> Result<String> {
        // Try env var first
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                return Ok(token);
            }
        }

        // Try gh CLI
        let output = std::process::Command::new("gh")
            .args(["auth", "token"])
            .output()
            .map_err(|e| {
                ContribError::Llm(format!("gh CLI not found: {}. Run `gh auth login` first.", e))
            })?;

        if !output.status.success() {
            return Err(ContribError::Llm(
                "gh auth token failed. Run `gh auth login` first.".into(),
            ));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(ContribError::Llm(
                "GitHub token is empty. Run `gh auth login` or set GITHUB_TOKEN.".into(),
            ));
        }

        Ok(token)
    }

    /// Call the OpenAI-compatible completions endpoint.
    async fn call_completions(
        &self,
        messages: Vec<Value>,
        temperature: f64,
        max_tokens: u32,
    ) -> Result<String> {
        let token = self.get_token().await?;

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&json!({
                "model": self.model,
                "messages": messages,
                "temperature": temperature,
                "max_tokens": max_tokens,
            }))
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Copilot API HTTP error: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ContribError::Llm(format!("Copilot API response read error: {}", e)))?;

        if !status.is_success() {
            let error_msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown error".to_string());

            if status.as_u16() == 429 {
                return Err(ContribError::Llm(format!(
                    "Copilot rate limit: {}",
                    error_msg
                )));
            }

            return Err(ContribError::Llm(format!(
                "Copilot API error {}: {}",
                status, error_msg
            )));
        }

        let data: Value = serde_json::from_str(&body).map_err(|e| {
            ContribError::Llm(format!("Copilot API response parse error: {}", e))
        })?;

        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");

        Ok(text.to_string())
    }
}

#[async_trait]
impl LlmProvider for CopilotProvider {
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(json!({ "role": "system", "content": sys }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));

        self.call_completions(
            messages,
            temperature.unwrap_or(self.temperature),
            max_tokens.unwrap_or(self.max_tokens),
        )
        .await
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut msgs: Vec<Value> = Vec::new();
        if let Some(sys) = system {
            if !messages.iter().any(|m| m.role == "system") {
                msgs.push(json!({ "role": "system", "content": sys }));
            }
        }
        for msg in messages {
            msgs.push(json!({ "role": &msg.role, "content": &msg.content }));
        }

        self.call_completions(
            msgs,
            temperature.unwrap_or(self.temperature),
            max_tokens.unwrap_or(self.max_tokens),
        )
        .await
    }
}

/// Check if Copilot auth is available (gh CLI configured + valid token).
pub fn copilot_available() -> bool {
    CopilotProvider::get_github_token().is_ok()
}
