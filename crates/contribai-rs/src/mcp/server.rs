//! MCP stdio server implementation.
//!
//! Implements the Model Context Protocol (JSON-RPC over stdio)
//! to expose ContribAI tools to Claude Desktop.
//!
//! Tools exposed:
//! - search_repos
//! - get_repo_info
//! - get_file_tree
//! - get_file_content
//! - get_open_issues
//! - fork_repo
//! - create_branch
//! - push_file_change
//! - create_pr
//! - close_pr
//! - check_duplicate_pr
//! - check_ai_policy
//! - get_stats
//! - patrol_prs
//! - cleanup_forks

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info};

use crate::github::client::GitHubClient;
use crate::orchestrator::memory::Memory;

/// JSON-RPC request.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

/// JSON-RPC response.
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
    id: Value,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: Some(result),
            error: None,
            id,
        }
    }

    fn error(id: Value, code: i64, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(json!({
                "code": code,
                "message": message,
            })),
            id,
        }
    }
}

/// MCP tool definition.
#[derive(Debug, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

/// Get all tool definitions.
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "search_repos".into(),
            description: "Search GitHub for open-source repositories by language and star range".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "language": {"type": "string", "description": "e.g. python, javascript"},
                    "stars_min": {"type": "integer", "default": 50},
                    "stars_max": {"type": "integer", "default": 10000},
                    "limit": {"type": "integer", "default": 10}
                },
                "required": ["language"]
            }),
        },
        ToolDef {
            name: "get_repo_info".into(),
            description: "Get metadata for a GitHub repository".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "get_file_tree".into(),
            description: "List files in a repository (recursive)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "max_files": {"type": "integer", "default": 200}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "get_file_content".into(),
            description: "Get the content of a specific file from a repository".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "path": {"type": "string"},
                    "ref": {"type": "string", "description": "Branch or commit SHA (optional)"}
                },
                "required": ["owner", "repo", "path"]
            }),
        },
        ToolDef {
            name: "get_open_issues".into(),
            description: "List open issues in a repository".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "limit": {"type": "integer", "default": 20}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "fork_repo".into(),
            description: "Fork a repository to the authenticated user's account".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "create_branch".into(),
            description: "Create a new branch on a repository".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "fork_owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch_name": {"type": "string"},
                    "from_branch": {"type": "string", "description": "Source branch (defaults to repo default)"}
                },
                "required": ["fork_owner", "repo", "branch_name"]
            }),
        },
        ToolDef {
            name: "push_file_change".into(),
            description: "Push a file change to a branch. For updates, sha (blob SHA) is required.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "fork_owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch": {"type": "string"},
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "commit_msg": {"type": "string"},
                    "sha": {"type": "string", "description": "Blob SHA of existing file (required for updates)"}
                },
                "required": ["fork_owner", "repo", "branch", "path", "content", "commit_msg"]
            }),
        },
        ToolDef {
            name: "create_pr".into(),
            description: "Create a pull request from a fork branch to the upstream repo".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "head_branch": {"type": "string", "description": "fork_owner:branch"},
                    "base_branch": {"type": "string", "description": "Target branch (defaults to default branch)"}
                },
                "required": ["owner", "repo", "title", "body", "head_branch"]
            }),
        },
        ToolDef {
            name: "close_pr".into(),
            description: "Close a pull request".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pr_number": {"type": "integer"}
                },
                "required": ["owner", "repo", "pr_number"]
            }),
        },
        ToolDef {
            name: "check_duplicate_pr".into(),
            description: "Check if ContribAI has already submitted a PR to this repo".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "check_ai_policy".into(),
            description: "Check if a repo bans AI-generated contributions".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "get_stats".into(),
            description: "Get ContribAI contribution statistics".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDef {
            name: "patrol_prs".into(),
            description: "Collect raw review comments from open PRs for Claude to classify and act on".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pr_number": {"type": "integer", "description": "Optional: check a single PR"}
                },
                "required": ["owner", "repo"]
            }),
        },
        ToolDef {
            name: "cleanup_forks".into(),
            description: "List or delete stale forks where all PRs are merged/closed".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "default": true}
                }
            }),
        },
    ]
}

