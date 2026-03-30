//! Context compressor for token-efficient LLM interactions.
//!
//! Port from Python `analysis/context_compressor.py`.
//! Truncation-based compression to stay within token limits.

use regex::Regex;
use tracing::debug;

use crate::core::error::Result;
use crate::llm::provider::LlmProvider;

/// Rough estimate: 1 token ≈ 4 chars for English code.
const CHARS_PER_TOKEN: usize = 4;

/// System prompt used in `summarize_with_llm`.
const COMPRESSION_SYSTEM: &str = "You are a concise technical summarizer.";

/// User prompt template for LLM-driven compression.
/// The `{context}` placeholder is replaced at call time.
const COMPRESSION_PROMPT: &str = concat!(
    "You are a context compressor. Given the following analysis context, ",
    "produce a structured summary that preserves critical information ",
    "while being as concise as possible.\n\n",
    "Respond in this exact format:\n",
    "TASK_OVERVIEW: <1-2 sentence overview of what was being analyzed>\n",
    "CURRENT_STATE: <key findings, patterns, issues discovered>\n",
    "IMPORTANT_DISCOVERIES: <most significant items, bullet each>\n",
    "CONTEXT_TO_PRESERVE: <anything needed for follow-up work>\n\n",
    "--- CONTEXT TO COMPRESS ---\n"
);

/// Output template filled from parsed LLM response sections.
const SUMMARY_TEMPLATE: &str = concat!(
    "# Task Overview\n{task_overview}\n\n",
    "# Current State\n{current_state}\n\n",
    "# Important Discoveries\n{important_discoveries}\n\n",
    "# Context to Preserve\n{context_to_preserve}"
);

/// Compresses file content and analysis context to fit within token budgets.
pub struct ContextCompressor {
    max_tokens: usize,
    max_chars: usize,
}

