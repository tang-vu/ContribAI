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
//!
//! The deadline covers both direct-child exit and output capture. Cleanup of
//! the direct child has a bounded grace period; descendant processes are not
//! sandboxed or terminated as a group by this runner.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use crate::core::command_safety::{classify_command, CommandClass};
use crate::core::error::{ContribError, Result};
use crate::core::safe_truncate;

/// Default per-command timeout when the caller doesn't set one.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Captured output is bounded — a runaway build cannot exhaust memory.
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// Reaping a killed direct child must not introduce another unbounded wait.
const CHILD_CLEANUP_GRACE: Duration = Duration::from_secs(1);
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
        if ENV_ALLOWLIST.contains(&key.as_str()) || extra_passthrough.contains(&key) {
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
    /// Wall-clock deadline for execution and output capture across the run.
    /// Direct-child cleanup may additionally use `CHILD_CLEANUP_GRACE`.
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

        let now = Instant::now();
        let mut timeout = timeout.unwrap_or(self.default_timeout);
        if let Some(run_deadline) = self.run_deadline {
            let remaining = run_deadline.saturating_duration_since(now);
            if remaining.is_zero() {
                return Ok(gated_outcome(argv, "deadline", "run deadline exceeded"));
            }
            timeout = timeout.min(remaining);
        }
        let deadline = now
            .checked_add(timeout)
            .ok_or_else(|| ContribError::Config("command timeout is too large".into()))?;

        let env = scrub_environment(&self.env_passthrough);
        let program = resolve_program(&argv[0], &env);
        let start = Instant::now();
        let mut child = Command::new(&program)
            .args(&argv[1..])
            .current_dir(&self.workspace_root)
            .env_clear()
            .envs(&env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ContribError::Config(format!("spawn {}: {e}", argv[0])))?;

        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let mut stdout_bytes = Vec::with_capacity(4096);
        let mut stderr_bytes = Vec::with_capacity(4096);

        // Drain both pipes concurrently, including bytes beyond the capture
        // cap. Closing a pipe at the cap can turn successful verbose checks
        // into BrokenPipe failures. Keep these futures scoped to this call:
        // timeout, I/O failure, and cancellation drop their handles instead
        // of leaving detached reader tasks behind.
        let execution = tokio::time::timeout_at(deadline.into(), async {
            tokio::try_join!(
                child.wait(),
                drain_output(stdout, &mut stdout_bytes),
                drain_output(stderr, &mut stderr_bytes),
            )
        })
        .await;

        let timed_out;
        let exit_status;
        match execution {
            Ok(Ok((status, (), ()))) => {
                timed_out = false;
                exit_status = status.code();
            }
            Ok(Err(e)) => {
                stop_child(&mut child).await;
                return Err(ContribError::Config(format!(
                    "execute or capture {}: {e}",
                    argv[0]
                )));
            }
            Err(_) => {
                timed_out = true;
                exit_status = None;
                stop_child(&mut child).await;
            }
        }

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

/// Retain only the evidence prefix, but keep consuming until EOF so the
/// subprocess can finish normally. Read failures must fail the command closed.
async fn drain_output(
    mut reader: impl AsyncRead + Unpin,
    captured: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(());
        }
        let keep = count.min(MAX_OUTPUT_BYTES.saturating_sub(captured.len()));
        captured.extend_from_slice(&chunk[..keep]);
    }
}

async fn stop_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(CHILD_CLEANUP_GRACE, child.wait()).await;
    // kill_on_drop remains the backstop if waiting or cancellation interrupts
    // cleanup. Tokio may subsequently reap an exited direct child best-effort.
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
}

#[cfg(test)]
mod bounded_io_regressions {
    use super::*;
    use std::io::Write;
    use std::pin::Pin;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    #[tokio::test]
    async fn drains_past_capture_limit_without_growing_evidence() {
        let bytes = vec![b'x'; MAX_OUTPUT_BYTES * 4];
        let mut reader = bytes.as_slice();
        let mut captured = Vec::new();
        drain_output(&mut reader, &mut captured).await.unwrap();
        assert!(reader.is_empty(), "excess output must still be consumed");
        assert_eq!(captured, bytes[..MAX_OUTPUT_BYTES]);
    }

    struct ProbeReader {
        dropped: Arc<AtomicBool>,
        fails: bool,
    }

