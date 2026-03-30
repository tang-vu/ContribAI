//! Code validation sandbox.
//!
//! Port from Python `sandbox/sandbox.py`.
//! Validates generated code using Docker containers or local fallback.
//! Catches syntax errors before submitting PRs.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::warn;

/// Result of sandbox code validation.
#[derive(Debug, Clone)]
pub struct SandboxResult {
    pub success: bool,
    pub output: String,
    pub errors: String,
    pub language: String,
    pub duration_sec: f64,
}

impl Default for SandboxResult {
    fn default() -> Self {
        Self {
            success: true,
            output: String::new(),
            errors: String::new(),
            language: String::new(),
            duration_sec: 0.0,
        }
    }
}

/// Language → Docker image mapping.
fn language_images() -> HashMap<&'static str, &'static str> {
    [
        ("python", "python:3.12-slim"),
        ("javascript", "node:20-slim"),
        ("typescript", "node:20-slim"),
        ("go", "golang:1.22-alpine"),
        ("rust", "rust:1.77-slim"),
    ]
    .into()
}

/// Language → syntax check command.
fn syntax_check_commands() -> HashMap<&'static str, &'static str> {
    [
        ("python", r#"python -c "import ast, sys; ast.parse(sys.stdin.read())""#),
        ("javascript", "node --check /tmp/code.js"),
        ("typescript", "node --check /tmp/code.ts"),
        ("go", "gofmt -e /tmp/code.go"),
        ("rust", "rustfmt --check /tmp/code.rs"),
    ]
    .into()
}

/// Get file extension for a language.
fn get_extension(language: &str) -> &'static str {
    match language {
        "python" => ".py",
        "javascript" => ".js",
        "typescript" => ".ts",
        "go" => ".go",
        "rust" => ".rs",
        _ => ".txt",
    }
}

/// Validates generated code in isolated Docker containers.
pub struct Sandbox {
    enabled: bool,
    timeout_sec: u64,
    docker_image_override: Option<String>,
}

impl Sandbox {
    pub fn new(enabled: bool, timeout_sec: u64) -> Self {
        Self {
            enabled,
            timeout_sec,
            docker_image_override: None,
        }
    }

    /// Check if Docker is available.
    pub fn docker_available() -> bool {
        which::which("docker").is_ok()
    }

    /// Validate code syntax.
    pub async fn validate(&self, code: &str, language: &str) -> SandboxResult {
        if !self.enabled {
            return SandboxResult {
                success: true,
                output: "Sandbox disabled".into(),
                language: language.into(),
                ..Default::default()
            };
        }

        if !Self::docker_available() {
            warn!("Docker not found, using local validation");
            return self.validate_local(code, language);
        }

        self.validate_docker(code, language).await
    }

    /// Validate multiple files.
    pub async fn validate_batch(
        &self,
        files: &HashMap<String, String>,
        language: &str,
    ) -> HashMap<String, SandboxResult> {
        let mut results = HashMap::new();
        for (path, content) in files {
            results.insert(path.clone(), self.validate(content, language).await);
        }
        results
    }

    /// Validate using Docker container.
    async fn validate_docker(&self, code: &str, language: &str) -> SandboxResult {
        let images = language_images();
        let image = self
            .docker_image_override
            .as_deref()
            .or_else(|| images.get(language).copied());

        let image = match image {
            Some(img) => img,
            None => {
                return SandboxResult {
                    success: true,
                    output: format!("No Docker image for {}, skipping", language),
                    language: language.into(),
                    ..Default::default()
                };
            }
        };

        // Write to temp file
        let ext = get_extension(language);
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("contribai_sandbox{}", ext));
        if let Err(e) = std::fs::write(&temp_path, code) {
            return SandboxResult {
                success: false,
                errors: format!("Cannot write temp file: {}", e),
                language: language.into(),
                ..Default::default()
            };
        }