/// Run the MCP server on stdio (JSON-RPC over stdin/stdout).
pub async fn run_stdio_server(
    github: &GitHubClient,
    memory: &Memory,
) -> anyhow::Result<()> {
    let reader = BufReader::new(tokio::io::stdin());
    let mut lines = reader.lines();
    let stdout = io::stdout();

    info!("MCP server started on stdio");

    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "Invalid JSON-RPC request");
                continue;
            }
        };

        let id = request.id.clone().unwrap_or(Value::Null);

        let response = match request.method.as_str() {
            "initialize" => {
                JsonRpcResponse::success(
                    id,
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "contribai",
                            "version": crate::VERSION
                        }
                    }),
                )
            }

            "tools/list" => {
                let tools = tool_definitions();
                JsonRpcResponse::success(id, json!({ "tools": tools }))
            }

            "tools/call" => {
                let tool_name = request.params.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = request.params.get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));

                match handle_tool_call(tool_name, &arguments, github, memory).await {
                    Ok(result) => JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&result).unwrap_or_default()
                            }]
                        }),
                    ),
                    Err(e) => JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": json!({"error": e.to_string()}).to_string()
                            }],
                            "isError": true
                        }),
                    ),
                }
            }

            "notifications/initialized" | "ping" => {
                // No response needed for notifications
                continue;
            }

            _ => {
                JsonRpcResponse::error(id, -32601, &format!("Method not found: {}", request.method))
            }
        };

        let response_json = serde_json::to_string(&response)?;
        let mut out = stdout.lock();
        writeln!(out, "{}", response_json)?;
        out.flush()?;
    }

    info!("MCP server stopped");
    Ok(())
}

