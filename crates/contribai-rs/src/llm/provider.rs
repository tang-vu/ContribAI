//! LLM Provider abstraction — Gemini, OpenAI, Anthropic, Ollama.
//!
//! All providers implement the same async trait. In Rust we use raw HTTP
//! via reqwest instead of Python SDK packages, giving us zero extra dependencies.
//! Port from Python `llm/provider.py`.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::info;

use crate::core::config::LlmConfig;
use crate::core::error::{ContribError, Result};

/// Message in a chat conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn system(content: &str) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// Abstract LLM provider interface.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Single-turn completion.
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String>;

    /// Multi-turn chat completion.
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String>;
}

// ── gcloud token helper ───────────────────────────────────────────────────────

/// Fetch an access token from `gcloud auth print-access-token`.
/// Used for Vertex AI authentication.
///
/// On Windows, gcloud is installed as `gcloud.cmd` (batch file) and cannot be
/// spawned directly as a binary — we must use `cmd /c gcloud` instead.
fn fetch_gcloud_token() -> Result<String> {
    // On Windows, gcloud is a .cmd batch file; spawn via cmd.exe
    #[cfg(target_os = "windows")]
    let out = std::process::Command::new("cmd")
        .args(["/c", "gcloud", "auth", "print-access-token"])
        .output()
        .map_err(|e| ContribError::Llm(format!("gcloud not found: {}", e)))?;

    #[cfg(not(target_os = "windows"))]
    let out = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .map_err(|e| ContribError::Llm(format!("gcloud not found: {}", e)))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(ContribError::Llm(format!(
            "gcloud auth print-access-token failed: {}",
            stderr.trim()
        )));
    }

    let token = String::from_utf8(out.stdout)
        .map_err(|e| ContribError::Llm(format!("gcloud token encoding: {}", e)))?;
    Ok(token.trim().to_string())
}

// ── Gemini Provider (primary) ─────────────────────────────────────────────────

/// Cached Vertex AI access token — avoids calling gcloud per-request.
/// Token expires after 1h; we refresh at 55 minutes.
struct TokenCache {
    token: String,
    fetched_at: std::time::Instant,
}

impl TokenCache {
    fn is_fresh(&self) -> bool {
        self.fetched_at.elapsed() < std::time::Duration::from_secs(55 * 60)
    }
}

/// Google Gemini provider — primary/default.
///
/// Supports both API key auth and Vertex AI (Google Cloud).
/// When `vertex_project` is set in config, uses `gcloud auth print-access-token`
/// and routes to the Vertex AI endpoint (no API key needed).
///
/// v5.2: Token cached for 55 minutes to avoid per-request gcloud calls.
pub struct GeminiProvider {
    client: Client,
    /// Non-empty for API key auth; empty for Vertex AI.
    api_key: String,
    model: String,
    temperature: f64,
    max_tokens: u32,
    /// Non-empty when using Vertex AI.
    vertex_project: String,
    vertex_location: String,
    /// Cached Vertex AI token (55-min TTL). None = not yet fetched or key-auth mode.
    token_cache: std::sync::Arc<std::sync::Mutex<Option<TokenCache>>>,
}

