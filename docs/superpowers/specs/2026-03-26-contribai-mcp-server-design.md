# ContribAI MCP Server — Design Spec
**Date:** 2026-03-26
**Status:** Approved

## Overview

Build an MCP (Model Context Protocol) server for ContribAI that allows Claude Desktop to act as the sole AI brain. ContribAI provides GitHub integration tools; Claude Desktop handles all analysis, code reading, and fix generation.

## Goals

- Claude Desktop replaces all internal LLM calls (no Gemini/OpenAI/Anthropic API key needed)
- All existing ContribAI GitHub/safety/memory logic is reused as-is
- Full tool set: GitHub read/write + memory + patrol + cleanup

## Architecture

```
Claude Desktop (AI brain)
    │  MCP Protocol (stdio)
    ▼
contribai/mcp_server.py
    ├── Group 1: GitHub Read   (search_repos, get_repo_info, get_file_tree, get_file_content, get_open_issues)
    ├── Group 2: GitHub Write  (fork_repo, create_branch, push_file_change, create_pr, close_pr)
    ├── Group 3: Memory/Safety (check_duplicate_pr, check_ai_policy, get_stats)
    └── Group 4: Maintenance   (patrol_prs, cleanup_forks)
        │
        └── Reuses: GitHubClient, Memory (no LLM modules imported)
```

## Tools (14 total)

### Group 1: GitHub Read

| Tool | Input | Output | Notes |
|------|-------|--------|-------|
| `search_repos` | language, stars_min, stars_max, limit | list of {full_name, stars, language, description} | Builds query string: `language:X stars:min..max` internally |
| `get_repo_info` | owner, repo | {stars, language, open_issues, default_branch} | Calls `get_repo_details` → Repository model serialized to dict. Contributing guide fetched separately if needed via `get_file_content` |
| `get_file_tree` | owner, repo, max_files=200 | list of file paths | Calls `get_file_tree` then truncates client-side to max_files |
| `get_file_content` | owner, repo, path, ref=None | file content (string) | Uses `_get(f"/repos/{owner}/{repo}/contents/{path}", params={"ref": ref})` to support branch refs |
| `get_open_issues` | owner, repo, limit=20 | list of {number, title, body, labels} | Calls `get_open_issues` |

### Group 2: GitHub Write

| Tool | Input | Output | Notes |
|------|-------|--------|-------|
| `fork_repo` | owner, repo | fork_full_name | Calls `fork_repository` |
| `create_branch` | fork_owner, repo, branch_name, from_branch=None | branch_ref | `from_branch` defaults to repo default branch |
| `push_file_change` | fork_owner, repo, branch, path, content, commit_msg, sha=None | {commit_sha, content_url} | `sha` is blob SHA required for updates (get from prior `get_file_content`). Calls `create_or_update_file` |
| `create_pr` | owner, repo, title, body, head_branch, base_branch | {pr_number, pr_url} | Records to Memory DB after creation |
| `close_pr` | owner, repo, pr_number | {success: bool} | Catches exceptions → returns success bool |

### Group 3: Memory & Safety

| Tool | Input | Output | Notes |
|------|-------|--------|-------|
| `check_duplicate_pr` | owner, repo | {is_duplicate, existing_pr_url} | Checks Memory DB first, then live GitHub PRs |
| `check_ai_policy` | owner, repo | {banned: bool, reason: str} | Inlines `_check_ai_policy` logic from `pipeline.py` — fetches AI_POLICY.md, ai_policy.md via get_file_content |
| `get_stats` | — | {repos_analyzed, prs_submitted, prs_merged, merge_rate} | Calls `memory.get_stats()` + `memory.get_outcome_stats()`, computes merge_rate = prs_merged/prs_submitted |

### Group 4: Maintenance

| Tool | Input | Output | Notes |
|------|-------|--------|-------|
| `patrol_prs` | dry_run=True | {prs_checked, reviews_list: [{pr_number, repo, pr_url, comment_author, comment_body, is_inline, file_path}]} | Does NOT use PRPatrol (requires LLM). Collects raw review comments via GitHubClient directly. Returns structured list to Claude Desktop for reasoning |
| `cleanup_forks` | dry_run=True | {forks_to_delete: list, forks_kept: list} | Uses `GitHubClient._delete(f"/repos/{fork_name}")` — adds `delete_repository` method to GitHubClient |

## File Changes

**New file:** `contribai/mcp_server.py` (~450 lines)
**Modified:** `contribai/github/client.py`:
  - Add `ref: str | None = None` param to `get_file_content` (also fixes existing bug in `patrol.py`)
  - Add `list_user_forks() -> list[dict]` method — calls `GET /user/repos?type=fork`
  - Add `delete_repository(owner, repo)` method — calls `DELETE /repos/{owner}/{repo}`
**Modified:** `pyproject.toml` — add `mcp>=1.0,<2.0` to `[project.optional-dependencies].mcp`
**No other files modified.**

## Claude Desktop Config

File: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "contribai": {
      "command": "C:\\Users\\<username>\\AppData\\Local\\Programs\\Python\\Python312\\python.exe",
      "args": ["-m", "contribai.mcp_server"],
      "cwd": "C:\\Claude\\ContribAI",
      "env": {
        "GITHUB_TOKEN": "ghp_your_token_here"
      }
    }
  }
}
```

> Note: Use full path to python.exe (or virtualenv python). Claude Desktop launches with minimal PATH.

## Dependency

```bash
pip install mcp
```

Add to `pyproject.toml` under `[project] dependencies`:
```toml
"mcp>=1.0.0",
```

## Claude Desktop Workflow

```
search_repos
    → get_file_tree
    → get_file_content (multiple files, ref=default_branch)
    → [Claude analyzes + writes fix]
    → check_duplicate_pr + check_ai_policy
    → fork_repo
    → create_branch(from_branch=default_branch)
    → get_file_content(ref=fork_branch) to get sha
    → push_file_change(sha=<blob_sha>)
    → create_pr
```

## Implementation Notes

- Server runs via stdio (MCP standard) — Claude Desktop spawns the process
- Config loaded from `config.yaml` — only `github.token` required (or `GITHUB_TOKEN` env var)
- All tools are `async def` — GitHubClient already async
- Each tool wrapped in try/except, returns `{"error": "..."}` on failure
- Memory (SQLite) reused as-is — existing PR history preserved
- `patrol_prs` collects raw GitHub review comments via direct API calls (no LLM required); Claude Desktop does the classification and decides on fixes
- `get_file_tree` truncates results client-side (GitHubClient has no built-in limit)
- `push_file_change` requires `sha` for file updates — Claude must first call `get_file_content` to obtain blob SHA from response headers or a metadata endpoint
