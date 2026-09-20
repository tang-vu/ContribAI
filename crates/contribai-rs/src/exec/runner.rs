//! Bounded argv command execution.
//!
//! Commands run as raw argv via `tokio::process::Command` — never through a
//! shell. Every invocation is gated by [`crate::core::command_safety`] before
//! spawn: `forbidden` never runs, `requires_approval` runs only when the
//! operator explicitly enabled it, `safe` runs within time/output bounds.
//!
//! The environment is scrubbed: secrets (tokens, keys, credentials) are
//! stripped so a poisoned repository script cannot exfiltrate them, and only
//! a conservative allowlist passes through.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::core::command_safety::{classify_command, CommandClass};
use crate::core::error::{ContribError, Result};
use crate::core::safe_truncate;

/// Default per-command timeout when the caller doesn't set one.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Captured output is bounded — a runaway build cannot exhaust memory.
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// Env vars passed to every child process (platform basics + toolchain
/// discovery). Secrets never pass through regardless of this list.
const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "TEMP",
    "TMP",
    "TMPDIR",
    "COMSPEC",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "NUMBER_OF_PROCESSORS",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "JAVA_HOME",
    "ANDROID_HOME",
    "NODE_ENV",
    "CI",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// Name fragments that mark an environment variable as a secret. Matching is
/// case-insensitive on the variable NAME (values are never inspected).
const SECRET_MARKERS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "PRIVATE",
    "API_KEY",
    "APIKEY",
    "AUTH",
    "SESSION",
    "ACCESS_KEY",
    "SIGNING",
    "ENCRYPT",
];

/// Resolve the program part of an argv to a spawnable path.
///
/// On Windows, package-manager entrypoints like `npm`, `pnpm`, or `yarn`
/// are `.cmd`/`.bat` shims that `CreateProcess` cannot run under a bare
/// name — PATHEXT lookup is a shell feature. Walk the child PATH with the
/// child PATHEXT so `npm` resolves to the real `npm.cmd`; Rust's `Command`
/// then routes batch shims through `cmd.exe` with escaped arguments.
/// Qualified paths and names that already carry an extension are returned
/// unchanged.
fn resolve_program(
    program: &str,
    #[cfg_attr(not(windows), allow(unused_variables))] env: &HashMap<String, String>,
) -> std::path::PathBuf {
    let path = Path::new(program);
    if program.contains('/') || program.contains('\\') || path.extension().is_some() {
        return path.to_path_buf();
    }
    #[cfg(windows)]
    {
        let path_var = env
            .get("PATH")
            .cloned()
            .or_else(|| std::env::var("PATH").ok());
        let pathext = env
            .get("PATHEXT")
            .cloned()
            .or_else(|| std::env::var("PATHEXT").ok())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        if let Some(path_var) = path_var {
            for dir in std::env::split_paths(&path_var) {
                for ext in pathext.split(';').filter(|e| !e.is_empty()) {
                    let candidate = dir.join(format!("{program}{ext}"));
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }
    path.to_path_buf()
}

/// Whether a variable name looks like a secret.
fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_MARKERS.iter().any(|m| upper.contains(m))
}

/// Build the scrubbed child environment: allowlist + caller extras, minus
/// anything that looks like a secret.
pub fn scrub_environment(extra_passthrough: &[String]) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for (key, value) in std::env::vars() {
        if is_secret_name(&key) {
            continue;
        }
        // Environment variable names are case-insensitive on Windows: the OS
        // reports PATH as `Path`, so the allowlist match must be
        // case-insensitive and the canonical spelling kept for the child.
        if let Some(allow) = ENV_ALLOWLIST
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&key))
        {
            env.insert((*allow).to_string(), value);
        } else if extra_passthrough.contains(&key) {
            env.insert(key, value);
        }
    }
    env
}

/// Bounded result of one command execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub argv: Vec<String>,
    /// `safe`, `approval`, `forbidden`, or `spawn_error`.
    pub gate: String,
    pub exit_status: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// Bounded excerpts — never unbounded logs.
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    /// SHA-256 over the bounded captured output, for evidence linkage.
    pub output_digest: String,
}

impl CommandOutcome {
    pub fn passed(&self) -> bool {
        !self.timed_out && self.exit_status == Some(0)
    }
}

/// Executes argv commands inside a workspace under deterministic policy.
pub struct BoundedRunner {
    workspace_root: std::path::PathBuf,
    /// When true, `requires_approval` commands run anyway (explicit operator
    /// opt-in). Forbidden commands never run.
    allow_approval_commands: bool,
    /// Extra env var names allowed to pass through to the child.
    env_passthrough: Vec<String>,
    default_timeout: Duration,
    /// Wall-clock deadline for the whole run; commands never outlive it.
    run_deadline: Option<Instant>,
}