impl ContextCompressor {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            max_chars: max_tokens * CHARS_PER_TOKEN,
        }
    }

    /// Compress a map of {path: content} to fit within budget.
    pub fn compress_files(
        &self,
        files: &[(&str, &str)],
        max_per_file_tokens: usize,
    ) -> Vec<(String, String)> {
        let max_per_file_chars = max_per_file_tokens * CHARS_PER_TOKEN;
        let total_budget = self.max_chars;
        let mut compressed = Vec::new();
        let mut total_chars = 0;

        // Sort by size — process smallest first to keep more files
        let mut sorted: Vec<_> = files.to_vec();
        sorted.sort_by_key(|(_, content)| content.len());

        for (path, content) in sorted {
            let remaining = total_budget.saturating_sub(total_chars);
            if remaining == 0 {
                break;
            }

            let per_file_limit = max_per_file_chars.min(remaining);
            let result = if content.len() <= per_file_limit {
                content.to_string()
            } else {
                Self::truncate_middle(content, per_file_limit)
            };

            total_chars += result.len();
            compressed.push((path.to_string(), result));
        }

        compressed
    }

    /// Compress arbitrary text to fit within token budget.
    pub fn compress_text(&self, text: &str, max_tokens: Option<usize>) -> String {
        let limit = (max_tokens.unwrap_or(self.max_tokens)) * CHARS_PER_TOKEN;
        if text.len() <= limit {
            return text.to_string();
        }
        Self::truncate_middle(text, limit)
    }

    /// Keep first and last portions, replace middle with marker.
    fn truncate_middle(text: &str, max_chars: usize) -> String {
        if text.len() <= max_chars {
            return text.to_string();
        }
        // 60% head, 40% tail
        let head_size = (max_chars as f64 * 0.6) as usize;
        let marker_size = 60;
        let tail_size = max_chars.saturating_sub(head_size + marker_size);

        if tail_size == 0 {
            return text.chars().take(max_chars).collect();
        }

        let omitted = text.len() - head_size - tail_size;
        let head: String = text.chars().take(head_size).collect();
        let tail: String = text.chars().skip(text.len() - tail_size).collect();

        format!(
            "{}\n\n... ({} chars / ~{} tokens omitted) ...\n\n{}",
            head,
            omitted,
            omitted / CHARS_PER_TOKEN,
            tail
        )
    }

    /// Compact finding summary for prompt injection.
    pub fn summarize_findings_compact(findings: &[crate::core::models::Finding]) -> String {
        if findings.is_empty() {
            return "No issues.".to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        for f in findings.iter().take(10) {
            parts.push(format!("[{}] {} ({})", f.severity, f.title, f.file_path));
        }
        if findings.len() > 10 {
            parts.push(format!(" +{} more", findings.len() - 10));
        }
        parts.join("\n")
    }

    // ── Signature extraction ──────────────────────────────────────────────────

    /// Extract key structural elements (imports, class/function signatures)
    /// from source code, stripping implementation bodies.
    ///
    /// Supports Python, JavaScript/TypeScript, Rust, Go, and Java.
    /// Falls back to head+tail snippet for unknown languages.
    pub fn extract_signatures(&self, content: &str, language: &str) -> String {
        debug!(language = %language, chars = content.len(), "extract_signatures");
        match language {
            "python" | "py" => Self::extract_python_signatures(content),
            "javascript" | "js" | "typescript" | "ts" | "jsx" | "tsx" => {
                Self::extract_js_ts_signatures(content)
            }
            "rust" | "rs" => Self::extract_rust_signatures(content),
            "go" => Self::extract_go_signatures(content),
            "java" => Self::extract_java_signatures(content),
            _ => {
                // Fallback: keep first 50 and last 20 lines
                let lines: Vec<&str> = content.lines().collect();
                if lines.len() <= 70 {
                    return content.to_string();
                }
                let head = &lines[..50];
                let tail = &lines[lines.len() - 20..];
                let omitted = lines.len() - 70;
                format!(
                    "{}\n... ({} lines omitted) ...\n{}",
                    head.join("\n"),
                    omitted,
                    tail.join("\n")
                )
            }
        }
    }

    /// Detect language from a file extension (e.g. "foo.rs" → "rust").
    pub fn detect_language(path: &str) -> &'static str {
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext {
            "py" => "python",
            "js" | "mjs" | "cjs" => "javascript",
            "ts" | "tsx" | "jsx" => "typescript",
            "rs" => "rust",
            "go" => "go",
            "java" => "java",
            _ => "unknown",
        }
    }

    /// 3-tier compression: full → signatures → truncated.
    ///
    /// Same signature as `compress_files` but uses `extract_signatures`
    /// as an intermediate step before falling back to `truncate_middle`.
    pub fn compress_files_with_signatures(
        &self,
        files: &[(&str, &str)],
        max_per_file_tokens: usize,
    ) -> Vec<(String, String)> {
        let max_per_file_chars = max_per_file_tokens * CHARS_PER_TOKEN;
        let total_budget = self.max_chars;
        let mut compressed = Vec::new();
        let mut total_chars = 0usize;

        // Sort by size — process smallest first to keep more files
        let mut sorted: Vec<_> = files.to_vec();
        sorted.sort_by_key(|(_, content)| content.len());

        for (path, content) in sorted {
            let remaining = total_budget.saturating_sub(total_chars);
            if remaining == 0 {
                debug!(%path, "budget exhausted, skipping");
                break;
            }

            let per_file_limit = max_per_file_chars.min(remaining);

            let result = if content.len() <= per_file_limit {
                // Tier 1: fits as-is
                content.to_string()
            } else {
                // Tier 2: try signatures
                let lang = Self::detect_language(path);
                let sigs = self.extract_signatures(content, lang);
                debug!(
                    %path,
                    original = content.len(),
                    signatures = sigs.len(),
                    limit = per_file_limit,
                    "signature extraction"
                );
                if sigs.len() <= per_file_limit {
                    sigs
                } else {
                    // Tier 3: truncate
                    Self::truncate_middle(&sigs, per_file_limit)
                }
            };

            total_chars += result.len();
            compressed.push((path.to_string(), result));
        }

        compressed
    }

    /// Use an LLM to create a structured summary of analysis context.
    ///
    /// Produces a compact summary using a fixed template (Task Overview,
    /// Current State, Important Discoveries, Context to Preserve).
    /// Falls back to `compress_text` on LLM error.
    pub async fn summarize_with_llm(
        context: &str,
        llm: &dyn LlmProvider,
        max_summary_tokens: usize,
    ) -> Result<String> {
        // Cap input to avoid runaway token spend (4× budget as chars)
        let input_cap = max_summary_tokens * CHARS_PER_TOKEN * 4;
        let capped_context = crate::core::safe_truncate(context, input_cap);

        let prompt = format!("{}{}", COMPRESSION_PROMPT, capped_context);

        match llm
            .complete(
                &prompt,
                Some(COMPRESSION_SYSTEM),
                None,
                Some(max_summary_tokens as u32),
            )
            .await
        {
            Ok(response) => {
                // Parse structured response into template fields
                let mut task_overview = String::new();
                let mut current_state = String::new();
                let mut important_discoveries = String::new();
                let mut context_to_preserve = String::new();

                for line in response.trim().lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("TASK_OVERVIEW:") {
                        task_overview = rest.trim().to_string();
                    } else if let Some(rest) = trimmed.strip_prefix("CURRENT_STATE:") {
                        current_state = rest.trim().to_string();
                    } else if let Some(rest) = trimmed.strip_prefix("IMPORTANT_DISCOVERIES:") {
                        important_discoveries = rest.trim().to_string();
                    } else if let Some(rest) = trimmed.strip_prefix("CONTEXT_TO_PRESERVE:") {
                        context_to_preserve = rest.trim().to_string();
                    }
                }

                let summary = SUMMARY_TEMPLATE
                    .replace("{task_overview}", &task_overview)
                    .replace("{current_state}", &current_state)
                    .replace("{important_discoveries}", &important_discoveries)
                    .replace("{context_to_preserve}", &context_to_preserve);

                debug!(
                    original = context.len(),
                    compressed = summary.len(),
                    reduction_pct = ((1.0 - summary.len() as f64 / context.len().max(1) as f64)
                        * 100.0) as u32,
                    "LLM compression complete"
                );
                Ok(summary)
            }
            Err(e) => {
                debug!(error = %e, "LLM compression failed, falling back to truncation");
                let fallback = ContextCompressor::new(max_summary_tokens);
                Ok(fallback.compress_text(context, None))
            }
        }
    }

    // ── Per-language signature extractors ────────────────────────────────────

    /// Extract imports, class/function signatures from Python source.
    fn extract_python_signatures(content: &str) -> String {
        // Regex for definition starters — class, def, async def
        let re_def = Regex::new(r"^(class |def |async def )").expect("valid regex");
        // Regex for module-level UPPERCASE constants (e.g. `FOO_BAR = ...`)
        let re_const = Regex::new(r"^[A-Z_][A-Z_0-9]+ =").expect("valid regex");

        let mut result: Vec<&str> = Vec::new();
        let mut in_docstring = false;

        for line in content.lines() {
            let stripped = line.trim();

            // Track triple-quote docstring boundaries
            let dq = stripped.matches("\"\"\"").count();
            let sq = stripped.matches("'''").count();
            let toggle_count = dq + sq;
            if toggle_count % 2 == 1 {
                in_docstring = !in_docstring;
                if !in_docstring {
                    continue;
                }
            }
            if in_docstring {
                continue;
            }

            let is_import = stripped.starts_with("import ") || stripped.starts_with("from ");
            let is_def = re_def.is_match(stripped);
            let is_decorator = stripped.starts_with('@');
            // Module-level constant: must not be indented
            let is_const = re_const.is_match(stripped)
                && !line.starts_with(' ')
                && !line.starts_with('\t');

            if is_import || is_def || is_decorator || is_const {
                result.push(line);
            }
        }

        result.join("\n")
    }

    /// Extract function/class/export signatures from JavaScript or TypeScript.
    fn extract_js_ts_signatures(content: &str) -> String {
        // Patterns kept: function declarations, class declarations, exports,
        // arrow-function const bindings, type/interface declarations, decorators.
        let re_sig = Regex::new(
            r"(?x)^
            (
              export\s |          # export ...
              function\s |        # function foo(
              class\s |           # class Foo
              async\s+function\s | # async function
              const\s+\w+\s*=\s*( # const foo = (
                async\s+
              )?\( |
              (export\s+)?(default\s+)?(abstract\s+)?class\s |
              (export\s+)?(type|interface)\s |
              @\w                 # decorators
            )",
        )
        .expect("valid regex");

        let mut result: Vec<&str> = Vec::new();
        for line in content.lines() {
            let stripped = line.trim();
            if re_sig.is_match(stripped) {
                result.push(line);
            }
        }
        result.join("\n")
    }

    /// Extract pub fn/struct/enum/impl/trait signatures from Rust source.
    fn extract_rust_signatures(content: &str) -> String {
        // Non-verbose regex: pub fn/struct/enum/trait/type/const, impl blocks,
        // use/mod declarations, and attribute macros (#[...).
        let re_sig = Regex::new(
            r"^(pub(\s*\([\w:, ]+\))?\s+(async\s+)?fn\s|pub(\s*\([\w:, ]+\))?\s+struct\s|pub(\s*\([\w:, ]+\))?\s+enum\s|pub(\s*\([\w:, ]+\))?\s+trait\s|pub(\s*\([\w:, ]+\))?\s+type\s|pub(\s*\([\w:, ]+\))?\s+const\s|impl(\s+\w[\w<>, :]+)?\s*\{?\s*$|use\s|mod\s|#\[)",
        )
        .expect("valid regex");

        let mut result: Vec<&str> = Vec::new();
        for line in content.lines() {
            let stripped = line.trim();
            if re_sig.is_match(stripped) {
                result.push(line);
            }
        }
        result.join("\n")
    }

    /// Extract function/type/struct signatures from Go source.
    fn extract_go_signatures(content: &str) -> String {
        let re_sig = Regex::new(
            r"(?x)^
            (
              func\s |            # func Foo(
              type\s+\w+\s+struct | # type Foo struct
              type\s+\w+\s+interface | # type Foo interface
              type\s+\w+\s |      # type alias
              import\s |          # import (
              package\s           # package main
            )",
        )
        .expect("valid regex");

        let mut result: Vec<&str> = Vec::new();
        for line in content.lines() {
            let stripped = line.trim();
            if re_sig.is_match(stripped) {
                result.push(line);
            }
        }
        result.join("\n")
    }

    /// Extract class/method/field signatures from Java source.
    fn extract_java_signatures(content: &str) -> String {
        let re_sig = Regex::new(
            r"(?x)^
            (
              (public|protected|private|static|final|abstract|synchronized)\s | # modifiers
              class\s |           # class Foo
              interface\s |       # interface Bar
              enum\s |            # enum Status
              @\w |               # annotations
              import\s |          # import statements
              package\s           # package declaration
            )",
        )
        .expect("valid regex");

        let mut result: Vec<&str> = Vec::new();
        for line in content.lines() {
            let stripped = line.trim();
            if re_sig.is_match(stripped) {
                result.push(line);
            }
        }
        result.join("\n")
    }
}