impl GeminiProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        if config.use_vertex() {
            // Vertex AI mode — token fetched per-request from gcloud CLI
            info!(
                model = %config.model,
                project = %config.vertex_project,
                "Gemini via Vertex AI (token cached 55 min)"
            );
            return Ok(Self {
                client: Client::new(),
                api_key: String::new(),
                model: config.model.clone(),
                temperature: config.temperature,
                max_tokens: config.max_tokens,
                vertex_project: config.vertex_project.clone(),
                vertex_location: config.vertex_location.clone(),
                token_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            });
        }

        // API key mode
        let api_key = if !config.api_key.is_empty() {
            config.api_key.clone()
        } else {
            std::env::var("GEMINI_API_KEY")
                .map_err(|_| ContribError::Llm("GEMINI_API_KEY not set".into()))?
        };

        info!(model = %config.model, "Gemini via API key");

        Ok(Self {
            client: Client::new(),
            api_key,
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            vertex_project: String::new(),
            vertex_location: String::new(),
            token_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    /// Get a Vertex AI access token, using cache if still fresh.
    fn get_cached_token(&self) -> Result<String> {
        let mut cache = self.token_cache.lock().unwrap();
        if let Some(ref tc) = *cache {
            if tc.is_fresh() {
                return Ok(tc.token.clone());
            }
        }
        // Cache miss or expired — fetch fresh token
        let token = fetch_gcloud_token()?;
        *cache = Some(TokenCache {
            token: token.clone(),
            fetched_at: std::time::Instant::now(),
        });
        Ok(token)
    }

    /// Build the request URL and auth header.
    /// Returns (url, Option<bearer_token>).
    fn build_endpoint(&self) -> Result<(String, Option<String>)> {
        if !self.vertex_project.is_empty() {
            // Preview models use v1beta1, stable models use v1
            let api_version = if self.model.contains("preview") {
                "v1beta1"
            } else {
                "v1"
            };
            // "global" uses aiplatform.googleapis.com (no region prefix)
            // Regional uses {region}-aiplatform.googleapis.com
            let hostname = if self.vertex_location == "global" {
                "aiplatform.googleapis.com".to_string()
            } else {
                format!("{}-aiplatform.googleapis.com", self.vertex_location)
            };
            let url = format!(
                "https://{}/{}/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                hostname, api_version, self.vertex_project, self.vertex_location, self.model
            );
            let token = self.get_cached_token()?;
            Ok((url, Some(token)))
        } else {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                self.model, self.api_key
            );
            Ok((url, None))
        }
    }

    // ── Context Caching ──────────────────────────────────────────────────────

    /// Create a cached content resource on Gemini.
    ///
    /// Uploads the static `context` as a cached content object with 1-hour TTL.
    /// Returns the cache name (e.g. `cachedContents/abc123`) for subsequent calls.
    /// Returns `None` if caching fails (gracefully degrades to inline context).
    pub async fn create_cached_content(
        &self,
        context: &str,
        system_instruction: Option<&str>,
    ) -> Option<String> {
        let (cache_url, bearer) = if !self.vertex_project.is_empty() {
            let api_version = if self.model.contains("preview") {
                "v1beta1"
            } else {
                "v1"
            };
            let hostname = if self.vertex_location == "global" {
                "aiplatform.googleapis.com".to_string()
            } else {
                format!("{}-aiplatform.googleapis.com", self.vertex_location)
            };
            let url = format!(
                "https://{}/{}/projects/{}/locations/{}/cachedContents",
                hostname, api_version, self.vertex_project, self.vertex_location
            );
            let token = self.get_cached_token().ok()?;
            (url, Some(token))
        } else {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/cachedContents?key={}",
                self.api_key
            );
            (url, None)
        };

        let mut body = json!({
            "model": format!("models/{}", self.model),
            "contents": [{
                "role": "user",
                "parts": [{ "text": context }]
            }],
            "ttl": "3600s"
        });

        if let Some(sys) = system_instruction {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        // For Vertex AI, model format is different
        if !self.vertex_project.is_empty() {
            body["model"] = json!(format!(
                "projects/{}/locations/{}/publishers/google/models/{}",
                self.vertex_project, self.vertex_location, self.model
            ));
        }

        let mut req = self.client.post(&cache_url).json(&body);
        if let Some(ref token) = bearer {
            req = req.bearer_auth(token);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    let data: Value = serde_json::from_str(&text).ok()?;
                    let name = data["name"].as_str()?.to_string();
                    info!(cache = %name, "📦 Context cached (1h TTL)");
                    Some(name)
                } else {
                    tracing::debug!(status = %status, "Context caching unavailable, using inline");
                    None
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "Context caching request failed");
                None
            }
        }
    }

    /// Complete using a cached content reference instead of inline context.
    ///
    /// Falls back to normal `complete()` if cache_name is empty.
    pub async fn complete_with_cache(
        &self,
        cache_name: &str,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        if cache_name.is_empty() {
            return self.complete(prompt, system, temperature, max_tokens).await;
        }

        let temp = temperature.unwrap_or(self.temperature);
        let max_tok = max_tokens.unwrap_or(self.max_tokens);

        let (url, bearer) = self.build_endpoint()?;

        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "temperature": temp,
                "maxOutputTokens": max_tok,
            },
            "cachedContent": cache_name
        });

        if let Some(sys) = system {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        let mut req = self.client.post(&url).json(&body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let response = req
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini HTTP error: {}", e)))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini response read: {}", e)))?;

        let data: Value = serde_json::from_str(&body_text).map_err(|e| {
            let preview = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            ContribError::Llm(format!(
                "Gemini JSON parse: {} — response preview: {}",
                e, preview
            ))
        })?;

        if !status.is_success() {
            let error_msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
            // Cache expired/invalid → fall back to regular completion
            if status.as_u16() == 400 || status.as_u16() == 404 {
                tracing::debug!(cache = cache_name, "Cache invalid, falling back");
                return self.complete(prompt, system, temperature, max_tokens).await;
            }
            return Err(ContribError::Llm(format!(
                "Gemini API error {}: {}",
                status, error_msg
            )));
        }

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .or_else(|| {
                data["candidates"][0]["content"]["parts"]
                    .as_array()
                    .and_then(|parts| parts.iter().filter_map(|p| p["text"].as_str()).next())
            })
            .unwrap_or("");

        Ok(text.to_string())
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let temp = temperature.unwrap_or(self.temperature);
        let max_tok = max_tokens.unwrap_or(self.max_tokens);

        let (url, bearer) = self.build_endpoint()?;

        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "temperature": temp,
                "maxOutputTokens": max_tok,
            }
        });

        if let Some(sys) = system {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        let mut req = self.client.post(&url).json(&body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let response = req
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini HTTP error: {}", e)))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini response read: {}", e)))?;

        let data: Value = serde_json::from_str(&body_text).map_err(|e| {
            // Log first 500 chars of body for debugging
            let preview = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            ContribError::Llm(format!(
                "Gemini JSON parse: {} — response preview: {}",
                e, preview
            ))
        })?;

        if !status.is_success() {
            let error_msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
            if status.as_u16() == 429 {
                return Err(ContribError::Llm(format!(
                    "Gemini rate limit: {}",
                    error_msg
                )));
            }
            return Err(ContribError::Llm(format!(
                "Gemini API error {}: {}",
                status, error_msg
            )));
        }

        // Extract text from response — handle both single-part and multi-part
        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .or_else(|| {
                // Some models return parts as array of text chunks
                data["candidates"][0]["content"]["parts"]
                    .as_array()
                    .and_then(|parts| parts.iter().filter_map(|p| p["text"].as_str()).next())
            })
            .unwrap_or("");

        Ok(text.to_string())
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let temp = temperature.unwrap_or(self.temperature);
        let max_tok = max_tokens.unwrap_or(self.max_tokens);

        let (url, bearer) = self.build_endpoint()?;

        let contents: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let role = if msg.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                json!({
                    "role": role,
                    "parts": [{ "text": &msg.content }]
                })
            })
            .collect();

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "temperature": temp,
                "maxOutputTokens": max_tok,
            }
        });

        if let Some(sys) = system {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        let mut req = self.client.post(&url).json(&body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let response = req
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini HTTP error: {}", e)))?;

        let body_text = response
            .text()
            .await
            .map_err(|e| ContribError::Llm(format!("Gemini response read: {}", e)))?;

        let data: Value = serde_json::from_str(&body_text).map_err(|e| {
            let preview = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            ContribError::Llm(format!(
                "Gemini JSON parse: {} — response preview: {}",
                e, preview
            ))
        })?;

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .or_else(|| {
                data["candidates"][0]["content"]["parts"]
                    .as_array()
                    .and_then(|parts| parts.iter().filter_map(|p| p["text"].as_str()).next())
            })
            .unwrap_or("");

        Ok(text.to_string())
    }
}

