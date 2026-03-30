//! Message formatter abstraction for multi-provider LLM support.
//!
//! Port from Python `llm/formatter.py`.

use serde_json::{json, Value};

/// Format messages for a specific LLM provider.
pub trait MessageFormatter {
    fn format_messages(
        &self,
        messages: &[Message],
        system: Option<&str>,
    ) -> Value;

    fn format_prompt(
        &self,
        prompt: &str,
        system: Option<&str>,
    ) -> Value;
}

/// A generic chat message.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ── Provider Formatters ──────────────────────────────

/// Gemini: Content/Part format.
pub struct GeminiFormatter;

impl MessageFormatter for GeminiFormatter {
    fn format_messages(&self, messages: &[Message], system: Option<&str>) -> Value {
        let contents: Vec<Value> = messages
            .iter()
            .map(|m| {
                let role = if m.role == "assistant" { "model" } else { "user" };
                json!({ "role": role, "parts": [{ "text": m.content }] })
            })
            .collect();
        json!({ "contents": contents, "system_instruction": system })
    }

    fn format_prompt(&self, prompt: &str, system: Option<&str>) -> Value {
        json!({ "contents": prompt, "system_instruction": system })
    }
}

/// OpenAI: chat completion format.
pub struct OpenAIFormatter;

impl MessageFormatter for OpenAIFormatter {
    fn format_messages(&self, messages: &[Message], system: Option<&str>) -> Value {
        let mut formatted: Vec<Value> = Vec::new();
        if let Some(sys) = system {
            formatted.push(json!({ "role": "system", "content": sys }));
        }
        for m in messages {
            formatted.push(json!({ "role": m.role, "content": m.content }));
        }
        json!(formatted)
    }

    fn format_prompt(&self, prompt: &str, system: Option<&str>) -> Value {
        let mut messages: Vec<Value> = Vec::new();
        if let Some(sys) = system {
            messages.push(json!({ "role": "system", "content": sys }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));
        json!(messages)
    }
}

/// Anthropic: system prompt separate from messages.
pub struct AnthropicFormatter;

impl MessageFormatter for AnthropicFormatter {
    fn format_messages(&self, messages: &[Message], system: Option<&str>) -> Value {
        let filtered: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();
        json!({ "messages": filtered, "system": system.unwrap_or("") })
    }

    fn format_prompt(&self, prompt: &str, system: Option<&str>) -> Value {
        json!({
            "messages": [{ "role": "user", "content": prompt }],
            "system": system.unwrap_or("")
        })
    }
}

/// Ollama: OpenAI-compatible.
pub struct OllamaFormatter;

impl MessageFormatter for OllamaFormatter {
    fn format_messages(&self, messages: &[Message], system: Option<&str>) -> Value {
        OpenAIFormatter.format_messages(messages, system)
    }

    fn format_prompt(&self, prompt: &str, system: Option<&str>) -> Value {
        OpenAIFormatter.format_prompt(prompt, system)
    }
}

/// Get formatter for a provider name.
pub fn get_formatter(provider: &str) -> Box<dyn MessageFormatter> {
    match provider {
        "gemini" => Box::new(GeminiFormatter),
        "anthropic" => Box::new(AnthropicFormatter),
        "ollama" => Box::new(OllamaFormatter),
        _ => Box::new(OpenAIFormatter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_messages() -> Vec<Message> {
        vec![
            Message { role: "user".into(), content: "Hello".into() },
            Message { role: "assistant".into(), content: "Hi".into() },
        ]
    }

    #[test]
    fn test_gemini_formatter() {
        let f = GeminiFormatter;
        let result = f.format_messages(&test_messages(), Some("system"));
        assert!(result["contents"].is_array());
        assert_eq!(result["system_instruction"], "system");
    }

    #[test]
    fn test_openai_formatter() {
        let f = OpenAIFormatter;
        let result = f.format_messages(&test_messages(), Some("sys"));
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["role"], "system");
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn test_anthropic_formatter() {
        let f = AnthropicFormatter;
        let result = f.format_messages(&test_messages(), Some("sys"));
        assert_eq!(result["system"], "sys");
        assert_eq!(result["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_get_formatter() {
        let f = get_formatter("gemini");
        let result = f.format_prompt("hello", None);
        assert_eq!(result["contents"], "hello");
    }

    #[test]
    fn test_prompt_format() {
        let f = OpenAIFormatter;
        let result = f.format_prompt("question", Some("be helpful"));
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["content"], "be helpful");
    }
}
