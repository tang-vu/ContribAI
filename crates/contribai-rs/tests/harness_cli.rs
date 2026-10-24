//! End-to-end CLI contract tests. HTTP fixtures exercise the real OpenAI adapter
//! without using credentials, paid APIs, or an actual model.

use serde_json::{json, Value};
use tokio::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_contribai"))
}

#[tokio::test]
async fn help_and_version_start_without_configuration() {
    for (args, expected) in [
        (vec!["--help"], "Usage: contribai"),
        (vec!["--version"], "contribai "),
        (vec!["version"], "contribai "),
        (vec!["harness", "--help"], "Usage: contribai harness"),
    ] {
        let result = command().args(args).output().await.unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains(expected));
    }
}

// Windows already exercises its small default main-thread stack in every CLI
// test. Leave extra headroom by using only 512 KiB on Linux, rather than hiding
// the problem with a larger linker stack or release-optimized executables.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn offline_demo_completes_with_half_megabyte_process_stack() {
    let temp = tempfile::tempdir().unwrap();
    let result = Command::new("sh")
        .args([
            "-c",
            "ulimit -s 512 || exit 1; exec \"$@\"",
            "small-stack-harness",
            env!("CARGO_BIN_EXE_contribai"),
            "harness",
            "--db",
        ])
        .arg(temp.path().join("harness.db"))
        .args(["demo", "--json"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let outcome: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(outcome["run"]["status"], "completed");
    assert_eq!(outcome["run"]["tool_calls"], 2);
}

#[tokio::test]
async fn offline_demo_and_history_roundtrip_without_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("harness.db");
    let result = command()
        .args(["--config", "does-not-exist.yaml", "harness", "--db"])
        .arg(&db)
        .args(["demo", "--json"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let outcome: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(outcome["run"]["provider"], "demo_fixture");
    assert_eq!(outcome["run"]["status"], "completed");
    assert_eq!(outcome["run"]["tool_calls"], 2);
    let id = outcome["run"]["id"].as_str().unwrap();
    let result = command()
        .args(["harness", "--db"])
        .arg(&db)
        .args(["inspect", id, "--json"])
        .output()
        .await
        .unwrap();
    assert!(result.status.success());
    let history: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(history["run"]["id"], id);
    assert_eq!(
        history["events"].as_array().unwrap().last().unwrap()["event"],
        "finished"
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains("A tiny read-only harness fixture."));
    let result = command()
        .args(["harness", "--db"])
        .arg(&db)
        .args(["runs", "--json"])
        .output()
        .await
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn live_run_requires_explicit_config_and_files_without_creating_history() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("absent.db");
    for (args, diagnostic) in [
        (vec!["run", "task"], "--file"),
        (
            vec!["run", "task", "--file", "README.md"],
            "harness run requires an explicit --config",
        ),
    ] {
        let result = command()
            .args(["harness", "--db"])
            .arg(&db)
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!db.exists());
    }
    let result = command()
        .args(["harness", "--db"])
        .arg(&db)
        .args(["runs", "--json"])
        .output()
        .await
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("harness journal does not exist"));
    assert!(!db.exists());
}

async fn fixture_config(temp: &tempfile::TempDir, server: &MockServer) -> std::path::PathBuf {
    let config = temp.path().join("provider.yaml");
    tokio::fs::write(&config, format!("llm:\n  provider: openai\n  model: local-test\n  api_key: fixture-not-a-real-key\n  base_url: {}\n", server.uri())).await.unwrap();
    config
}

#[tokio::test]
async fn configured_provider_executes_real_multi_turn_http_and_reads_selected_file() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(|request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["max_tokens"], 2048);
            let messages = body["messages"].as_array().unwrap();
            let content = if messages.len() == 2 {
                json!({"action":"read_file","path":"README.md"}).to_string()
            } else {
                assert!(messages.last().unwrap()["content"]
                    .as_str()
                    .unwrap()
                    .contains("selected-evidence"));
                json!({"action":"finish","answer":"The selected file contains selected-evidence."})
                    .to_string()
            };
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":content}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    tokio::fs::write(root.join("README.md"), "selected-evidence")
        .await
        .unwrap();
    tokio::fs::write(root.join("private.txt"), "must-not-read")
        .await
        .unwrap();
    let config = fixture_config(&temp, &server).await;
    let result = command()
        .arg("--config")
        .arg(&config)
        .args(["harness", "--db"])
        .arg(root.join("history.db"))
        .args(["run", "Summarize the selected file", "--workspace"])
        .arg(&root)
        .args(["--file", "README.md", "--json"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let outcome: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(outcome["run"]["provider"], "openai");
    assert_eq!(outcome["run"]["status"], "completed");
    assert_eq!(outcome["run"]["model_calls"], 2);
    assert_eq!(outcome["run"]["files"].as_object().unwrap().len(), 1);
    assert_eq!(
        tokio::fs::read_to_string(root.join("README.md"))
            .await
            .unwrap(),
        "selected-evidence"
    );
}

#[tokio::test]
async fn forbidden_model_action_is_persisted_and_exits_nonzero() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices":[{"message":{"content":"{\"action\":\"shell\",\"command\":\"touch forbidden\"}"}}]}))).expect(1).mount(&server).await;
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    tokio::fs::write(root.join("README.md"), "safe")
        .await
        .unwrap();
    let config = fixture_config(&temp, &server).await;
    let result = command()
        .arg("--config")
        .arg(config)
        .args(["harness", "--db"])
        .arg(root.join("history.db"))
        .args(["run", "task", "--workspace"])
        .arg(&root)
        .args(["--file", "README.md", "--json"])
        .output()
        .await
        .unwrap();
    assert!(!result.status.success());
    let outcome: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(outcome["run"]["status"], "blocked");
    assert_eq!(outcome["run"]["tool_calls"], 0);
    assert!(!root.join("forbidden").exists());
}

#[tokio::test]
async fn unsafe_allowlist_is_rejected_before_provider_contact() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let config = fixture_config(&temp, &server).await;
    let db = temp.path().join("history.db");
    let result = command()
        .arg("--config")
        .arg(config)
        .args(["harness", "--db"])
        .arg(&db)
        .args(["run", "task", "--workspace"])
        .arg(temp.path())
        .args(["--file", "../private.txt", "--json"])
        .output()
        .await
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("unsafe snapshot path"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!db.exists());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn ollama_receives_token_reservation_without_gcloud_or_github_resolution() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/chat")).respond_with(|request: &Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["options"]["num_predict"], 17);
        ResponseTemplate::new(200).set_body_json(json!({"message":{"content":"{\"action\":\"finish\",\"answer\":\"Local fixture answer\"}"}}))
    }).expect(1).mount(&server).await;
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    tokio::fs::write(root.join("README.md"), "safe")
        .await
        .unwrap();
    let config = root.join("provider.yaml");
    tokio::fs::write(
        &config,
        format!(
            "llm:\n  provider: ollama\n  model: local-fixture\n  base_url: {}\n",
            server.uri()
        ),
    )
    .await
    .unwrap();
    let result = command()
        .env("GOOGLE_CLOUD_PROJECT", "must-not-enable-vertex")
        .env("PATH", "")
        .arg("--config")
        .arg(config)
        .args(["harness", "--db"])
        .arg(root.join("history.db"))
        .args(["run", "task", "--workspace"])
        .arg(&root)
        .args(["--file", "README.md", "--token-budget", "17", "--json"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let outcome: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(outcome["run"]["reserved_output_tokens"], 17);
    assert_eq!(outcome["run"]["status"], "completed");
}
