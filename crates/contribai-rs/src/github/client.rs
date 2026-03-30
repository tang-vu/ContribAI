//! Async GitHub REST API client.
//!
//! Handles all GitHub REST API interactions: repo metadata,
//! file content, forking, branching, committing, and PR creation.
//! Direct port from Python `github/client.py` — httpx → reqwest.

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{info, warn};

use crate::core::error::{ContribError, Result};
use crate::core::models::{FileNode, Issue, Repository};

const GITHUB_API: &str = "https://api.github.com";

/// Async GitHub REST API client.
pub struct GitHubClient {
    client: Client,
    #[allow(dead_code)]
    token: String,
    rate_limit_buffer: u32,
}

impl GitHubClient {
    /// Create a new GitHub client with the given token.
    pub fn new(token: &str, rate_limit_buffer: u32) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|e| ContribError::GitHub(format!("Invalid token: {}", e)))?,
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("ContribAI/5.0"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ContribError::GitHub(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            token: token.to_string(),
            rate_limit_buffer,
        })
    }

    // ── Core HTTP ──────────────────────────────────────────────────────────

    /// Make an authenticated GitHub API request with error handling and retry.
    async fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        params: Option<&[(&str, &str)]>,
        json_body: Option<&Value>,
    ) -> Result<Value> {
        let full_url = if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", GITHUB_API, url)
        };

        let max_retries: u32 = 3;
        let mut last_error = None;

        for attempt in 1..=max_retries {
            let mut req = self.client.request(method.clone(), &full_url);

            if let Some(p) = params {
                req = req.query(p);
            }
            if let Some(body) = json_body {
                req = req.json(body);
            }

            let response = req
                .send()
                .await
                .map_err(|e| ContribError::GitHub(format!("HTTP error: {}", e)))?;

            let status = response.status();

            // Rate limit check
            if status == StatusCode::FORBIDDEN {
                let remaining = response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("?");
                let reset = response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown")
                    .to_string();

                if remaining == "0" {
                    return Err(ContribError::RateLimit { reset_at: reset });
                }

                let body = response.text().await.unwrap_or_default();
                return Err(ContribError::GitHub(format!("Forbidden: {}", body)));
            }

            if status == StatusCode::NOT_FOUND {
                return Err(ContribError::GitHub(format!("Not found: {}", url)));
            }

            // Retry on 5xx
            if status.is_server_error() {
                let body = response.text().await.unwrap_or_default();
                last_error = Some(ContribError::GitHub(format!(
                    "GitHub API error {}: {}",
                    status.as_u16(),
                    body
                )));
                if attempt < max_retries {
                    let wait = 2u64.pow(attempt);
                    warn!(
                        status = status.as_u16(),
                        %url,
                        attempt,
                        max_retries,
                        "GitHub server error, retrying in {}s",
                        wait
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(last_error.unwrap());
            }

            if status.is_client_error() {
                let body = response.text().await.unwrap_or_default();
                return Err(ContribError::GitHub(format!(
                    "GitHub API error {}: {}",
                    status.as_u16(),
                    body
                )));
            }

            // Success — parse JSON
            let body = response.text().await.unwrap_or_default();
            if body.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&body)
                .map_err(|e| ContribError::GitHub(format!("JSON parse error: {}", e)));
        }

        Err(last_error.unwrap_or_else(|| ContribError::GitHub("Unknown error".into())))
    }

    /// Make a raw request returning the Response (for diffs, etc.).
    async fn request_raw(
        &self,
        method: reqwest::Method,
        url: &str,
        extra_headers: Option<HeaderMap>,
    ) -> Result<Response> {
        let full_url = if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", GITHUB_API, url)
        };

        let mut req = self.client.request(method, &full_url);
        if let Some(h) = extra_headers {
            req = req.headers(h);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ContribError::GitHub(format!("HTTP error: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ContribError::GitHub(format!(
                "GitHub API error {}: {}",
                status.as_u16(),
                body
            )));
        }

        Ok(response)
    }

    async fn get(&self, url: &str) -> Result<Value> {
        self.request(reqwest::Method::GET, url, None, None).await
    }

    async fn get_with_params(&self, url: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.request(reqwest::Method::GET, url, Some(params), None)
            .await
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        self.request(reqwest::Method::POST, url, None, Some(body))
            .await
    }

    async fn put(&self, url: &str, body: &Value) -> Result<Value> {
        self.request(reqwest::Method::PUT, url, None, Some(body))
            .await
    }

    async fn patch(&self, url: &str, body: &Value) -> Result<Value> {
        self.request(reqwest::Method::PATCH, url, None, Some(body))
            .await
    }

    async fn delete(&self, url: &str) -> Result<Value> {
        self.request(reqwest::Method::DELETE, url, None, None).await
    }

    // ── Rate Limit ─────────────────────────────────────────────────────────

    /// Check current rate limit status.
    pub async fn check_rate_limit(&self) -> Result<RateLimitInfo> {
        let data = self.get("/rate_limit").await?;
        let core = &data["resources"]["core"];
        let info = RateLimitInfo {
            remaining: core["remaining"].as_u64().unwrap_or(0) as u32,
            limit: core["limit"].as_u64().unwrap_or(0) as u32,
            reset: core["reset"].as_u64().unwrap_or(0),
        };
        info!(
            remaining = info.remaining,
            limit = info.limit,
            "Rate limit status"
        );
        Ok(info)
    }

    /// Ensure we have enough API calls remaining.
    pub async fn ensure_rate_limit(&self) -> Result<()> {
        let info = self.check_rate_limit().await?;
        if info.remaining < self.rate_limit_buffer {
            return Err(ContribError::RateLimit {
                reset_at: info.reset.to_string(),
            });
        }
        Ok(())
    }

    // ── Repository Operations ──────────────────────────────────────────────

    /// Search GitHub repositories.
    pub async fn search_repositories(
        &self,
        query: &str,
        sort: &str,
        per_page: u32,
    ) -> Result<Vec<Repository>> {
        let pp = per_page.to_string();
        let data = self
            .get_with_params(
                "/search/repositories",
                &[("q", query), ("sort", sort), ("order", "desc"), ("per_page", &pp)],
            )
            .await?;

        let items = data["items"].as_array().cloned().unwrap_or_default();
        Ok(items.into_iter().map(|item| parse_repo(&item)).collect())
    }

    /// Get detailed repository information.
    pub async fn get_repo_details(&self, owner: &str, repo: &str) -> Result<Repository> {
        let data = self.get(&format!("/repos/{}/{}", owner, repo)).await?;
        Ok(parse_repo(&data))
    }

    /// Get the full file tree of a repository.
    pub async fn get_file_tree(
        &self,
        owner: &str,
        repo: &str,
        branch: Option<&str>,
    ) -> Result<Vec<FileNode>> {
        let branch = match branch {
            Some(b) => b.to_string(),
            None => {
                let details = self.get_repo_details(owner, repo).await?;
                details.default_branch
            }
        };

        let data = self
            .get_with_params(
                &format!("/repos/{}/{}/git/trees/{}", owner, repo, branch),
                &[("recursive", "1")],
            )
            .await?;

        let tree = data["tree"].as_array().cloned().unwrap_or_default();
        Ok(tree
            .into_iter()
            .map(|item| FileNode {
                path: item["path"].as_str().unwrap_or("").to_string(),
                node_type: item["type"].as_str().unwrap_or("blob").to_string(),
                size: item["size"].as_i64().unwrap_or(0),
                sha: item["sha"].as_str().unwrap_or("").to_string(),
            })
            .collect())
    }

    /// Get the content of a file from the repository.
    pub async fn get_file_content(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        ref_name: Option<&str>,
    ) -> Result<String> {
        let url = format!("/repos/{}/{}/contents/{}", owner, repo, path);
        let data = if let Some(r) = ref_name {
            self.get_with_params(&url, &[("ref", r)]).await?
        } else {
            self.get(&url).await?
        };

        if data["encoding"].as_str() == Some("base64") {
            let content = data["content"].as_str().unwrap_or("");
            let clean = content.replace('\n', "").replace('\r', "");
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&clean)
                .map_err(|e| ContribError::GitHub(format!("Base64 decode error: {}", e)))?;
            return String::from_utf8(decoded)
                .map_err(|e| ContribError::GitHub(format!("UTF-8 decode error: {}", e)));
        }

        Ok(data["content"].as_str().unwrap_or("").to_string())
    }

    /// Get file content and blob SHA (needed for updates).
    pub async fn get_file_content_with_sha(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        ref_name: Option<&str>,
    ) -> Result<(String, String)> {
        let url = format!("/repos/{}/{}/contents/{}", owner, repo, path);
        let data = if let Some(r) = ref_name {
            self.get_with_params(&url, &[("ref", r)]).await?
        } else {
            self.get(&url).await?
        };

        let content = if data["encoding"].as_str() == Some("base64") {
            let raw = data["content"].as_str().unwrap_or("");
            let clean = raw.replace('\n', "").replace('\r', "");
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&clean)
                .map_err(|e| ContribError::GitHub(format!("Base64 decode: {}", e)))?;
            String::from_utf8(decoded)
                .map_err(|e| ContribError::GitHub(format!("UTF-8 decode: {}", e)))?
        } else {
            data["content"].as_str().unwrap_or("").to_string()
        };

        let sha = data["sha"].as_str().unwrap_or("").to_string();
        Ok((content, sha))
    }

    /// Get open issues for a repository.
    pub async fn get_open_issues(
        &self,
        owner: &str,
        repo: &str,
        per_page: u32,
    ) -> Result<Vec<Issue>> {
        let pp = per_page.to_string();
        let data = self
            .get_with_params(
                &format!("/repos/{}/{}/issues", owner, repo),
                &[("state", "open"), ("per_page", &pp)],
            )
            .await?;

        let items = data.as_array().cloned().unwrap_or_default();
        Ok(items
            .into_iter()
            .filter(|item| item.get("pull_request").is_none())
            .map(|item| Issue {
                number: item["number"].as_i64().unwrap_or(0),
                title: item["title"].as_str().unwrap_or("").to_string(),
                body: item["body"].as_str().map(String::from),
                labels: item["labels"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|l| l["name"].as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                state: item["state"].as_str().unwrap_or("open").to_string(),
                created_at: None,
                html_url: item["html_url"].as_str().unwrap_or("").to_string(),
            })
            .collect())
    }

    /// Try to fetch CONTRIBUTING.md.
    pub async fn get_contributing_guide(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        for path in &[
            "CONTRIBUTING.md",
            "contributing.md",
            ".github/CONTRIBUTING.md",
        ] {
            match self.get_file_content(owner, repo, path, None).await {
                Ok(content) => return Ok(Some(content)),
                Err(_) => continue,
            }
        }
        Ok(None)
    }

    // ── Fork & Branch ──────────────────────────────────────────────────────

    /// Fork a repository to the authenticated user's account.
    pub async fn fork_repository(&self, owner: &str, repo: &str) -> Result<Repository> {
        let data = self
            .post(
                &format!("/repos/{}/{}/forks", owner, repo),
                &Value::Object(serde_json::Map::new()),
            )
            .await?;
        info!(
            fork = data["full_name"].as_str().unwrap_or("?"),
            "Forked {}/{}",
            owner,
            repo
        );
        Ok(parse_repo(&data))
    }

    /// Create a new branch from the default or specified branch.
    pub async fn create_branch(
        &self,
        owner: &str,
        repo: &str,
        branch_name: &str,
        from_branch: Option<&str>,
    ) -> Result<Value> {
        let from = match from_branch {
            Some(b) => b.to_string(),
            None => {
                let details = self.get_repo_details(owner, repo).await?;
                details.default_branch
            }
        };

        // Get SHA of source branch
        let ref_data = self
            .get(&format!(
                "/repos/{}/{}/git/ref/heads/{}",
                owner, repo, from
            ))
            .await?;
        let sha = ref_data["object"]["sha"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let data = self
            .post(
                &format!("/repos/{}/{}/git/refs", owner, repo),
                &serde_json::json!({
                    "ref": format!("refs/heads/{}", branch_name),
                    "sha": sha,
                }),
            )
            .await?;

        info!(branch = branch_name, "Created branch on {}/{}", owner, repo);
        Ok(data)
    }

    // ── Commit & PR ────────────────────────────────────────────────────────

    /// Create or update a file in the repository.
    pub async fn create_or_update_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        content: &str,
        message: &str,
        branch: &str,
        sha: Option<&str>,
        signoff: Option<&str>,
    ) -> Result<Value> {
        let mut msg = message.to_string();
        if let Some(signer) = signoff {
            if !msg.contains("Signed-off-by:") {
                msg = format!("{}\n\nSigned-off-by: {}", msg, signer);
            }
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let mut payload = serde_json::json!({
            "message": msg,
            "content": encoded,
            "branch": branch,
        });
        if let Some(s) = sha {
            payload["sha"] = Value::String(s.to_string());
        }

        self.put(
            &format!("/repos/{}/{}/contents/{}", owner, repo, path),
            &payload,
        )
        .await
    }

    /// Create a pull request.
    pub async fn create_pull_request(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
        head: &str,
        base: Option<&str>,
    ) -> Result<Value> {
        let base_branch = match base {
            Some(b) => b.to_string(),
            None => {
                let details = self.get_repo_details(owner, repo).await?;
                details.default_branch
            }
        };

        let data = self
            .post(
                &format!("/repos/{}/{}/pulls", owner, repo),
                &serde_json::json!({
                    "title": title,
                    "body": body,
                    "head": head,
                    "base": base_branch,
                }),
            )
            .await?;

        info!(
            pr = data["number"].as_i64().unwrap_or(0),
            "Created PR on {}/{}: {}",
            owner,
            repo,
            title
        );
        Ok(data)
    }

    /// Close a pull request with an optional comment.
    pub async fn close_pull_request(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        comment: Option<&str>,
    ) -> Result<()> {
        if let Some(c) = comment {
            self.post(
                &format!("/repos/{}/{}/issues/{}/comments", owner, repo, pr_number),
                &serde_json::json!({ "body": c }),
            )
            .await?;
        }
        self.patch(
            &format!("/repos/{}/{}/pulls/{}", owner, repo, pr_number),
            &serde_json::json!({ "state": "closed" }),
        )
        .await?;
        info!(pr = pr_number, "Closed PR on {}/{}", owner, repo);
        Ok(())
    }

    /// Update a PR's title and/or body.
    pub async fn update_pull_request(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<Value> {
        let mut payload = serde_json::Map::new();
        if let Some(t) = title {
            payload.insert("title".into(), Value::String(t.into()));
        }
        if let Some(b) = body {
            payload.insert("body".into(), Value::String(b.into()));
        }
        self.patch(
            &format!("/repos/{}/{}/pulls/{}", owner, repo, pr_number),
            &Value::Object(payload),
        )
        .await
    }

    // ── PR Comments & Reviews ──────────────────────────────────────────────

    /// Get comments on a pull request (issue comments).
    pub async fn get_pr_comments(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<Vec<Value>> {
        let data = self
            .get(&format!(
                "/repos/{}/{}/issues/{}/comments",
                owner, repo, pr_number
            ))
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Post a comment on a pull request.
    pub async fn create_pr_comment(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        body: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/repos/{}/{}/issues/{}/comments", owner, repo, pr_number),
            &serde_json::json!({ "body": body }),
        )
        .await
    }

    /// Get reviews on a PR.
    pub async fn get_pr_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<Vec<Value>> {
        let data = self
            .get(&format!(
                "/repos/{}/{}/pulls/{}/reviews",
                owner, repo, pr_number
            ))
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Get inline code-level review comments on a PR.
    pub async fn get_pr_review_comments(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<Vec<Value>> {
        let data = self
            .get(&format!(
                "/repos/{}/{}/pulls/{}/comments",
                owner, repo, pr_number
            ))
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Reply to an inline review comment.
    pub async fn create_pr_review_comment_reply(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        comment_id: i64,
        body: &str,
    ) -> Result<Value> {
        self.post(
            &format!(
                "/repos/{}/{}/pulls/{}/comments/{}/replies",
                owner, repo, pr_number, comment_id
            ),
            &serde_json::json!({ "body": body }),
        )
        .await
    }

    /// Get the diff of a pull request.
    pub async fn get_pr_diff(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<String> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github.v3.diff"));

        let response = self
            .request_raw(
                reqwest::Method::GET,
                &format!("/repos/{}/{}/pulls/{}", owner, repo, pr_number),
                Some(headers),
            )
            .await?;

        response
            .text()
            .await
            .map_err(|e| ContribError::GitHub(format!("Failed to read diff: {}", e)))
    }

    /// Get the authenticated user's profile.
    pub async fn get_authenticated_user(&self) -> Result<Value> {
        self.get("/user").await
    }

    // ── Issues ─────────────────────────────────────────────────────────────

    /// List pull requests on a repository.
    pub async fn list_pull_requests(
        &self,
        owner: &str,
        repo: &str,
        state: &str,
        per_page: u32,
    ) -> Result<Vec<Value>> {
        let pp = per_page.min(100).to_string();
        let data = self
            .get_with_params(
                &format!("/repos/{}/{}/pulls", owner, repo),
                &[
                    ("state", state),
                    ("per_page", &pp),
                    ("sort", "created"),
                    ("direction", "desc"),
                ],
            )
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Get combined CI status for a commit ref.
    pub async fn get_combined_status(
        &self,
        owner: &str,
        repo: &str,
        ref_name: &str,
    ) -> Result<CiStatus> {
        let checks = self
            .get_with_params(
                &format!("/repos/{}/{}/commits/{}/check-runs", owner, repo, ref_name),
                &[("per_page", "100")],
            )
            .await
            .unwrap_or(Value::Null);

        let runs = checks["check_runs"].as_array().cloned().unwrap_or_default();

        if runs.is_empty() {
            return Ok(CiStatus {
                state: "pending".into(),
                total: 0,
                failed: vec![],
                passed: vec![],
                in_progress: vec![],
            });
        }

        let mut failed = vec![];
        let mut passed = vec![];
        let mut in_progress = vec![];

        for run in &runs {
            let name = run["name"].as_str().unwrap_or("").to_string();
            match run["conclusion"].as_str() {
                Some("failure") => failed.push(name),
                Some("success") => passed.push(name),
                _ => {
                    if matches!(run["status"].as_str(), Some("queued" | "in_progress")) {
                        in_progress.push(name);
                    }
                }
            }
        }

        let state = if !in_progress.is_empty() {
            "pending"
        } else if !failed.is_empty() {
            "failure"
        } else {
            "success"
        };

        Ok(CiStatus {
            state: state.into(),
            total: runs.len(),
            failed,
            passed,
            in_progress,
        })
    }

    // ── Fork Management ────────────────────────────────────────────────────

    /// List all forks owned by the authenticated user.
    pub async fn list_user_forks(&self) -> Result<Vec<Value>> {
        let data = self
            .get_with_params("/user/repos", &[("type", "fork"), ("per_page", "100")])
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Delete a repository.
    pub async fn delete_repository(&self, owner: &str, repo: &str) -> Result<()> {
        self.delete(&format!("/repos/{}/{}", owner, repo)).await?;
        Ok(())
    }

    /// Search GitHub issues across repositories.
    pub async fn search_issues(
        &self,
        query: &str,
        sort: &str,
        per_page: u32,
    ) -> Result<Vec<Value>> {
        let pp = per_page.to_string();
        let data = self
            .get_with_params(
                "/search/issues",
                &[
                    ("q", query),
                    ("sort", sort),
                    ("order", "desc"),
                    ("per_page", &pp),
                ],
            )
            .await?;
        Ok(data["items"].as_array().cloned().unwrap_or_default())
    }

    /// Get the blob SHA of a file (needed for updates).
    pub async fn get_file_sha(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        ref_name: Option<&str>,
    ) -> Result<String> {
        let url = format!("/repos/{}/{}/contents/{}", owner, repo, path);
        let data = if let Some(r) = ref_name {
            self.get_with_params(&url, &[("ref", r)]).await?
        } else {
            self.get(&url).await?
        };
        Ok(data["sha"].as_str().unwrap_or("").to_string())
    }

    /// Get PR details.
    pub async fn get_pr_details(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<Value> {
        self.get(&format!("/repos/{}/{}/pulls/{}", owner, repo, pr_number))
            .await
    }

    /// Get issues assigned to a user.
    pub async fn get_assigned_issues(
        &self,
        owner: &str,
        repo: &str,
        assignee: &str,
    ) -> Result<Vec<Value>> {
        let data = self
            .get_with_params(
                &format!("/repos/{}/{}/issues", owner, repo),
                &[("assignee", assignee), ("state", "open")],
            )
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Create an issue.
    pub async fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/repos/{}/{}/issues", owner, repo),
            &serde_json::json!({ "title": title, "body": body }),
        )
        .await
    }

    /// Create an issue with labels.
    ///
    /// If the label does not exist on the target repository the API returns 422.
    /// Callers should catch the error and fall back to `create_issue` without labels.
    pub async fn create_issue_with_labels(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<Value> {
        let labels_json: Vec<Value> =
            labels.iter().map(|l| Value::String(l.to_string())).collect();
        self.post(
            &format!("/repos/{}/{}/issues", owner, repo),
            &serde_json::json!({ "title": title, "body": body, "labels": labels_json }),
        )
        .await
    }

    /// Close an issue.
    pub async fn close_issue(
        &self,
        owner: &str,
        repo: &str,
        issue_number: i64,
        comment: Option<&str>,
    ) -> Result<()> {
        if let Some(c) = comment {
            self.post(
                &format!("/repos/{}/{}/issues/{}/comments", owner, repo, issue_number),
                &serde_json::json!({ "body": c }),
            )
            .await?;
        }
        self.patch(
            &format!("/repos/{}/{}/issues/{}", owner, repo, issue_number),
            &serde_json::json!({ "state": "closed" }),
        )
        .await?;
        Ok(())
    }

    /// Get comments on an issue (uses the issues comments endpoint).
    ///
    /// GitHub's issues comments API is shared with PRs — calling it with
    /// an issue number returns that issue's comments.
    pub async fn get_issue_comments(
        &self,
        owner: &str,
        repo: &str,
        issue_number: i64,
    ) -> Result<Vec<Value>> {
        let data = self
            .get(&format!(
                "/repos/{}/{}/issues/{}/comments",
                owner, repo, issue_number
            ))
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// Get the timeline events for an issue.
    ///
    /// Useful for detecting cross-references from pull requests
    /// (`event == "cross-referenced"` with `source.type == "issue"` and
    /// `source.issue.pull_request` present).
    pub async fn get_issue_timeline(
        &self,
        owner: &str,
        repo: &str,
        issue_number: i64,
    ) -> Result<Vec<Value>> {
        let data = self
            .get_with_params(
                &format!(
                    "/repos/{}/{}/issues/{}/timeline",
                    owner, repo, issue_number
                ),
                &[("per_page", "100")],
            )
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    /// List open issues for a repository with optional label and assignee filters.
    ///
    /// Pass `labels` as a comma-separated string (e.g. `"bug,enhancement"`).
    /// Pass `assignee = Some("none")` to restrict to unassigned issues.
    pub async fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        labels: Option<&str>,
        assignee: Option<&str>,
        per_page: u32,
    ) -> Result<Vec<Value>> {
        let pp = per_page.min(100).to_string();
        let mut params: Vec<(&str, &str)> = vec![("state", "open"), ("per_page", &pp)];
        if let Some(l) = labels {
            params.push(("labels", l));
        }
        if let Some(a) = assignee {
            params.push(("assignee", a));
        }
        let data = self
            .get_with_params(
                &format!("/repos/{}/{}/issues", owner, repo),
                &params,
            )
            .await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }
}

// ── Helper Types & Functions ──────────────────────────────────────────────────

/// Rate limit information.
#[derive(Debug)]
pub struct RateLimitInfo {
    pub remaining: u32,
    pub limit: u32,
    pub reset: u64,
}

/// CI status information.
#[derive(Debug)]
pub struct CiStatus {
    pub state: String,
    pub total: usize,
    pub failed: Vec<String>,
    pub passed: Vec<String>,
    pub in_progress: Vec<String>,
}

/// Parse raw API response into Repository model.
pub fn parse_repo(data: &Value) -> Repository {
    let owner = &data["owner"];
    Repository {
        owner: owner["login"].as_str().unwrap_or("").to_string(),
        name: data["name"].as_str().unwrap_or("").to_string(),
        full_name: data["full_name"].as_str().unwrap_or("").to_string(),
        description: data["description"].as_str().map(String::from),
        language: data["language"].as_str().map(String::from),
        languages: HashMap::new(),
        stars: data["stargazers_count"].as_i64().unwrap_or(0),
        forks: data["forks_count"].as_i64().unwrap_or(0),
        open_issues: data["open_issues_count"].as_i64().unwrap_or(0),
        topics: data["topics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        default_branch: data["default_branch"]
            .as_str()
            .unwrap_or("main")
            .to_string(),
        html_url: data["html_url"].as_str().unwrap_or("").to_string(),
        clone_url: data["clone_url"].as_str().unwrap_or("").to_string(),
        has_contributing: false,
        has_license: !data["license"].is_null(),
        last_push_at: None,
        created_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repo() {
        let json = serde_json::json!({
            "owner": { "login": "tang-vu" },
            "name": "ContribAI",
            "full_name": "tang-vu/ContribAI",
            "description": "AI agent",
            "language": "Python",
            "stargazers_count": 184,
            "forks_count": 10,
            "open_issues_count": 5,
            "topics": ["ai", "github"],
            "default_branch": "main",
            "html_url": "https://github.com/tang-vu/ContribAI",
            "clone_url": "https://github.com/tang-vu/ContribAI.git",
            "license": { "spdx_id": "MIT" }
        });

        let repo = parse_repo(&json);
        assert_eq!(repo.owner, "tang-vu");
        assert_eq!(repo.name, "ContribAI");
        assert_eq!(repo.stars, 184);
        assert!(repo.has_license);
        assert_eq!(repo.topics, vec!["ai", "github"]);
    }

    #[test]
    fn test_parse_repo_minimal() {
        let json = serde_json::json!({
            "owner": { "login": "test" },
            "name": "repo",
            "full_name": "test/repo"
        });

        let repo = parse_repo(&json);
        assert_eq!(repo.owner, "test");
        assert_eq!(repo.stars, 0);
        assert!(!repo.has_license);
        assert_eq!(repo.default_branch, "main");
    }
}