impl Default for ContextCompressor {
    fn default() -> Self {
        Self::new(30_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_text_small() {
        let c = ContextCompressor::new(100);
        let text = "hello world";
        assert_eq!(c.compress_text(text, None), text);
    }

    #[test]
    fn test_compress_text_large() {
        let c = ContextCompressor::new(50); // 200 chars max
        let text = "a".repeat(1000);
        let result = c.compress_text(&text, None);
        // Result should be significantly smaller than original
        assert!(result.len() < 500, "Expected < 500, got {}", result.len());
        assert!(result.contains("omitted"));
    }

    #[test]
    fn test_compress_files_fits() {
        let c = ContextCompressor::new(10000);
        let files = vec![("a.py", "print('hello')"), ("b.py", "x = 1")];
        let result = c.compress_files(&files, 1000);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_truncate_middle() {
        let text = "a".repeat(500);
        let result = ContextCompressor::truncate_middle(&text, 200);
        // head=120 + tail=20 + marker ≈ reasonable size
        assert!(result.contains("omitted"), "Should contain omitted marker");
        // Total content (head+tail) should be less than original
        assert!(result.len() < 500, "Expected < 500, got {}", result.len());
    }

    // ── extract_signatures tests ──────────────────────────────────────────────

    #[test]
    fn test_extract_signatures_python_keeps_defs() {
        let c = ContextCompressor::default();
        let src = r#"import os
from typing import List

MAX_RETRIES = 3

class Foo:
    """Docstring — should be stripped."""

    def bar(self, x: int) -> str:
        return str(x)

    async def baz(self) -> None:
        pass

def standalone(y: float) -> float:
    # body comment — should not appear
    return y * 2.0
"#;
        let result = c.extract_signatures(src, "python");

        // Imports preserved
        assert!(result.contains("import os"), "missing import os");
        assert!(result.contains("from typing import List"), "missing from-import");
        // Module-level constant preserved
        assert!(result.contains("MAX_RETRIES = 3"), "missing constant");
        // Class and function signatures preserved
        assert!(result.contains("class Foo:"), "missing class Foo");
        assert!(result.contains("def bar(self, x: int) -> str:"), "missing def bar");
        assert!(result.contains("async def baz(self) -> None:"), "missing async def baz");
        assert!(result.contains("def standalone(y: float) -> float:"), "missing standalone");
        // Bodies stripped
        assert!(!result.contains("return str(x)"), "body should be stripped");
        assert!(!result.contains("body comment"), "comment should be stripped");
        // Docstring stripped
        assert!(!result.contains("Docstring"), "docstring should be stripped");
    }

    #[test]
    fn test_extract_signatures_python_short_file_unchanged() {
        let c = ContextCompressor::default();
        let src = "x = 1\ny = 2\n";
        // Short file with no defs → result is a subset (possibly empty), not crash
        let result = c.extract_signatures(src, "python");
        // Neither line starts with import/def/class/decorator/constant-pattern,
        // so result may be empty — just ensure no panic and it's shorter or equal.
        assert!(result.len() <= src.len());
    }

    #[test]
    fn test_extract_signatures_js_keeps_exports_and_classes() {
        let c = ContextCompressor::default();
        let src = r#"import { foo } from './foo';

export function greet(name: string): string {
    return `Hello, ${name}`;
}

export class MyService {
    private db: Database;

    constructor(db: Database) {
        this.db = db;
    }
}

export const handler = async (req: Request) => {
    // implementation
};

export type UserId = string;
export interface Config {
    debug: boolean;
}
"#;
        let result = c.extract_signatures(src, "typescript");

        assert!(result.contains("export function greet"), "missing greet fn");
        assert!(result.contains("export class MyService"), "missing class");
        assert!(result.contains("export const handler"), "missing const handler");
        assert!(result.contains("export type UserId"), "missing type alias");
        assert!(result.contains("export interface Config"), "missing interface");
        // Implementation body should not appear
        assert!(!result.contains("Hello,"), "body should be stripped");
        assert!(!result.contains("implementation"), "comment should be stripped");
    }

    #[test]
    fn test_extract_signatures_rust_keeps_pub_items() {
        let c = ContextCompressor::default();
        let src = r#"use std::collections::HashMap;
use crate::core::error::Result;

pub struct Analyzer {
    config: Config,
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Processor: Send + Sync {
    fn process(&self, input: &str) -> Result<String>;
}

impl Analyzer {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(&self) -> Result<()> {
        // do work
        Ok(())
    }

    fn private_helper(&self) {}
}
"#;
        let result = c.extract_signatures(src, "rust");

        assert!(result.contains("use std::collections::HashMap"), "missing use");
        assert!(result.contains("pub struct Analyzer"), "missing struct");
        assert!(result.contains("pub enum Status"), "missing enum");
        assert!(result.contains("pub trait Processor"), "missing trait");
        assert!(result.contains("pub fn new"), "missing pub fn new");
        assert!(result.contains("pub async fn run"), "missing pub async fn run");
        // Private helper — `fn private_helper` is NOT pub; should not appear
        // (our regex only matches `pub ... fn`)
        // Bodies stripped
        assert!(!result.contains("do work"), "body comment should be stripped");
        assert!(!result.contains("Self { config }"), "impl body should be stripped");
    }

    #[test]
    fn test_extract_signatures_unknown_language_short_file() {
        let c = ContextCompressor::default();
        // < 70 lines → returned unchanged
        let src = "line1\nline2\n";
        let result = c.extract_signatures(src, "cobol");
        assert_eq!(result, src);
    }

    #[test]
    fn test_detect_language_mapping() {
        assert_eq!(ContextCompressor::detect_language("foo.rs"), "rust");
        assert_eq!(ContextCompressor::detect_language("bar.py"), "python");
        assert_eq!(ContextCompressor::detect_language("baz.ts"), "typescript");
        assert_eq!(ContextCompressor::detect_language("qux.js"), "javascript");
        assert_eq!(ContextCompressor::detect_language("main.go"), "go");
        assert_eq!(ContextCompressor::detect_language("App.java"), "java");
        assert_eq!(ContextCompressor::detect_language("unknown.xyz"), "unknown");
    }

    #[test]
    fn test_compress_files_with_signatures_3tier() {
        let c = ContextCompressor::new(100); // small budget to force compression

        // Build a Python file that is larger than per_file budget
        let big_py = format!(
            "import os\n\ndef foo():\n{}\n\ndef bar():\n{}\n",
            "    x = 1\n".repeat(50),
            "    y = 2\n".repeat(50),
        );
        let files = vec![("big.py", big_py.as_str()), ("tiny.py", "x = 1")];

        let result = c.compress_files_with_signatures(&files, 50);
        // tiny.py fits → included unchanged
        let tiny = result.iter().find(|(p, _)| p == "tiny.py");
        assert!(tiny.is_some(), "tiny.py should be in result");
        assert_eq!(tiny.unwrap().1, "x = 1");

        // big.py should be included but compressed
        let big = result.iter().find(|(p, _)| p == "big.py");
        assert!(big.is_some(), "big.py should be in result (signatures or truncated)");
        let big_content = &big.unwrap().1;
        // At minimum it should contain one of the signatures or an omitted marker
        assert!(
            big_content.contains("def foo")
                || big_content.contains("def bar")
                || big_content.contains("omitted"),
            "Expected signatures or truncation marker, got: {}",
            &big_content[..big_content.len().min(200)]
        );
    }
}
