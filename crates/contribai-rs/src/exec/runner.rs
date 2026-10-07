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
//! The deadline bounds the runner future, including direct-child exit and
//! async output capture, with a bounded direct-child cleanup grace period.
//! Output uses cancellable OS pipe reads, including overlapped named pipes on
//! Windows, so inherited writers cannot keep this runner's reads alive during
//! runtime shutdown. Descendant processes are not sandboxed or terminated as
//! a group by this runner.

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
        let mut command = Command::new(&program);
        command
            .args(&argv[1..])
            .current_dir(&self.workspace_root)
            .env_clear()
            .envs(&env)
            .kill_on_drop(true);

        #[cfg(windows)]
        let (stdout, stderr) = {
            let pipes = tokio::time::timeout_at(deadline.into(), async {
                let stdout = windows_output_pipe().await?;
                let stderr = windows_output_pipe().await?;
                Ok::<_, std::io::Error>((stdout, stderr))
            })
            .await
            .map_err(|_| ContribError::Config("output pipe setup deadline exceeded".into()))?
            .map_err(|e| ContribError::Config(format!("prepare output pipes: {e}")))?;
            let ((stdout, stdout_writer), (stderr, stderr_writer)) = pipes;
            command.stdout(stdout_writer).stderr(stderr_writer);
            (stdout, stderr)
        };
        #[cfg(not(windows))]
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|e| ContribError::Config(format!("spawn {}: {e}", argv[0])))?;
        // Command retains configured Stdio handles for reuse. Release our
        // writer copies now, otherwise a successful child can never reach EOF.
        drop(command);

        #[cfg(not(windows))]
        let stdout = child.stdout.take().expect("piped");
        #[cfg(not(windows))]
        let stderr = child.stderr.take().expect("piped");
        let mut stdout_bytes = Vec::with_capacity(4096);
        let mut stderr_bytes = Vec::with_capacity(4096);

        // Drain both pipes concurrently, including bytes beyond the capture
        // cap. Closing a pipe at the cap can turn successful verbose checks
        // into BrokenPipe failures. Keep these futures scoped to this call:
        // timeout, I/O failure, and cancellation drop their async handles
        // instead of detaching our reader futures. Windows named-pipe reads
        // use overlapped I/O rather than Tokio's blocking child-pipe adapter.
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

/// Connect a synchronous child writer to a cancellable, overlapped reader.
/// Tokio's Windows ChildStdout/ChildStderr instead use blocking reads, which
/// can delay runtime destruction while a descendant retains a writer.
#[cfg(windows)]
async fn windows_output_pipe() -> std::io::Result<(
    tokio::net::windows::named_pipe::NamedPipeServer,
    std::process::Stdio,
)> {
    let name = format!(r"\\.\pipe\contribai-output-{}", uuid::Uuid::new_v4());
    let (reader, writer) = windows_output_pipe_named(&name).await?;
    Ok((reader, writer.into()))
}