// ── OpenAI Provider ───────────────────────────────────────────────────────────

/// OpenAI provider (GPT-4o, etc.) — also works with any OpenAI-compatible endpoint.
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    temperature: f64,
    max_tokens: u32,
}

impl OpenAIProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let api_key = if !config.api_key.is_empty() {
            config.api_key.clone()
        } else {
            std::env::var("OPENAI_API_KEY")
                .map_err(|_| ContribError::Llm("OPENAI_API_KEY not set".into()))?
        };

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        info!(model = %config.model, base_url = %base_url, "OpenAI provider");

        Ok(Self {
            client: Client::new(),
            api_key,
            base_url,
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(ChatMessage::system(sys));
        }
        messages.push(ChatMessage::user(prompt));
        self.chat(&messages, None, temperature, max_tokens).await
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let temp = temperature.unwrap_or(self.temperature);
        let max_tok = max_tokens.unwrap_or(self.max_tokens);

        let mut msgs: Vec<Value> = Vec::new();
        if let Some(sys) = system {
            if !messages.iter().any(|m| m.role == "system") {
                msgs.push(json!({ "role": "system", "content": sys }));
            }
        }
        for msg in messages {
            msgs.push(json!({ "role": &msg.role, "content": &msg.content }));
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": self.model,
                "messages": msgs,
                "temperature": temp,
                "max_tokens": max_tok,
            }))
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("OpenAI HTTP error: {}", e)))?;

        let status = response.status();
        let data: Value = response
            .json()
            .await
            .map_err(|e| ContribError::Llm(format!("OpenAI JSON parse: {}", e)))?;

        if !status.is_success() {
            let error_msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
            if status.as_u16() == 429 {
                return Err(ContribError::Llm(format!(
                    "OpenAI rate limit: {}",
                    error_msg
                )));
            }
            return Err(ContribError::Llm(format!(
                "OpenAI error {}: {}",
                status, error_msg
            )));
        }

        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");
        Ok(text.to_string())
    }
}