impl BoundedRunner {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            allow_approval_commands: false,
            env_passthrough: Vec::new(),
            default_timeout: DEFAULT_TIMEOUT,
            run_deadline: None,
        }
    }

    /// Explicit operator opt-in to approval-gated commands.
    pub fn with_approval_commands(mut self, allow: bool) -> Self {
        self.allow_approval_commands = allow;
        self
    }

    pub fn with_env_passthrough(mut self, vars: &[String]) -> Self {
        self.env_passthrough = vars.to_vec();
        self
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Bound all commands by the run's wall-clock deadline.
    pub fn with_run_deadline(mut self, deadline: Instant) -> Self {
        self.run_deadline = Some(deadline);
        self
    }

    /// Whether the run deadline has already elapsed.
    pub fn deadline_exceeded(&self) -> bool {
        self.run_deadline.is_some_and(|d| Instant::now() > d)
    }

    /// Classify and execute one argv command.
    ///
    /// Returns an outcome even when the gate denies execution — the caller
    /// records truthful evidence ("not run: forbidden") instead of silence.
    pub async fn run(&self, argv: &[String]) -> Result<CommandOutcome> {
        self.run_inner(argv, None).await
    }

    /// Execute with an indirect argv (e.g., the resolved body of an npm
    /// script). BOTH argvs must classify safe for automatic execution.
    pub async fn run_indirect(
        &self,
        argv: &[String],
        indirect_argv: &[String],
    ) -> Result<CommandOutcome> {
        match classify_command(indirect_argv).class {
            CommandClass::Safe => {}
            CommandClass::RequiresApproval if self.allow_approval_commands => {}
            other => {
                return Ok(gated_outcome(
                    argv,
                    match other {
                        CommandClass::Forbidden => "forbidden",
                        _ => "approval",
                    },
                    "indirect command is not safe to run",
                ));
            }
        }
        self.run_inner(argv, None).await
    }

    async fn run_inner(
        &self,
        argv: &[String],
        timeout: Option<Duration>,
    ) -> Result<CommandOutcome> {
        if argv.is_empty() {
            return Err(ContribError::Config("empty argv".into()));
        }
        match classify_command(argv).class {
            CommandClass::Forbidden => {
                return Ok(gated_outcome(
                    argv,
                    "forbidden",
                    "command class is forbidden",
                ));
            }
            CommandClass::RequiresApproval if !self.allow_approval_commands => {
                return Ok(gated_outcome(
                    argv,
                    "approval",
                    "command requires operator approval",
                ));
            }
            _ => {}
        }

        let mut timeout = timeout.unwrap_or(self.default_timeout);
        if let Some(deadline) = self.run_deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(gated_outcome(argv, "deadline", "run deadline exceeded"));
            }
            timeout = timeout.min(remaining);
        }

        let env = scrub_environment(&self.env_passthrough);
        let program = resolve_program(&argv[0], &env);
        let start = Instant::now();
        let mut child = Command::new(&program)
            .args(&argv[1..])
            .current_dir(&self.workspace_root)
            .env_clear()
            .envs(&env)
            // Bytecode caches must never outlive a single command: a rewrite
            // that lands in the same mtime second with the same byte length
            // would otherwise validate stale compiled sources.
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ContribError::Config(format!("spawn {}: {e}", argv[0])))?;

        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::with_capacity(4096);
            let _ = stdout
                .take(MAX_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut buf)
                .await;
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::with_capacity(4096);
            let _ = stderr
                .take(MAX_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut buf)
                .await;
            buf
        });

        let timed_out;
        let exit_status;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                timed_out = false;
                exit_status = status.code();
            }
            Ok(Err(e)) => {
                return Err(ContribError::Config(format!("wait {}: {e}", argv[0])));
            }
            Err(_) => {
                timed_out = true;
                exit_status = None;
                let _ = child.kill().await;
            }
        }
        let stdout_bytes = stdout_task.await.unwrap_or_default();
        let stderr_bytes = stderr_task.await.unwrap_or_default();

        let mut digest = Sha256::new();
        digest.update(&stdout_bytes);
        digest.update(&stderr_bytes);

        Ok(CommandOutcome {
            argv: argv.to_vec(),
            gate: "ran".into(),
            exit_status,
            timed_out,
            duration_ms: start.elapsed().as_millis() as u64,
            stdout_excerpt: safe_truncate(&String::from_utf8_lossy(&stdout_bytes), 2000)
                .to_string(),
            stderr_excerpt: safe_truncate(&String::from_utf8_lossy(&stderr_bytes), 2000)
                .to_string(),
            output_digest: hex::encode(digest.finalize()),
        })
    }
}