    impl AsyncRead for ProbeReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.fails {
                Poll::Ready(Err(std::io::Error::other("fixture read failure")))
            } else {
                Poll::Pending
            }
        }
    }

    impl Drop for ProbeReader {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn output_read_failure_is_not_silently_accepted() {
        let dropped = Arc::new(AtomicBool::new(false));
        let reader = ProbeReader {
            dropped: dropped.clone(),
            fails: true,
        };
        let mut captured = Vec::new();
        assert!(drain_output(reader, &mut captured).await.is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelled_capture_drops_its_reader() {
        let dropped = Arc::new(AtomicBool::new(false));
        let reader = ProbeReader {
            dropped: dropped.clone(),
            fails: false,
        };
        let mut captured = Vec::new();
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            drain_output(reader, &mut captured)
        )
        .await
        .is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }

    fn helper_argv(name: &str) -> Vec<String> {
        vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--ignored".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            name.into(),
        ]
    }

    // Ignored in normal test runs. Invoked by exact unique name filtering below.
    #[test]
    #[ignore]
    fn regression_helper_noisy_success() {
        let block = [b'x'; 4096];
        for _ in 0..256 {
            std::io::stdout().lock().write_all(&block).unwrap();
            std::io::stderr().lock().write_all(&block).unwrap();
        }
    }

    #[test]
    #[ignore]
    fn regression_helper_hold_inherited_pipes() {
        // Self-terminating backstop: even a failing baseline leaves no infinite
        // subprocess. This helper may survive a failed assertion for <= 8 s.
        std::thread::sleep(Duration::from_secs(8));
    }

    #[test]
    #[ignore]
    // This finite-lived descendant intentionally outlives the fixture parent
    // to exercise inherited-pipe EOF. There is no infinite background helper.
    #[allow(clippy::zombie_processes)]
    fn regression_helper_exit_with_pipe_holder() {
        let args = helper_argv("regression_helper_hold_inherited_pipes");
        let child = std::process::Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        // std::process::Child drop deliberately does not wait or kill.
        drop(child);
    }

    #[tokio::test]
    async fn capture_limit_does_not_fail_a_successful_noisy_command() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(10));
        let args = helper_argv("regression_helper_noisy_success");
        let outcome = tokio::time::timeout(Duration::from_secs(15), runner.run(&args))
            .await
            .expect("runner must return within its deadline and cleanup grace")
            .expect("controlled helper should spawn");
        assert!(
            outcome.passed(),
            "noisy success became failure: {outcome:?}"
        );
        assert!(outcome.stdout_excerpt.len() <= 2000);
        assert!(outcome.stderr_excerpt.len() <= 2000);
        assert_eq!(outcome.output_digest.len(), 64);
    }

    #[tokio::test]
    async fn inherited_pipe_reader_cannot_outlive_command_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(1));
        let args = helper_argv("regression_helper_exit_with_pipe_holder");
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("readers must not hang after the direct child exits")
            .expect("controlled helper should spawn");
        // Any deadline hit in execution/capture must produce non-passing
        // evidence. A fix that cancels a lingering reader cannot silently
        // certify the direct child's zero exit code as complete execution.
        assert!(
            !outcome.passed(),
            "incomplete execution became passing evidence"
        );
        assert!(outcome.timed_out);
        assert_eq!(outcome.gate, "ran");
        assert!(outcome.exit_status.is_none());
    }

    #[tokio::test]
    async fn inherited_pipe_reader_cannot_outlive_run_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(30))
            .with_run_deadline(Instant::now() + Duration::from_secs(1));
        let args = helper_argv("regression_helper_exit_with_pipe_holder");
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("capture must honor the earlier overall run deadline")
            .expect("controlled helper should spawn");
        assert!(!outcome.passed(), "expired run became passing evidence");
        assert!(outcome.timed_out);
        assert_eq!(outcome.gate, "ran");
        assert!(outcome.exit_status.is_none());
    }

    #[tokio::test]
    async fn direct_child_timeout_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_millis(200));
        let args = helper_argv("regression_helper_hold_inherited_pipes");
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("direct-child cleanup must be bounded")
            .expect("controlled helper should spawn");
        assert!(outcome.timed_out);
        assert_eq!(outcome.gate, "ran");
        assert!(!outcome.passed());
        assert!(outcome.exit_status.is_none());
    }

    #[tokio::test]
    async fn extreme_timeout_is_capped_by_run_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::MAX)
            .with_run_deadline(Instant::now() + Duration::from_millis(200));
        let args = helper_argv("regression_helper_hold_inherited_pipes");
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("finite run deadline must cap an extreme command timeout")
            .expect("timeout must be clamped before adding it to Instant");
        assert_eq!(outcome.gate, "ran");
        assert!(outcome.timed_out);
        assert!(!outcome.passed());
    }

    #[tokio::test]
    async fn unrepresentable_timeout_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path()).with_default_timeout(Duration::MAX);
        let result = runner.run(&["git".into(), "status".into()]).await;
        assert!(matches!(result, Err(ContribError::Config(_))));
    }
}