// ── Anthropic Provider ────────────────────────────────────────────────────────

/// Anthropic provider (Claude).
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    temperature: f64,
    max_tokens: u32,
}

impl AnthropicProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let api_key = if !config.api_key.is_empty() {
            config.api_key.clone()
        } else {
            std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| ContribError::Llm("ANTHROPIC_API_KEY not set".into()))?
        };

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());

        info!(model = %config.model, base_url = %base_url, "Anthropic provider");

        Ok(Self {
            client: Client::new(),
            api_key,
            base_url,
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let messages = vec![ChatMessage::user(prompt)];
        self.chat(&messages, system, temperature, max_tokens).await
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let temp = temperature.unwrap_or(self.temperature);
        let max_tok = max_tokens.unwrap_or(self.max_tokens);

        let msgs: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|msg| json!({ "role": &msg.role, "content": &msg.content }))
            .collect();

        let mut body = json!({
            "model": self.model,
            "messages": msgs,
            "temperature": temp,
            "max_tokens": max_tok,
        });

        if let Some(sys) = system {
            body["system"] = Value::String(sys.to_string());
        }

        let response = self
            .client
            .post(self.base_url.to_string() + "/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Anthropic HTTP error: {}", e)))?;

        let status = response.status();
        let data: Value = response
            .json()
            .await
            .map_err(|e| ContribError::Llm(format!("Anthropic JSON parse: {}", e)))?;

        if !status.is_success() {
            let error_msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
            if status.as_u16() == 429 {
                return Err(ContribError::Llm(format!(
                    "Anthropic rate limit: {}",
                    error_msg
                )));
            }
            return Err(ContribError::Llm(format!(
                "Anthropic error {}: {}",
                status, error_msg
            )));
        }

        let text = data["content"][0]["text"].as_str().unwrap_or("");
        Ok(text.to_string())
    }
}

// ── Ollama Provider (local) ───────────────────────────────────────────────────

/// Ollama local model provider.
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
    temperature: f64,
    #[allow(dead_code)]
    max_tokens: u32,
}

impl OllamaProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        info!(model = %config.model, base_url = %base_url, "Ollama provider");

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| ContribError::Llm(format!("HTTP client error: {}", e)))?,
            base_url,
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(ChatMessage::system(sys));
        }
        messages.push(ChatMessage::user(prompt));
        self.chat(&messages, None, temperature, max_tokens).await
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        temperature: Option<f64>,
        _max_tokens: Option<u32>,
    ) -> Result<String> {
        let temp = temperature.unwrap_or(self.temperature);

        let mut msgs: Vec<Value> = Vec::new();
        if let Some(sys) = system {
            if !messages.iter().any(|m| m.role == "system") {
                msgs.push(json!({ "role": "system", "content": sys }));
            }
        }
        for msg in messages {
            msgs.push(json!({ "role": &msg.role, "content": &msg.content }));
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": self.model,
                "messages": msgs,
                "stream": false,
                "options": { "temperature": temp },
            }))
            .send()
            .await
            .map_err(|e| ContribError::Llm(format!("Ollama HTTP error: {}", e)))?;

        let data: Value = response
            .json()
            .await
            .map_err(|e| ContribError::Llm(format!("Ollama JSON parse: {}", e)))?;

        let text = data["message"]["content"].as_str().unwrap_or("");
        Ok(text.to_string())
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Create an LLM provider instance from config, wrapped with retry logic.
pub fn create_llm_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    use super::retry::RetryingProvider;

    let base: Box<dyn LlmProvider> = match config.provider.as_str() {
        "gemini" | "vertex" => Ok(Box::new(GeminiProvider::new(config)?) as Box<dyn LlmProvider>),
        "openai" => Ok(Box::new(OpenAIProvider::new(config)?) as Box<dyn LlmProvider>),
        "anthropic" => Ok(Box::new(AnthropicProvider::new(config)?) as Box<dyn LlmProvider>),
        "ollama" => Ok(Box::new(OllamaProvider::new(config)?) as Box<dyn LlmProvider>),
        other => Err(ContribError::Llm(format!(
            "Unknown LLM provider: {}. Available: gemini, vertex, openai, anthropic, ollama",
            other
        ))),
    }?;

    // Wrap with retry (3 retries, 1s base delay)
    Ok(Box::new(RetryingProvider::with_config(base, 3, 1000)))
}