fn gated_outcome(argv: &[String], gate: &str, _reason: &str) -> CommandOutcome {
    CommandOutcome {
        argv: argv.to_vec(),
        gate: gate.into(),
        exit_status: None,
        timed_out: false,
        duration_ms: 0,
        stdout_excerpt: String::new(),
        stderr_excerpt: String::new(),
        output_digest: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn secrets_are_scrubbed_from_environment() {
        std::env::set_var("CONTRIBAI_TEST_SECRET_TOKEN", "hunter2");
        std::env::set_var("CONTRIBAI_TEST_PLAIN", "visible");
        let env = scrub_environment(&["CONTRIBAI_TEST_PLAIN".to_string()]);
        assert!(!env.contains_key("CONTRIBAI_TEST_SECRET_TOKEN"));
        assert_eq!(env.get("CONTRIBAI_TEST_PLAIN").unwrap(), "visible");
        // Allowlist basics survive.
        if std::env::var("PATH").is_ok() {
            assert!(env.contains_key("PATH"));
        }
    }

    #[test]
    fn secret_markers_cover_common_names() {
        for name in [
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "NPM_AUTH_TOKEN",
            "MY_API_KEY",
            "DB_PASSWORD",
            "SSH_AUTH_SOCK",
            "SIGNING_KEY",
        ] {
            assert!(is_secret_name(name), "{name} must be treated as secret");
        }
        for name in ["PATH", "CARGO_HOME", "RUSTUP_HOME", "NODE_ENV"] {
            assert!(!is_secret_name(name), "{name} must pass through");
        }
    }

    #[tokio::test]
    async fn forbidden_commands_never_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path());
        let outcome = runner.run(&argv(&["rm", "-rf", "/"])).await.unwrap();
        assert_eq!(outcome.gate, "forbidden");
        assert!(outcome.exit_status.is_none());
    }

    #[tokio::test]
    async fn approval_commands_gate_without_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path());
        // An unrecognized command classifies as RequiresApproval.
        let outcome = runner
            .run(&argv(&["some_unrecognized_tool", "--version"]))
            .await
            .unwrap();
        assert_eq!(outcome.gate, "approval");
    }

    #[tokio::test]
    async fn safe_command_executes_with_bounded_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), b"hi").unwrap();
        let runner = BoundedRunner::new(dir.path());
        // `git status` is safe; run inside a git repo for a real exit code.
        let init = runner.run(&argv(&["git", "init"])).await.unwrap();
        assert_eq!(init.gate, "ran");
        assert!(init.passed(), "git init failed: {}", init.stderr_excerpt);
        let status = runner.run(&argv(&["git", "status"])).await.unwrap();
        assert!(status.passed());
    }

    #[tokio::test]
    async fn deadline_blocks_commands() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_run_deadline(Instant::now() - Duration::from_secs(1));
        assert!(runner.deadline_exceeded());
        let outcome = runner.run(&argv(&["git", "status"])).await.unwrap();
        assert_eq!(outcome.gate, "deadline");
    }

    #[tokio::test]
    async fn indirect_argv_must_also_be_safe() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path());
        // npm run deploy where deploy="rm -rf /" — the indirect argv is
        // forbidden even though `npm run` itself is benign.
        let outcome = runner
            .run_indirect(&argv(&["npm", "run", "deploy"]), &argv(&["rm", "-rf", "/"]))
            .await
            .unwrap();
        assert_eq!(outcome.gate, "forbidden");
    }

    /// A `__pycache__` left behind by one check could shadow a same-second,
    /// same-length source rewrite in the next — spawned commands must not
    /// persist bytecode caches into the workspace.
    #[cfg(unix)]
    #[tokio::test]
    async fn python_checks_never_leave_bytecode_caches() {
        if std::process::Command::new("python")
            .args(["-m", "pytest", "--version"])
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return; // python+pytest not installed — nothing to assert
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("app.py"), b"def f():\n    return 1\n").unwrap();
        std::fs::write(
            dir.path().join("tests/test_app.py"),
            b"import sys\nfrom pathlib import Path\n\nsys.path.insert(0, str(Path(__file__).resolve().parents[1]))\n\nimport app\n\n\ndef test_f():\n    assert app.f() == 1\n",
        )
        .unwrap();
        let runner = BoundedRunner::new(dir.path());
        let outcome = runner
            .run(&argv(&["python", "-m", "pytest", "-q"]))
            .await
            .unwrap();
        assert_eq!(outcome.gate, "ran");
        assert!(outcome.passed(), "{}", outcome.stderr_excerpt);
        assert!(!dir.path().join("__pycache__").exists());
        assert!(!dir.path().join("tests/__pycache__").exists());
    }
}
