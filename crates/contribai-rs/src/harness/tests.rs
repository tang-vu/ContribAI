use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;

use super::store::HarnessStore;
use super::*;

struct Scripted {
    responses: Mutex<VecDeque<String>>,
    calls: AtomicUsize,
    hang: bool,
    fail: bool,
}

impl Scripted {
    fn new(responses: &[&str]) -> Self {
        Self {
            responses: Mutex::new(responses.iter().map(|s| (*s).into()).collect()),
            calls: AtomicUsize::new(0),
            hang: false,
            fail: false,
        }
    }
}

#[async_trait]
impl LlmProvider for Scripted {
    async fn complete(
        &self,
        _: &str,
        _: Option<&str>,
        _: Option<f64>,
        _: Option<u32>,
    ) -> Result<String> {
        panic!("chat only")
    }
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        _: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(system.unwrap().contains("untrusted DATA"));
        assert!(max_tokens.unwrap() > 0);
        assert_eq!(messages.len() % 2, 1);
        if self.hang {
            std::future::pending::<()>().await;
        }
        if self.fail {
            return Err(ContribError::Llm("secret-provider-body".into()));
        }
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| r#"{"action":"list_files"}"#.into()))
    }
}

fn snapshot() -> HarnessSnapshot {
    HarnessSnapshot::from_files(BTreeMap::from([(
        "README.md".into(),
        "private-file-content".into(),
    )]))
    .unwrap()
}

async fn execute(
    provider: &Scripted,
    limits: HarnessLimits,
    cancel: &HarnessCancellation,
) -> (HarnessOutcome, HarnessStore) {
    let store = HarnessStore::in_memory().unwrap();
    let snapshot = snapshot();
    let outcome = Harness {
        provider,
        provider_name: "test",
        model_name: "fixture",
        snapshot: &snapshot,
        journal: &store,
        limits,
    }
    .run("private-task", cancel)
    .await
    .unwrap();
    (outcome, store)
}

#[tokio::test]
async fn tools_complete_with_ordered_private_history() {
    let provider = Scripted::new(&[
        r#"{"action":"list_files"}"#,
        r#"{"action":"read_file","path":"README.md"}"#,
        r#"{"action":"finish","answer":"private-answer"}"#,
    ]);
    let (outcome, store) = execute(
        &provider,
        HarnessLimits::default(),
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(outcome.run.status, HarnessStatus::Completed);
    assert_eq!(outcome.run.model_calls, 3);
    assert_eq!(outcome.run.tool_calls, 2);
    assert_eq!(outcome.run.reserved_output_tokens, 6144);
    assert_eq!(outcome.answer.as_deref(), Some("private-answer"));
    assert!(!outcome.run.external_writes_enabled);
    let (saved, events) = store.inspect(&outcome.run.id).unwrap().unwrap();
    assert_eq!(
        saved.answer_sha256.as_deref(),
        Some(fingerprint("private-answer").as_str())
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.sequence as usize, index + 1);
    }
    let serialized = serde_json::to_string(&(saved, events)).unwrap();
    for secret in ["private-file-content", "private-task", "private-answer"] {
        assert!(!serialized.contains(secret));
    }
    assert_eq!(store.list(10).unwrap().len(), 1);
    assert!(store.inspect("unknown").unwrap().is_none());
}