#[cfg(windows)]
async fn windows_output_pipe_named(
    name: &str,
) -> std::io::Result<(
    tokio::net::windows::named_pipe::NamedPipeServer,
    std::fs::File,
)> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    // One inbound-only instance: no other server can claim the name, and a
    // read-only client cannot observe output. Windows' default pipe DACL
    // restricts writers to the owner/admins; remote clients are rejected.
    // Keep the writer synchronous for ordinary child stdout/stderr APIs.
    let reader = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .access_inbound(true)
        .access_outbound(false)
        .reject_remote_clients(true)
        .pipe_mode(PipeMode::Byte)
        .create(name)?;
    let writer = std::fs::OpenOptions::new().write(true).open(name)?;
    // Complete the connection while our writer is still alive. Connecting
    // after spawn races a fast child's exit and can report ERROR_NO_DATA.
    reader.connect().await?;
    Ok((reader, writer))
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

    const CALL_WALL_CLOCK_BOUND: Duration = Duration::from_secs(3);
    const PROCESS_PROBE_BACKSTOP: Duration = Duration::from_secs(15);
    const PROCESS_PROBE_REAP_GRACE: Duration = Duration::from_secs(2);
    const PROCESS_PROBE_FILE: &str = "runner-runtime-probe.json";
    const HOLDER_READY: &str = "pipe-holder-ready";
    const HOLDER_CHECK: &str = "pipe-holder-check";
    const HOLDER_ALIVE: &str = "pipe-holder-alive";
    const HOLDER_RELEASE: &str = "pipe-holder-release";
    const HOLDER_DONE: &str = "pipe-holder-done";

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    struct RuntimeProbe {
        call_return_ms: u64,
        runtime_drop_ms: Option<u64>,
        outcome: Option<CommandOutcome>,
        cancelled: bool,
        holder_survived_runtime: bool,
    }

    fn assert_call_returned_promptly(start: Instant) {
        assert!(
            start.elapsed() < CALL_WALL_CLOCK_BOUND,
            "runner call exceeded its wall-clock bound: {:?}",
            start.elapsed()
        );
    }

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
        for _ in 0..256 {
            std::io::stdout().lock().write_all(&[b'x'; 4096]).unwrap();
            std::io::stderr().lock().write_all(&[b'y'; 4096]).unwrap();
        }
    }

    #[test]
    #[ignore]
    fn regression_helper_quick_success() {
        std::io::stdout()
            .lock()
            .write_all(b"quick stdout\n")
            .unwrap();
        std::io::stderr()
            .lock()
            .write_all(b"quick stderr\n")
            .unwrap();
    }

    #[tokio::test]
    async fn fast_success_reaches_eof_repeatedly() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(2));
        let args = helper_argv("regression_helper_quick_success");
        for _ in 0..16 {
            let outcome = runner.run(&args).await.unwrap();
            assert!(outcome.passed(), "fast child failed: {outcome:?}");
            assert!(outcome.stdout_excerpt.contains("quick stdout"));
            assert!(outcome.stderr_excerpt.contains("quick stderr"));
        }
    }

    #[tokio::test]
    async fn spawn_failure_does_not_poison_later_capture() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(2));
        let missing = vec![dir
            .path()
            .join("absent-fixture.exe")
            .to_string_lossy()
            .into_owned()];
        for _ in 0..16 {
            assert!(runner.run(&missing).await.is_err());
        }
        let outcome = runner
            .run(&helper_argv("regression_helper_quick_success"))
            .await
            .unwrap();
        assert!(outcome.passed());
    }

    #[test]
    #[ignore]
    fn regression_helper_hold_inherited_pipes() {
        // Disposable, self-terminating process. File handshakes prove that it
        // really inherited the pipes and survived cancellation/runtime drop.
        std::io::stdout()
            .lock()
            .write_all(b"holder stdout ready\n")
            .unwrap();
        std::io::stderr()
            .lock()
            .write_all(b"holder stderr ready\n")
            .unwrap();
        std::fs::write(HOLDER_READY, b"ready").unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(8) {
            if Path::new(HOLDER_CHECK).exists() {
                let _ = std::fs::write(HOLDER_ALIVE, b"alive");
            }
            if Path::new(HOLDER_RELEASE).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::write(HOLDER_DONE, b"done");
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
        wait_for_fixture_file(Path::new(HOLDER_READY));
    }

    fn wait_for_fixture_file(path: &Path) {
        let start = Instant::now();
        while !path.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "missing fixture marker: {path:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore]
    fn regression_helper_observe_runtime_shutdown() {
        observe_runtime_shutdown(false);
    }

    #[test]
    #[ignore]
    fn regression_helper_observe_cancelled_runtime() {
        observe_runtime_shutdown(true);
    }

    fn observe_runtime_shutdown(cancel: bool) {
        // A real helper process separates the runner result from the later
        // destruction of its Tokio runtime. The parent supplies an isolated
        // working directory and an independent process-exit backstop.
        let root = std::env::current_dir().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let runner = BoundedRunner::new(&root)
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(1));
        let args = helper_argv("regression_helper_exit_with_pipe_holder");
        let call_start = Instant::now();
        let outcome = if cancel {
            runtime.block_on(async {
                let call = runner.run(&args);
                tokio::pin!(call);
                let ready = async {
                    while !root.join(HOLDER_READY).exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    // Poll real pipe reads before cancellation.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                };
                tokio::select! {
                    result = &mut call => panic!("runner finished before cancellation: {result:?}"),
                    () = ready => {},
                }
                // The pinned runner and its readers are dropped here.
            });
            None
        } else {
            Some(runtime.block_on(runner.run(&args)).unwrap())
        };
        assert!(
            root.join(HOLDER_READY).exists(),
            "descendant must actually start"
        );
        assert!(
            !root.join(HOLDER_DONE).exists(),
            "descendant must still hold its pipes"
        );
        let mut probe = RuntimeProbe {
            call_return_ms: call_start.elapsed().as_millis() as u64,
            runtime_drop_ms: None,
            outcome,
            cancelled: cancel,
            holder_survived_runtime: false,
        };
        let record = root.join(PROCESS_PROBE_FILE);
        // This first record survives even if runtime destruction stalls.
        std::fs::write(&record, serde_json::to_vec(&probe).unwrap()).unwrap();
        let drop_start = Instant::now();
        drop(runtime);
        probe.runtime_drop_ms = Some(drop_start.elapsed().as_millis() as u64);
        std::fs::write(&record, serde_json::to_vec(&probe).unwrap()).unwrap();
        // Ask the live descendant to acknowledge after runtime destruction;
        // then release only this test's process without sending any signal.
        std::fs::write(root.join(HOLDER_CHECK), b"check").unwrap();
        wait_for_fixture_file(&root.join(HOLDER_ALIVE));
        probe.holder_survived_runtime = true;
        std::fs::write(&record, serde_json::to_vec(&probe).unwrap()).unwrap();
        std::fs::write(root.join(HOLDER_RELEASE), b"release").unwrap();
        wait_for_fixture_file(&root.join(HOLDER_DONE));
    }

    #[test]
    fn records_call_return_and_host_shutdown_separately() {
        assert_host_shutdown(false);
    }

    #[test]
    fn cancelled_runner_does_not_delay_host_shutdown() {
        assert_host_shutdown(true);
    }

    fn assert_host_shutdown(cancel: bool) {
        let dir = tempfile::tempdir().unwrap();
        let args = helper_argv(if cancel {
            "regression_helper_observe_cancelled_runtime"
        } else {
            "regression_helper_observe_runtime_shutdown"
        });
        let process_start = Instant::now();
        let mut child = std::process::Command::new(&args[0])
            .args(&args[1..])
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if process_start.elapsed() >= PROCESS_PROBE_BACKSTOP {
                let kill_result = child.kill();
                let reap_start = Instant::now();
                let reap_result = loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break format!("reaped: {status}"),
                        Err(error) => break format!("reap error: {error}"),
                        Ok(None) if reap_start.elapsed() >= PROCESS_PROBE_REAP_GRACE => {
                            break "reap grace expired".to_string();
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    }
                };
                let record = std::fs::read_to_string(dir.path().join(PROCESS_PROBE_FILE));
                panic!(
                    "finite runtime probe did not exit; kill: {kill_result:?}; \
                     {reap_result}; last observation: {record:?}"
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let record = std::fs::read_to_string(dir.path().join(PROCESS_PROBE_FILE));
        assert!(
            status.success(),
            "runtime probe failed: {status}; last observation: {record:?}"
        );
        let probe: RuntimeProbe =
            serde_json::from_slice(&std::fs::read(dir.path().join(PROCESS_PROBE_FILE)).unwrap())
                .unwrap();
        let process_ms = process_start.elapsed().as_millis();
        // Direct stderr preserves these bounded timing facts in normal CI
        // logs even when the test harness captures successful println output.
        writeln!(
            std::io::stderr().lock(),
            "runner runtime probe: cancelled={cancel}, return={}ms, runtime drop={:?}ms, host exit={}ms",
            probe.call_return_ms,
            probe.runtime_drop_ms,
            process_ms
        )
        .unwrap();
        assert_eq!(probe.cancelled, cancel);
        assert!(probe.holder_survived_runtime);
        assert!(
            dir.path().join(HOLDER_DONE).exists(),
            "fixture must clean up"
        );
        if cancel {
            assert!(probe.outcome.is_none());
        } else {
            let outcome = probe.outcome.unwrap();
            assert!(outcome.timed_out);
            assert!(!outcome.passed());
            assert_eq!(outcome.gate, "ran");
            assert!(outcome.stdout_excerpt.contains("holder stdout ready"));
            assert!(outcome.stderr_excerpt.contains("holder stderr ready"));
        }
        assert!(probe.call_return_ms < CALL_WALL_CLOCK_BOUND.as_millis() as u64);
        assert!(probe.runtime_drop_ms.is_some());
        assert!(probe.runtime_drop_ms.unwrap() < 1000);
        assert!(
            process_start.elapsed() < Duration::from_secs(4),
            "inherited writers delayed host shutdown: {process_ms}ms"
        );
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
        // Independently collect the finite helper's complete output to verify
        // stream separation, prefix truncation and the exact evidence digest.
        let full = std::process::Command::new(&args[0])
            .args(&args[1..])
            .output()
            .unwrap();
        assert!(full.status.success());
        assert!(full.stdout.len() > MAX_OUTPUT_BYTES);
        assert!(full.stderr.len() > MAX_OUTPUT_BYTES);
        let mut digest = Sha256::new();
        digest.update(&full.stdout[..MAX_OUTPUT_BYTES]);
        digest.update(&full.stderr[..MAX_OUTPUT_BYTES]);
        assert_eq!(outcome.output_digest, hex::encode(digest.finalize()));
        assert_eq!(
            outcome.stdout_excerpt,
            safe_truncate(&String::from_utf8_lossy(&full.stdout), 2000)
        );
        assert_eq!(outcome.stderr_excerpt, "y".repeat(2000));
    }

    #[tokio::test]
    async fn inherited_pipe_reader_cannot_outlive_command_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BoundedRunner::new(dir.path())
            .with_approval_commands(true)
            .with_default_timeout(Duration::from_secs(1));
        let args = helper_argv("regression_helper_exit_with_pipe_holder");
        let call_start = Instant::now();
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("readers must not hang after the direct child exits")
            .expect("controlled helper should spawn");
        assert_call_returned_promptly(call_start);
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
        let call_start = Instant::now();
        let outcome = tokio::time::timeout(Duration::from_secs(3), runner.run(&args))
            .await
            .expect("capture must honor the earlier overall run deadline")
            .expect("controlled helper should spawn");
        assert_call_returned_promptly(call_start);
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

#[cfg(all(test, windows))]
mod windows_pipe_regressions {
    use super::*;
    use std::io::Write;

    fn pipe_name() -> String {
        format!(r"\\.\pipe\contribai-test-{}", uuid::Uuid::new_v4())
    }

    async fn assert_pipe_instance_released(name: &str) {
        // Dropping a pipe cancels its overlapped I/O, but the completion
        // callback can briefly retain its handle. Give the reactor a chance
        // to finish that work; a leaked instance must still fail within 1 s.
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match windows_output_pipe_named(name).await {
                    Ok((reader, writer)) => {
                        drop(writer);
                        drop(reader);
                        break;
                    }
                    Err(error) if error.raw_os_error() == Some(231) => {
                        // ERROR_PIPE_BUSY: the old instance is still closing.
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("reopen released pipe: {error}"),
                }
            }
        })
        .await
        .expect("pipe cleanup must release its name within one second");
    }

    #[tokio::test]
    async fn immediate_writer_close_is_clean_eof() {
        for bytes in [b"".as_slice(), b"short output".as_slice()] {
            for _ in 0..16 {
                let (reader, mut writer) = windows_output_pipe_named(&pipe_name()).await.unwrap();
                writer.write_all(bytes).unwrap();
                drop(writer);
                let mut captured = Vec::new();
                tokio::time::timeout(Duration::from_secs(1), drain_output(reader, &mut captured))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(captured, bytes);
            }
        }
    }

    #[tokio::test]
    async fn connected_pipe_rejects_extra_readers_writers_and_servers() {
        let name = pipe_name();
        let (reader, writer) = windows_output_pipe_named(&name).await.unwrap();
        assert!(std::fs::OpenOptions::new().read(true).open(&name).is_err());
        assert!(std::fs::OpenOptions::new().write(true).open(&name).is_err());
        assert!(windows_output_pipe_named(&name).await.is_err());
        drop(writer);
        drop(reader);
    }

    #[tokio::test]
    async fn failed_spawn_releases_configured_pipe_handles() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..16 {
            let name = pipe_name();
            let (reader, writer) = windows_output_pipe_named(&name).await.unwrap();
            let mut command = Command::new(dir.path().join("absent-fixture.exe"));
            command.stdout(writer).stderr(std::process::Stdio::null());
            assert!(command.spawn().is_err());
            drop(command);
            let mut captured = Vec::new();
            tokio::time::timeout(Duration::from_secs(1), drain_output(reader, &mut captured))
                .await
                .unwrap()
                .unwrap();
            assert!(captured.is_empty());
            assert_pipe_instance_released(&name).await;
        }
    }

    #[tokio::test]
    async fn cancelled_reads_release_pipe_instances() {
        for _ in 0..16 {
            let name = pipe_name();
            let (reader, mut writer) = windows_output_pipe_named(&name).await.unwrap();
            let mut captured = Vec::new();
            assert!(tokio::time::timeout(
                Duration::from_millis(10),
                drain_output(reader, &mut captured)
            )
            .await
            .is_err());
            // Cancellation owns and drops the OS reader while the writer
            // stays alive. Yield for completion of the cancelled OS read.
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if writer.write_all(b"probe").is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            drop(writer);
            // The same unique name can be reserved again only after the old
            // server and outstanding operation release their handles.
            assert_pipe_instance_released(&name).await;
        }
    }
}