        let start = Instant::now();
        let result = self
            .run_docker_check(image, &temp_path, language)
            .await;

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);

        let duration = start.elapsed().as_secs_f64();
        SandboxResult {
            duration_sec: duration,
            ..result
        }
    }

    async fn run_docker_check(
        &self,
        image: &str,
        file_path: &Path,
        language: &str,
    ) -> SandboxResult {
        let ext = get_extension(language);
        let container_path = format!("/tmp/code{}", ext);
        let commands = syntax_check_commands();
        let check_cmd = commands
            .get(language)
            .copied()
            .unwrap_or("cat /tmp/code.txt");

        let cmd = format!(
            "docker run --rm --network none \
             -v {}:{}:ro \
             --memory 128m --cpus 0.5 \
             {} sh -c '{}'",
            file_path.display(),
            container_path,
            image,
            check_cmd
        );

        match tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_sec),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .output(),
        )
        .await
        {
            Ok(Ok(output)) => SandboxResult {
                success: output.status.success(),
                output: String::from_utf8_lossy(&output.stdout)
                    .chars()
                    .take(2000)
                    .collect(),
                errors: String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(2000)
                    .collect(),
                language: language.into(),
                ..Default::default()
            },
            Ok(Err(e)) => SandboxResult {
                success: false,
                errors: format!("Docker exec error: {}", e),
                language: language.into(),
                ..Default::default()
            },
            Err(_) => SandboxResult {
                success: false,
                errors: format!("Timeout after {}s", self.timeout_sec),
                language: language.into(),
                duration_sec: self.timeout_sec as f64,
                ..Default::default()
            },
        }
    }

    /// Local fallback validation (without Docker).
    fn validate_local(&self, code: &str, language: &str) -> SandboxResult {
        match language {
            "python" => self.validate_python_local(code),
            "rust" => self.validate_rust_local(code),
            _ => SandboxResult {
                success: true,
                output: format!("No local validator for {}", language),
                language: language.into(),
                ..Default::default()
            },
        }
    }

    /// Validate Python syntax using basic bracket matching.
    fn validate_python_local(&self, code: &str) -> SandboxResult {
        let start = Instant::now();

        // Basic syntax check: balanced brackets + indentation
        let opens: usize = code.matches('(').count()
            + code.matches('[').count()
            + code.matches('{').count();
        let closes: usize = code.matches(')').count()
            + code.matches(']').count()
            + code.matches('}').count();

        if (opens as i64 - closes as i64).unsigned_abs() > 3 {
            return SandboxResult {
                success: false,
                errors: format!("Unbalanced brackets: {} opens, {} closes", opens, closes),
                language: "python".into(),
                duration_sec: start.elapsed().as_secs_f64(),
                ..Default::default()
            };
        }

        // Check for common Python syntax issues
        for (i, line) in code.lines().enumerate() {
            let trimmed = line.trim();
            // Bare `def` or `class` without colon
            if (trimmed.starts_with("def ") || trimmed.starts_with("class "))
                && !trimmed.ends_with(':')
                && !trimmed.contains('#')
            {
                return SandboxResult {
                    success: false,
                    errors: format!("Line {}: missing colon after def/class", i + 1),
                    language: "python".into(),
                    duration_sec: start.elapsed().as_secs_f64(),
                    ..Default::default()
                };
            }
        }

        SandboxResult {
            success: true,
            output: "Syntax OK (local check)".into(),
            language: "python".into(),
            duration_sec: start.elapsed().as_secs_f64(),
            ..Default::default()
        }
    }

    /// Validate Rust syntax using basic checks.
    fn validate_rust_local(&self, code: &str) -> SandboxResult {
        let start = Instant::now();

        let opens = code.matches('{').count();
        let closes = code.matches('}').count();

        if opens != closes {
            return SandboxResult {
                success: false,
                errors: format!("Unbalanced braces: {} opens, {} closes", opens, closes),
                language: "rust".into(),
                duration_sec: start.elapsed().as_secs_f64(),
                ..Default::default()
            };
        }

        SandboxResult {
            success: true,
            output: "Syntax OK (local check)".into(),
            language: "rust".into(),
            duration_sec: start.elapsed().as_secs_f64(),
            ..Default::default()
        }
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new(false, 30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_disabled() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sandbox = Sandbox::default();

        let result = rt.block_on(sandbox.validate("print('hello')", "python"));
        assert!(result.success);
        assert!(result.output.contains("disabled"));
    }

    #[test]
    fn test_python_local_valid() {
        let sandbox = Sandbox::new(true, 30);
        let result = sandbox.validate_python_local("def foo():\n    return 42\n");
        assert!(result.success);
    }

    #[test]
    fn test_python_local_missing_colon() {
        let sandbox = Sandbox::new(true, 30);
        let result = sandbox.validate_python_local("def foo()\n    return 42\n");
        assert!(!result.success);
        assert!(result.errors.contains("colon"));
    }

    #[test]
    fn test_python_local_unbalanced() {
        let sandbox = Sandbox::new(true, 30);
        let result = sandbox.validate_python_local("foo(((((\n");
        assert!(!result.success);
        assert!(result.errors.contains("Unbalanced"));
    }

    #[test]
    fn test_rust_local_valid() {
        let sandbox = Sandbox::new(true, 30);
        let result = sandbox.validate_rust_local("fn main() {\n    println!(\"hello\");\n}\n");
        assert!(result.success);
    }

    #[test]
    fn test_rust_local_unbalanced() {
        let sandbox = Sandbox::new(true, 30);
        let result = sandbox.validate_rust_local("fn main() {\n    println!(\"hello\");\n");
        assert!(!result.success);
    }

    #[test]
    fn test_get_extension() {
        assert_eq!(get_extension("python"), ".py");
        assert_eq!(get_extension("rust"), ".rs");
        assert_eq!(get_extension("unknown"), ".txt");
    }
}
