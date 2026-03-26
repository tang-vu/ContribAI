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
        └── Reuses: GitHubClient, Memory, PRPatrol (no LLM modules imported)
```

## Tools (14 total)

### Group 1: GitHub Read
| Tool | Input | Output |
|------|-------|--------|
| `search_repos` | language, stars_min, stars_max, limit | list of repos |
| `get_repo_info` | owner, repo | metadata + contributing info |
| `get_file_tree` | owner, repo, max_files | list of file paths |
| `get_file_content` | owner, repo, path | file content string |
| `get_open_issues` | owner, repo, limit | list of issues |

### Group 2: GitHub Write
| Tool | Input | Output |
|------|-------|--------|
| `fork_repo` | owner, repo | fork_full_name |
| `create_branch` | fork_owner, repo, branch_name | branch_ref |
| `push_file_change` | fork_owner, repo, branch, path, content, commit_msg | commit_sha |
| `create_pr` | owner, repo, title, body, head_branch, base_branch | pr_number, pr_url |
| `close_pr` | owner, repo, pr_number | success bool |

### Group 3: Memory & Safety
| Tool | Input | Output |
|------|-------|--------|
| `check_duplicate_pr` | owner, repo | is_duplicate, existing_pr_url |
| `check_ai_policy` | owner, repo | banned bool, reason |
| `get_stats` | — | repos_analyzed, prs_submitted, merge_rate |

### Group 4: Maintenance
| Tool | Input | Output |
|------|-------|--------|
| `patrol_prs` | dry_run | prs_checked, fixes_needed, reviews_list |
| `cleanup_forks` | dry_run | forks_deleted, forks_kept |

## Claude Desktop Workflow

```
search_repos
    → get_file_tree
    → get_file_content (multiple files)
    → [Claude analyzes + writes fix]
    → check_duplicate_pr + check_ai_policy
    → fork_repo → create_branch → push_file_change → create_pr
```

## File Changes

**New file:** `contribai/mcp_server.py` (~400 lines)
**New dependency:** `pip install mcp`
**No other files modified.**

## Claude Desktop Config

`%APPDATA%\Claude\claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "contribai": {
      "command": "python",
      "args": ["-m", "contribai.mcp_server"],
      "cwd": "C:\\Claude\\ContribAI"
    }
  }
}
```

## Implementation Notes

- Server runs via stdio (MCP standard)
- Config loaded from `config.yaml` — only `github.token` required, no LLM key
- All tools are `async def` — GitHubClient already async
- Each tool wrapped in try/except, returns `{"error": "..."}` on failure
- Memory (SQLite) reused as-is — existing PR history preserved
- `patrol_prs` reuses `PRPatrol` class but returns review data to Claude instead of auto-fixing