#[tokio::test]
async fn model_budget_stops_before_an_extra_call() {
    let provider = Scripted::new(&[]);
    let (outcome, _) = execute(
        &provider,
        HarnessLimits {
            max_model_calls: 2,
            ..Default::default()
        },
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(outcome.run.status, HarnessStatus::BudgetExceeded);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn token_reservation_never_exceeds_budget() {
    let provider = Scripted::new(&[]);
    let (outcome, _) = execute(
        &provider,
        HarnessLimits {
            max_reserved_output_tokens: 2049,
            ..Default::default()
        },
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(outcome.run.reserved_output_tokens, 2049);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(outcome.run.status, HarnessStatus::BudgetExceeded);
}

#[tokio::test]
async fn tool_budget_does_not_execute_an_extra_tool() {
    let provider = Scripted::new(&[]);
    let (outcome, store) = execute(
        &provider,
        HarnessLimits {
            max_tool_calls: 1,
            ..Default::default()
        },
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(outcome.run.tool_calls, 1);
    assert_eq!(outcome.run.reason.as_deref(), Some("tool_budget_exceeded"));
    let (_, events) = store.inspect(&outcome.run.id).unwrap().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, HarnessEventKind::ToolRequested { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn reject_unknown_malformed_extra_fields_and_empty_answers() {
    for action in [
        r#"{"action":"shell","command":"rm -rf /"}"#,
        r#"{"action":"list_files","command":"echo unexpected"}"#,
        "not json",
        r#"{"action":"finish","answer":" "}"#,
    ] {
        let provider = Scripted::new(&[action]);
        let (outcome, _) = execute(
            &provider,
            HarnessLimits::default(),
            &HarnessCancellation::default(),
        )
        .await;
        assert_eq!(outcome.run.status, HarnessStatus::Blocked);
        assert_eq!(outcome.run.tool_calls, 0);
    }
}

#[tokio::test]
async fn reject_read_outside_selected_snapshot_without_recording_hostile_path() {
    for path in ["../secret", "/etc/passwd", ".env", "missing.rs"] {
        let action = serde_json::json!({"action":"read_file","path":path}).to_string();
        let provider = Scripted::new(&[&action]);
        let (outcome, store) = execute(
            &provider,
            HarnessLimits::default(),
            &HarnessCancellation::default(),
        )
        .await;
        assert_eq!(outcome.run.reason.as_deref(), Some("file_outside_snapshot"));
        assert_eq!(outcome.run.tool_calls, 0);
        assert!(
            !serde_json::to_string(&store.inspect(&outcome.run.id).unwrap())
                .unwrap()
                .contains(path)
        );
    }
}

#[tokio::test]
async fn cancellation_before_start_issues_no_model_call() {
    let provider = Scripted::new(&[]);
    let cancel = HarnessCancellation::default();
    cancel.cancel();
    let (outcome, _) = execute(&provider, HarnessLimits::default(), &cancel).await;
    assert_eq!(outcome.run.status, HarnessStatus::Cancelled);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_interrupts_inflight_provider() {
    let mut provider = Scripted::new(&[]);
    provider.hang = true;
    let cancel = HarnessCancellation::default();
    let trigger = cancel.clone();
    let cancelling = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.cancel();
    };
    let ((outcome, _), _) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            execute(&provider, HarnessLimits::default(), &cancel),
            cancelling
        )
    })
    .await
    .unwrap();
    assert_eq!(outcome.run.status, HarnessStatus::Cancelled);
}

#[tokio::test]
async fn deadline_interrupts_inflight_provider() {
    let mut provider = Scripted::new(&[]);
    provider.hang = true;
    let (outcome, _) = tokio::time::timeout(
        Duration::from_secs(2),
        execute(
            &provider,
            HarnessLimits {
                max_duration_ms: 20,
                ..Default::default()
            },
            &HarnessCancellation::default(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(outcome.run.status, HarnessStatus::TimedOut);
}

#[tokio::test]
async fn provider_error_is_recorded_without_secrets() {
    let mut provider = Scripted::new(&[]);
    provider.fail = true;
    let (outcome, store) = execute(
        &provider,
        HarnessLimits::default(),
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(outcome.run.reason.as_deref(), Some("provider_error"));
    assert!(
        !serde_json::to_string(&store.inspect(&outcome.run.id).unwrap())
            .unwrap()
            .contains("secret-provider-body")
    );
}

#[tokio::test]
async fn response_size_is_checked_before_parsing() {
    let response = "x".repeat(1025);
    let provider = Scripted::new(&[&response]);
    let (outcome, _) = execute(
        &provider,
        HarnessLimits {
            max_response_bytes: 1024,
            ..Default::default()
        },
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(
        outcome.run.reason.as_deref(),
        Some("response_budget_exceeded")
    );
}

#[tokio::test]
async fn context_size_is_checked_before_sending_more_data() {
    let provider = Scripted::new(&[]);
    let (outcome, _) = execute(
        &provider,
        HarnessLimits {
            max_context_bytes: 1024,
            max_model_calls: 64,
            max_reserved_output_tokens: 131_072,
            ..Default::default()
        },
        &HarnessCancellation::default(),
    )
    .await;
    assert_eq!(
        outcome.run.reason.as_deref(),
        Some("context_budget_exceeded")
    );
}

#[tokio::test]
async fn journal_failure_stops_before_model_request() {
    struct Broken;
    impl HarnessJournal for Broken {
        fn record(&self, _: &HarnessRun, _: &HarnessEvent) -> Result<()> {
            Err(ContribError::Database("unavailable".into()))
        }
    }
    let provider = Scripted::new(&[]);
    let snapshot = snapshot();
    assert!(Harness {
        provider: &provider,
        provider_name: "test",
        model_name: "test",
        snapshot: &snapshot,
        journal: &Broken,
        limits: HarnessLimits::default()
    }
    .run("task", &HarnessCancellation::default())
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn persisted_receipt_survives_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("history.db");
    let store = HarnessStore::open(&path).unwrap();
    let provider = Scripted::new(&[r#"{"action":"finish","answer":"done"}"#]);
    let snapshot = snapshot();
    let outcome = Harness {
        provider: &provider,
        provider_name: "test",
        model_name: "test",
        snapshot: &snapshot,
        journal: &store,
        limits: HarnessLimits::default(),
    }
    .run("task", &HarnessCancellation::default())
    .await
    .unwrap();
    drop(store);
    let store = HarnessStore::open(&path).unwrap();
    let (saved, events) = store.inspect(&outcome.run.id).unwrap().unwrap();
    assert_eq!(saved.status, HarnessStatus::Completed);
    assert!(matches!(
        events.last().unwrap().kind,
        HarnessEventKind::Finished {
            status: HarnessStatus::Completed,
            ..
        }
    ));
}

#[test]
fn invalid_budgets_fail_closed() {
    for limits in [
        HarnessLimits {
            max_model_calls: 0,
            ..Default::default()
        },
        HarnessLimits {
            max_duration_ms: u64::MAX,
            ..Default::default()
        },
        HarnessLimits {
            max_reserved_output_tokens: 0,
            ..Default::default()
        },
    ] {
        assert!(limits.validate().is_err());
    }
}

#[tokio::test]
async fn cancellation_during_journal_write_cannot_report_completed() {
    struct CancellingJournal {
        inner: HarnessStore,
        cancel: HarnessCancellation,
    }
    impl HarnessJournal for CancellingJournal {
        fn record(&self, run: &HarnessRun, event: &HarnessEvent) -> Result<()> {
            self.inner.record(run, event)?;
            if matches!(event.kind, HarnessEventKind::ModelCompleted { .. }) {
                self.cancel.cancel();
            }
            Ok(())
        }
    }
    let cancel = HarnessCancellation::default();
    let journal = CancellingJournal {
        inner: HarnessStore::in_memory().unwrap(),
        cancel: cancel.clone(),
    };
    let provider = Scripted::new(&[r#"{"action":"finish","answer":"done"}"#]);
    let snapshot = snapshot();
    let outcome = Harness {
        provider: &provider,
        provider_name: "test",
        model_name: "test",
        snapshot: &snapshot,
        journal: &journal,
        limits: HarnessLimits::default(),
    }
    .run("task", &cancel)
    .await
    .unwrap();
    assert_eq!(outcome.run.status, HarnessStatus::Cancelled);
    assert!(outcome.answer.is_none());
}