/// Create an LLM provider WITHOUT retry wrapper (for tests or perf-sensitive paths).
pub fn create_llm_provider_raw(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    match config.provider.as_str() {
        "gemini" | "vertex" => Ok(Box::new(GeminiProvider::new(config)?) as Box<dyn LlmProvider>),
        "openai" => Ok(Box::new(OpenAIProvider::new(config)?) as Box<dyn LlmProvider>),
        "anthropic" => Ok(Box::new(AnthropicProvider::new(config)?) as Box<dyn LlmProvider>),
        "ollama" => Ok(Box::new(OllamaProvider::new(config)?) as Box<dyn LlmProvider>),
        other => Err(ContribError::Llm(format!(
            "Unknown LLM provider: {}. Available: gemini, vertex, openai, anthropic, ollama",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_constructors() {
        let user = ChatMessage::user("hello");
        assert_eq!(user.role, "user");
        assert_eq!(user.content, "hello");

        let system = ChatMessage::system("you are a bot");
        assert_eq!(system.role, "system");

        let assistant = ChatMessage::assistant("hi there");
        assert_eq!(assistant.role, "assistant");
    }

    #[test]
    fn test_create_provider_unknown() {
        let config = LlmConfig {
            provider: "unknown".into(),
            api_key: String::new(),
            model: "test".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let result = create_llm_provider(&config);
        assert!(result.is_err());
        match result {
            Err(e) => assert!(e.to_string().contains("Unknown LLM provider")),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_create_gemini_requires_key() {
        // Providing an explicit key should succeed regardless of env
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: "explicit-test-key".into(),
            model: "gemini-2.5-flash".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let result = create_llm_provider(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_gemini_with_key() {
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: "test-key-12345".into(),
            model: "gemini-2.5-flash".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let result = create_llm_provider(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_openai_with_key() {
        let config = LlmConfig {
            provider: "openai".into(),
            api_key: "sk-test-12345".into(),
            model: "gpt-4o".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let result = create_llm_provider(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_anthropic_with_key() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: "sk-ant-test123".into(),
            model: "claude-sonnet-4-20250514".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let result = create_llm_provider(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_anthropic_default_base_url() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: "sk-ant-test123".into(),
            model: "claude-sonnet-4-20250514".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let p = AnthropicProvider::new(&config).unwrap();
        assert_eq!(p.base_url, "https://api.anthropic.com/v1");
    }

    #[test]
    fn test_anthropic_custom_base_url() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: "sk-ant-test123".into(),
            model: "claude-sonnet-4-20250514".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: Some("https://my-proxy.example.com/v1".into()),
            vertex_project: String::new(),
            vertex_location: "global".into(),
        };
        let p = AnthropicProvider::new(&config).unwrap();
        assert_eq!(p.base_url, "https://my-proxy.example.com/v1");
    }

    #[test]
    fn test_create_gemini_vertex_mode() {
        // Vertex AI mode: no api_key needed when vertex_project is set
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: String::new(),
            model: "gemini-2.5-flash".into(),
            temperature: 0.3,
            max_tokens: 4096,
            base_url: None,
            vertex_project: "my-gcp-project".into(),
            vertex_location: "us-central1".into(),
        };
        let result = create_llm_provider(&config);
        assert!(
            result.is_ok(),
            "Vertex AI mode should succeed without api_key: {:?}",
            result.err().map(|e| e.to_string())
        );
    }
}