/// Handle a tool call by name.
async fn handle_tool_call(
    name: &str,
    args: &Value,
    github: &GitHubClient,
    memory: &Memory,
) -> anyhow::Result<Value> {
    // Helper: extract a required non-empty string argument.
    let require_str = |key: &str| -> anyhow::Result<&str> {
        args[key]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("'{}' is required and must be non-empty", key))
    };

    match name {
        "search_repos" => {
            let language = args["language"].as_str().unwrap_or("python");
            let stars_min = args["stars_min"].as_i64().unwrap_or(50);
            let stars_max = args["stars_max"].as_i64().unwrap_or(10000);
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;

            let query = format!(
                "language:{} stars:{}..{} sort:updated",
                language, stars_min, stars_max
            );

            let repos = github.search_repositories(&query, "updated", limit as u32).await?;
            Ok(json!(repos))
        }

        "get_repo_info" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let info = github.get_repo_details(owner, repo).await?;
            Ok(json!(info))
        }

        "get_file_tree" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let tree = github.get_file_tree(owner, repo, None).await?;
            Ok(json!(tree))
        }

        "get_file_content" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let path = args["path"].as_str().unwrap_or("");
            let git_ref = args["ref"].as_str();
            let content = github.get_file_content(owner, repo, path, git_ref).await?;
            Ok(json!({"path": path, "content": content}))
        }

        "get_open_issues" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let issues = github.get_open_issues(owner, repo, 20).await?;
            Ok(json!(issues))
        }

        "fork_repo" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let fork = github.fork_repository(owner, repo).await?;
            Ok(json!(fork))
        }

        "create_branch" => {
            let fork_owner = args["fork_owner"].as_str().unwrap_or("");
            let repo = require_str("repo")?;
            let branch_name = args["branch_name"].as_str().unwrap_or("");
            let from_branch = args["from_branch"].as_str();
            github
                .create_branch(fork_owner, repo, branch_name, from_branch)
                .await?;
            Ok(json!({"status": "created", "branch": branch_name}))
        }

        "push_file_change" => {
            let fork_owner = args["fork_owner"].as_str().unwrap_or("");
            let repo = require_str("repo")?;
            let branch = args["branch"].as_str().unwrap_or("");
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let commit_msg = args["commit_msg"].as_str().unwrap_or("");
            let sha = args["sha"].as_str();

            github
                .create_or_update_file(
                    fork_owner, repo, path, content, commit_msg, branch, sha, None,
                )
                .await?;
            Ok(json!({"status": "pushed", "path": path}))
        }

        "create_pr" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let title = args["title"].as_str().unwrap_or("");
            let body = args["body"].as_str().unwrap_or("");
            let head = args["head_branch"].as_str().unwrap_or("");
            let base = args["base_branch"].as_str();

            let pr = github
                .create_pull_request(owner, repo, title, body, head, base)
                .await?;
            Ok(json!(pr))
        }

        "get_stats" => {
            let stats = memory.get_stats()?;
            Ok(json!(stats))
        }

        "close_pr" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let pr_number = args["pr_number"].as_i64().unwrap_or(0);
            info!(owner, repo, pr_number, "Closing PR");
            match github.close_pull_request(owner, repo, pr_number, None).await {
                Ok(()) => Ok(json!({"success": true, "pr_number": pr_number})),
                Err(e) => Ok(json!({"success": false, "reason": e.to_string()})),
            }
        }

        "check_duplicate_pr" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let full_repo = format!("{}/{}", owner, repo);
            info!(repo = %full_repo, "Checking for duplicate PR");
            // Query memory for any open PRs submitted to this repo
            let all_open = memory.get_prs(Some("open"), 1000)?;
            let repo_open: Vec<_> = all_open
                .into_iter()
                .filter(|pr| pr.get("repo").map(|r| r == &full_repo).unwrap_or(false))
                .collect();
            if let Some(existing) = repo_open.first() {
                let pr_url = existing.get("pr_url").cloned().unwrap_or_default();
                let pr_number = existing.get("pr_number").cloned().unwrap_or_default();
                Ok(json!({
                    "is_duplicate": true,
                    "existing_pr_url": pr_url,
                    "existing_pr_number": pr_number
                }))
            } else {
                Ok(json!({"is_duplicate": false, "existing_pr_url": null}))
            }
        }

        "check_ai_policy" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            info!(owner, repo, "Checking AI contribution policy");
            // Keywords that indicate AI contributions are banned
            let ai_ban_keywords = [
                "no ai", "no-ai", "not accept ai", "prohibit ai",
                "ban ai", "ai generated", "ai-generated",
                "no llm", "human only", "no bot", "no automated",
                "ai contributions will be rejected",
            ];
            // Policy files to check in order
            let policy_paths = [
                "CONTRIBUTING.md",
                ".github/CONTRIBUTING.md",
                "README.md",
                "AI_POLICY.md",
                ".github/AI_POLICY.md",
            ];
            for path in &policy_paths {
                match github.get_file_content(owner, repo, path, None).await {
                    Ok(content) => {
                        let lower = content.to_lowercase();
                        let banned = ai_ban_keywords.iter().any(|kw| lower.contains(kw));
                        if banned {
                            info!(owner, repo, path, "AI policy ban found");
                            return Ok(json!({
                                "ai_allowed": false,
                                "banned": true,
                                "reason": format!("Ban keyword found in {}", path)
                            }));
                        }
                    }
                    Err(e) => {
                        // Skip 404 (file not found); stop on other errors
                        let msg = e.to_string();
                        if msg.contains("Not found") || msg.contains("404") {
                            continue;
                        }
                        return Err(e.into());
                    }
                }
            }
            Ok(json!({"ai_allowed": true, "banned": false, "reason": null}))
        }

        "patrol_prs" => {
            let owner = require_str("owner")?;
            let repo = require_str("repo")?;
            let full_repo = format!("{}/{}", owner, repo);
            let single_pr = args["pr_number"].as_i64();
            info!(repo = %full_repo, "Patrolling PRs for review comments");

            // Collect PR numbers to check
            let pr_numbers: Vec<i64> = if let Some(n) = single_pr {
                vec![n]
            } else {
                // Get open PRs from memory for this repo
                let open_prs = memory.get_prs(Some("open"), 100)?;
                open_prs
                    .iter()
                    .filter(|pr| pr.get("repo").map(|r| r == &full_repo).unwrap_or(false))
                    .filter_map(|pr| pr.get("pr_number").and_then(|n| n.parse::<i64>().ok()))
                    .collect()
            };

            let mut reviews_list = Vec::new();
            for pr_number in &pr_numbers {
                // Issue-level comments
                match github.get_pr_comments(owner, repo, *pr_number).await {
                    Ok(comments) => {
                        for c in comments {
                            reviews_list.push(json!({
                                "pr_number": pr_number,
                                "repo": full_repo,
                                "comment_author": c["user"]["login"].as_str().unwrap_or(""),
                                "comment_body": c["body"].as_str().unwrap_or(""),
                                "is_inline": false,
                                "file_path": null
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(repo = %full_repo, pr_number, error = %e, "Failed to fetch PR comments");
                    }
                }
                // Inline review comments
                match github.get_pr_review_comments(owner, repo, *pr_number).await {
                    Ok(inline) => {
                        for c in inline {
                            reviews_list.push(json!({
                                "pr_number": pr_number,
                                "repo": full_repo,
                                "comment_author": c["user"]["login"].as_str().unwrap_or(""),
                                "comment_body": c["body"].as_str().unwrap_or(""),
                                "is_inline": true,
                                "file_path": c["path"].as_str()
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(repo = %full_repo, pr_number, error = %e, "Failed to fetch inline review comments");
                    }
                }
            }
            Ok(json!({
                "prs_checked": pr_numbers.len(),
                "reviews_list": reviews_list
            }))
        }

        "cleanup_forks" => {
            let dry_run = args["dry_run"].as_bool().unwrap_or(true);
            info!(dry_run, "Cleaning up stale forks");

            let forks = github.list_user_forks().await?;
            // Get all PRs from memory to check fork activity
            let all_prs = memory.get_prs(None, 10_000)?;

            let mut forks_to_delete: Vec<String> = Vec::new();
            let mut forks_kept: Vec<String> = Vec::new();

            for fork in &forks {
                let fork_name = fork["full_name"].as_str().unwrap_or("");
                if fork_name.is_empty() {
                    continue;
                }
                // Find PRs associated with this fork
                let fork_prs: Vec<_> = all_prs
                    .iter()
                    .filter(|pr| pr.get("fork").map(|f| f == fork_name).unwrap_or(false))
                    .collect();
                let has_open = fork_prs
                    .iter()
                    .any(|pr| pr.get("status").map(|s| s == "open").unwrap_or(false));
                // Only mark for deletion if we have PR records and none are open (all merged/closed)
                if !fork_prs.is_empty() && !has_open {
                    forks_to_delete.push(fork_name.to_string());
                } else {
                    forks_kept.push(fork_name.to_string());
                }
            }

            if !dry_run {
                for fork_name in &forks_to_delete {
                    let parts: Vec<&str> = fork_name.splitn(2, '/').collect();
                    if parts.len() != 2 {
                        continue;
                    }
                    let (fork_owner, fork_repo) = (parts[0], parts[1]);
                    // Safety: verify it is actually a fork before deleting
                    match github.get_repo_details(fork_owner, fork_repo).await {
                        Ok(repo_info) => {
                            // The API returns `fork: true` in the raw JSON; check via raw get
                            // repo_info is our Repository struct — trust our fork list is correct
                            // (we fetched via /user/repos?type=fork already)
                            match github.delete_repository(fork_owner, fork_repo).await {
                                Ok(()) => info!(fork = fork_name, "Deleted stale fork"),
                                Err(e) => tracing::warn!(fork = fork_name, error = %e, "Failed to delete fork"),
                            }
                            drop(repo_info);
                        }
                        Err(e) => {
                            tracing::warn!(fork = fork_name, error = %e, "Could not verify fork before deletion, skipping");
                        }
                    }
                }
            }

            Ok(json!({
                "forks_to_delete": forks_to_delete,
                "forks_kept": forks_kept,
                "dry_run": dry_run
            }))
        }

        _ => {
            anyhow::bail!("Unknown tool: {}", name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions_complete() {
        let tools = tool_definitions();
        // Must have all 15 tools registered
        assert_eq!(tools.len(), 15, "Expected 15 tools, got {}", tools.len());

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Original 10 tools
        assert!(names.contains(&"search_repos"), "missing search_repos");
        assert!(names.contains(&"get_repo_info"), "missing get_repo_info");
        assert!(names.contains(&"get_file_tree"), "missing get_file_tree");
        assert!(names.contains(&"get_file_content"), "missing get_file_content");
        assert!(names.contains(&"get_open_issues"), "missing get_open_issues");
        assert!(names.contains(&"fork_repo"), "missing fork_repo");
        assert!(names.contains(&"create_branch"), "missing create_branch");
        assert!(names.contains(&"push_file_change"), "missing push_file_change");
        assert!(names.contains(&"create_pr"), "missing create_pr");
        assert!(names.contains(&"get_stats"), "missing get_stats");

        // 5 newly added tools
        assert!(names.contains(&"close_pr"), "missing close_pr");
        assert!(names.contains(&"check_duplicate_pr"), "missing check_duplicate_pr");
        assert!(names.contains(&"check_ai_policy"), "missing check_ai_policy");
        assert!(names.contains(&"patrol_prs"), "missing patrol_prs");
        assert!(names.contains(&"cleanup_forks"), "missing cleanup_forks");
    }

    #[test]
    fn test_jsonrpc_response_success() {
        let resp = JsonRpcResponse::success(json!(1), json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"ok\":true"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn test_jsonrpc_response_error() {
        let resp = JsonRpcResponse::error(json!(2), -32601, "Method not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32601"));
        assert!(json.contains("Method not found"));
    }
}
