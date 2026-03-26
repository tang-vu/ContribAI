# ContribAI MCP Server Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an MCP server that exposes ContribAI's GitHub tools to Claude Desktop, replacing the internal LLM with Claude Desktop as the AI brain.

**Architecture:** A single `contribai/mcp_server.py` file imports ContribAI's `GitHubClient` and `Memory` modules directly (no LLM imports) and exposes 14 tools via the `mcp` library's stdio transport. Three small additions to `GitHubClient` unlock the full tool set.

**Tech Stack:** Python 3.11+, `mcp>=1.0`, `contribai.github.client.GitHubClient`, `contribai.orchestrator.memory.Memory`, `contribai.core.config.load_config`

**Spec:** `docs/superpowers/specs/2026-03-26-contribai-mcp-server-design.md`

---

## Chunk 1: GitHubClient prerequisites

### Task 1: Add `ref` parameter to `get_file_content`

**Files:**
- Modify: `contribai/github/client.py:174-179`
- Test: `tests/unit/test_github_client.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/unit/test_github_client.py`:

```python
class TestGetFileContentRef:
    @pytest.mark.asyncio
    async def test_get_file_content_with_ref(self, client):
        """ref param is passed as query param to GitHub API."""
        import respx, httpx, base64, json
        content_b64 = base64.b64encode(b"hello").decode()
        with respx.mock:
            respx.get(
                "https://api.github.com/repos/owner/repo/contents/file.py",
                params={"ref": "my-branch"},
            ).mock(return_value=httpx.Response(200, json={"encoding": "base64", "content": content_b64}))
            result = await client.get_file_content("owner", "repo", "file.py", ref="my-branch")
        assert result == "hello"

    @pytest.mark.asyncio
    async def test_get_file_content_without_ref(self, client):
        """ref param defaults to None — no query param sent."""
        import respx, httpx, base64
        content_b64 = base64.b64encode(b"world").decode()
        with respx.mock:
            respx.get(
                "https://api.github.com/repos/owner/repo/contents/file.py",
            ).mock(return_value=httpx.Response(200, json={"encoding": "base64", "content": content_b64}))
            result = await client.get_file_content("owner", "repo", "file.py")
        assert result == "world"
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_github_client.py::TestGetFileContentRef -v
```

Expected: `FAILED` — `get_file_content() got an unexpected keyword argument 'ref'`

- [ ] **Step 3: Update `get_file_content` in `contribai/github/client.py`**

Replace lines 174-179:

```python
async def get_file_content(
    self, owner: str, repo: str, path: str, ref: str | None = None
) -> str:
    """Get the content of a file from the repository."""
    params = {"ref": ref} if ref else None
    data = await self._get(f"/repos/{owner}/{repo}/contents/{path}", params=params)
    if data.get("encoding") == "base64":
        return base64.b64decode(data["content"]).decode("utf-8")
    return data.get("content", "")
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
pytest tests/unit/test_github_client.py::TestGetFileContentRef -v
```

Expected: `2 passed`

- [ ] **Step 5: Commit**

```bash
git add contribai/github/client.py tests/unit/test_github_client.py
git commit -m "fix: add ref param to get_file_content, fixes patrol.py bug"
```

---

### Task 2: Add `list_user_forks` and `delete_repository` to GitHubClient

**Files:**
- Modify: `contribai/github/client.py` (after `close_pull_request` method)
- Test: `tests/unit/test_github_client.py`

- [ ] **Step 1: Write failing tests**

Add to `tests/unit/test_github_client.py`:

```python
class TestListUserForks:
    @pytest.mark.asyncio
    async def test_returns_fork_list(self, client):
        import respx, httpx
        forks_data = [
            {"full_name": "me/forked-repo", "fork": True},
            {"full_name": "me/other-fork", "fork": True},
        ]
        with respx.mock:
            respx.get("https://api.github.com/user/repos", params={"type": "fork", "per_page": "100"}).mock(
                return_value=httpx.Response(200, json=forks_data)
            )
            result = await client.list_user_forks()
        assert len(result) == 2
        assert result[0]["full_name"] == "me/forked-repo"

    @pytest.mark.asyncio
    async def test_returns_empty_list_when_no_forks(self, client):
        import respx, httpx
        with respx.mock:
            respx.get("https://api.github.com/user/repos", params={"type": "fork", "per_page": "100"}).mock(
                return_value=httpx.Response(200, json=[])
            )
            result = await client.list_user_forks()
        assert result == []


class TestDeleteRepository:
    @pytest.mark.asyncio
    async def test_delete_success(self, client):
        import respx, httpx
        with respx.mock:
            respx.delete("https://api.github.com/repos/me/forked-repo").mock(
                return_value=httpx.Response(204)
            )
            # Should not raise
            await client.delete_repository("me", "forked-repo")

    @pytest.mark.asyncio
    async def test_delete_raises_on_error(self, client):
        import respx, httpx
        with respx.mock:
            respx.delete("https://api.github.com/repos/me/missing").mock(
                return_value=httpx.Response(404, json={"message": "Not Found"})
            )
            with pytest.raises(Exception):
                await client.delete_repository("me", "missing")
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_github_client.py::TestListUserForks tests/unit/test_github_client.py::TestDeleteRepository -v
```

Expected: `FAILED` — `AttributeError: 'GitHubClient' object has no attribute 'list_user_forks'`

- [ ] **Step 3: Add methods to `contribai/github/client.py`**

Find the `close_pull_request` method and add after it:

```python
async def list_user_forks(self) -> list[dict]:
    """List all forks owned by the authenticated user."""
    return await self._get("/user/repos", params={"type": "fork", "per_page": "100"})

async def delete_repository(self, owner: str, repo: str) -> None:
    """Delete a repository (must be owner or have admin access)."""
    await self._delete(f"/repos/{owner}/{repo}")
```

> Note: `_delete` already exists on `GitHubClient` at client.py line ~105.

- [ ] **Step 4: Run tests to verify they pass**

```bash
pytest tests/unit/test_github_client.py::TestListUserForks tests/unit/test_github_client.py::TestDeleteRepository -v
```

Expected: `4 passed`

- [ ] **Step 5: Run full test suite to check nothing broken**

```bash
pytest tests/unit/test_github_client.py -v
```

Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add contribai/github/client.py tests/unit/test_github_client.py
git commit -m "feat: add list_user_forks and delete_repository to GitHubClient"
```

---

## Chunk 2: MCP server — skeleton + GitHub Read tools

### Task 3: Install mcp and create server skeleton

**Files:**
- Create: `contribai/mcp_server.py`

- [ ] **Step 1: Install mcp package**

```bash
pip install "mcp>=1.0,<2.0"
```

Expected: `Successfully installed mcp-...`

- [ ] **Step 2: Verify mcp imports work**

```bash
python -c "from mcp.server import Server; from mcp.server.stdio import stdio_server; print('ok')"
```

Expected: `ok`

- [ ] **Step 3: Create `contribai/mcp_server.py` with skeleton**

```python
"""ContribAI MCP Server.

Exposes ContribAI's GitHub tools to Claude Desktop via the Model Context Protocol.
Claude Desktop acts as the AI brain; this server handles all GitHub I/O.

Run via: python -m contribai.mcp_server
"""
from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

import mcp.types as types
from mcp.server import Server
from mcp.server.stdio import stdio_server

from contribai.core.config import load_config
from contribai.github.client import GitHubClient
from contribai.orchestrator.memory import Memory

logger = logging.getLogger(__name__)

# ── Server init ────────────────────────────────────────────────────────────────

server = Server("contribai")
_config = load_config()
_github: GitHubClient | None = None
_memory: Memory | None = None


async def get_github() -> GitHubClient:
    global _github
    if _github is None:
        _github = GitHubClient(token=_config.github.token)
    return _github


async def get_memory() -> Memory:
    global _memory
    if _memory is None:
        _memory = Memory(_config.storage.resolved_db_path)
        await _memory.init()
    return _memory


def _ok(**kwargs: Any) -> list[types.TextContent]:
    return [types.TextContent(type="text", text=json.dumps(kwargs, default=str))]


def _err(msg: str) -> list[types.TextContent]:
    return [types.TextContent(type="text", text=json.dumps({"error": msg}))]


# ── Tool listing ───────────────────────────────────────────────────────────────

@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="search_repos",
            description="Search GitHub for open-source repositories by language and star range",
            inputSchema={
                "type": "object",
                "properties": {
                    "language": {"type": "string", "description": "e.g. python, javascript"},
                    "stars_min": {"type": "integer", "default": 50},
                    "stars_max": {"type": "integer", "default": 10000},
                    "limit": {"type": "integer", "default": 10},
                },
                "required": ["language"],
            },
        ),
        types.Tool(
            name="get_repo_info",
            description="Get metadata for a GitHub repository",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="get_file_tree",
            description="List files in a repository (recursive)",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "max_files": {"type": "integer", "default": 200},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="get_file_content",
            description="Get the content of a specific file from a repository",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "path": {"type": "string"},
                    "ref": {"type": "string", "description": "Branch or commit SHA (optional)"},
                },
                "required": ["owner", "repo", "path"],
            },
        ),
        types.Tool(
            name="get_open_issues",
            description="List open issues in a repository",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "limit": {"type": "integer", "default": 20},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="fork_repo",
            description="Fork a repository to the authenticated user's account",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="create_branch",
            description="Create a new branch on a repository",
            inputSchema={
                "type": "object",
                "properties": {
                    "fork_owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch_name": {"type": "string"},
                    "from_branch": {"type": "string", "description": "Source branch (defaults to repo default)"},
                },
                "required": ["fork_owner", "repo", "branch_name"],
            },
        ),
        types.Tool(
            name="push_file_change",
            description="Push a file change to a branch. For updates, sha (blob SHA) is required.",
            inputSchema={
                "type": "object",
                "properties": {
                    "fork_owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch": {"type": "string"},
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "commit_msg": {"type": "string"},
                    "sha": {"type": "string", "description": "Blob SHA of existing file (required for updates)"},
                },
                "required": ["fork_owner", "repo", "branch", "path", "content", "commit_msg"],
            },
        ),
        types.Tool(
            name="create_pr",
            description="Create a pull request from a fork branch to the upstream repo",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "head_branch": {"type": "string", "description": "fork_owner:branch"},
                    "base_branch": {"type": "string", "description": "Target branch (defaults to default branch)"},
                },
                "required": ["owner", "repo", "title", "body", "head_branch"],
            },
        ),
        types.Tool(
            name="close_pr",
            description="Close a pull request",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pr_number": {"type": "integer"},
                },
                "required": ["owner", "repo", "pr_number"],
            },
        ),
        types.Tool(
            name="check_duplicate_pr",
            description="Check if ContribAI has already submitted a PR to this repo",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="check_ai_policy",
            description="Check if a repo bans AI-generated contributions",
            inputSchema={
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                },
                "required": ["owner", "repo"],
            },
        ),
        types.Tool(
            name="get_stats",
            description="Get ContribAI contribution statistics from the local database",
            inputSchema={"type": "object", "properties": {}},
        ),
        types.Tool(
            name="patrol_prs",
            description="Collect raw review comments from open PRs for Claude to classify and act on",
            inputSchema={
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "default": True},
                },
            },
        ),
        types.Tool(
            name="cleanup_forks",
            description="List or delete stale forks where all PRs are merged/closed",
            inputSchema={
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "default": True},
                },
            },
        ),
    ]


# ── Tool dispatch ──────────────────────────────────────────────────────────────

@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[types.TextContent]:
    try:
        if name == "search_repos":
            return await _search_repos(arguments)
        elif name == "get_repo_info":
            return await _get_repo_info(arguments)
        elif name == "get_file_tree":
            return await _get_file_tree(arguments)
        elif name == "get_file_content":
            return await _get_file_content(arguments)
        elif name == "get_open_issues":
            return await _get_open_issues(arguments)
        elif name == "fork_repo":
            return await _fork_repo(arguments)
        elif name == "create_branch":
            return await _create_branch(arguments)
        elif name == "push_file_change":
            return await _push_file_change(arguments)
        elif name == "create_pr":
            return await _create_pr(arguments)
        elif name == "close_pr":
            return await _close_pr(arguments)
        elif name == "check_duplicate_pr":
            return await _check_duplicate_pr(arguments)
        elif name == "check_ai_policy":
            return await _check_ai_policy(arguments)
        elif name == "get_stats":
            return await _get_stats(arguments)
        elif name == "patrol_prs":
            return await _patrol_prs(arguments)
        elif name == "cleanup_forks":
            return await _cleanup_forks(arguments)
        else:
            return _err(f"Unknown tool: {name}")
    except Exception as e:
        logger.exception("Tool %s failed", name)
        return _err(str(e))


# ── Tool implementations (placeholder stubs — filled in Tasks 4-7) ─────────────

async def _search_repos(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _get_repo_info(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _get_file_tree(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _get_file_content(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _get_open_issues(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _fork_repo(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _create_branch(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _push_file_change(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _create_pr(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _close_pr(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _check_duplicate_pr(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _check_ai_policy(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _get_stats(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _patrol_prs(args: dict) -> list[types.TextContent]:
    return _err("not implemented")

async def _cleanup_forks(args: dict) -> list[types.TextContent]:
    return _err("not implemented")


# ── Entry point ────────────────────────────────────────────────────────────────

async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
```

- [ ] **Step 4: Verify the server starts without errors**

> Note: `import contribai.mcp_server` runs `load_config()` at module level. This requires either a valid `config.yaml` in the project root or a `GITHUB_TOKEN` env var to be set. If neither is present, set a minimal env var first:
> `set GITHUB_TOKEN=ghp_dummy` (Windows) or `export GITHUB_TOKEN=ghp_dummy` (Unix)

```bash
python -c "import contribai.mcp_server; print('import ok')"
```

Expected: `import ok`

- [ ] **Step 5: Commit**

```bash
git add contribai/mcp_server.py
git commit -m "feat: add MCP server skeleton with 14 tool stubs"
```

---

### Task 4: Implement GitHub Read tools (5 tools)

**Files:**
- Modify: `contribai/mcp_server.py` — replace stub implementations
- Test: `tests/unit/test_mcp_server.py` (new)

- [ ] **Step 1: Create test file with failing tests**

Create `tests/unit/test_mcp_server.py`:

```python
"""Tests for ContribAI MCP server tool implementations."""
from unittest.mock import AsyncMock, MagicMock, patch
import json
import pytest

import mcp.types as types


def _text(result: list[types.TextContent]) -> dict:
    return json.loads(result[0].text)


class TestSearchRepos:
    @pytest.mark.asyncio
    async def test_returns_repo_list(self):
        from contribai.mcp_server import _search_repos
        mock_repo = MagicMock()
        mock_repo.full_name = "owner/repo"
        mock_repo.stars = 1000
        mock_repo.language = "Python"
        mock_repo.description = "A test repo"

        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.search_repositories = AsyncMock(return_value=[mock_repo])
            mock_get_gh.return_value = gh
            result = await _search_repos({"language": "python", "stars_min": 100, "stars_max": 5000, "limit": 5})

        data = _text(result)
        assert "repos" in data
        assert data["repos"][0]["full_name"] == "owner/repo"

    @pytest.mark.asyncio
    async def test_builds_query_string(self):
        from contribai.mcp_server import _search_repos
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.search_repositories = AsyncMock(return_value=[])
            mock_get_gh.return_value = gh
            await _search_repos({"language": "javascript", "stars_min": 50, "stars_max": 2000})
            call_args = gh.search_repositories.call_args
            assert "language:javascript" in call_args[0][0]
            assert "stars:50..2000" in call_args[0][0]


class TestGetFileContent:
    @pytest.mark.asyncio
    async def test_returns_content(self):
        from contribai.mcp_server import _get_file_content
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content = AsyncMock(return_value="print('hello')")
            mock_get_gh.return_value = gh
            result = await _get_file_content({"owner": "o", "repo": "r", "path": "main.py"})
        data = _text(result)
        assert data["content"] == "print('hello')"

    @pytest.mark.asyncio
    async def test_passes_ref_param(self):
        from contribai.mcp_server import _get_file_content
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content = AsyncMock(return_value="x = 1")
            mock_get_gh.return_value = gh
            await _get_file_content({"owner": "o", "repo": "r", "path": "f.py", "ref": "fix-branch"})
            gh.get_file_content.assert_called_once_with("o", "r", "f.py", ref="fix-branch")


class TestGetFileTree:
    @pytest.mark.asyncio
    async def test_returns_file_list(self):
        from contribai.mcp_server import _get_file_tree
        mock_node = MagicMock()
        mock_node.path = "src/main.py"
        mock_node.type = "blob"
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_repo_details = AsyncMock(return_value=MagicMock(default_branch="main"))
            gh.get_file_tree = AsyncMock(return_value=[mock_node])
            mock_get_gh.return_value = gh
            result = await _get_file_tree({"owner": "o", "repo": "r"})
        data = _text(result)
        assert "files" in data
        assert "src/main.py" in data["files"]

    @pytest.mark.asyncio
    async def test_respects_max_files(self):
        from contribai.mcp_server import _get_file_tree
        nodes = [MagicMock(path=f"f{i}.py", type="blob") for i in range(500)]
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_repo_details = AsyncMock(return_value=MagicMock(default_branch="main"))
            gh.get_file_tree = AsyncMock(return_value=nodes)
            mock_get_gh.return_value = gh
            result = await _get_file_tree({"owner": "o", "repo": "r", "max_files": 10})
        data = _text(result)
        assert len(data["files"]) == 10
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_mcp_server.py -v
```

Expected: `FAILED` — `not implemented` errors

- [ ] **Step 3: Implement the 5 GitHub Read tool functions in `contribai/mcp_server.py`**

Replace the stub implementations:

```python
async def _search_repos(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    language = args["language"]
    stars_min = args.get("stars_min", 50)
    stars_max = args.get("stars_max", 10000)
    limit = args.get("limit", 10)
    query = f"language:{language} stars:{stars_min}..{stars_max}"
    repos = await gh.search_repositories(query, per_page=limit)
    return _ok(repos=[
        {"full_name": r.full_name, "stars": r.stars, "language": r.language, "description": r.description}
        for r in repos
    ])


async def _get_repo_info(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    repo = await gh.get_repo_details(args["owner"], args["repo"])
    return _ok(
        full_name=repo.full_name,
        stars=repo.stars,
        language=repo.language,
        open_issues=repo.open_issues,
        default_branch=repo.default_branch,
        description=repo.description,
    )


async def _get_file_tree(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    owner, repo = args["owner"], args["repo"]
    max_files = args.get("max_files", 200)
    details = await gh.get_repo_details(owner, repo)
    nodes = await gh.get_file_tree(owner, repo, details.default_branch)
    files = [n.path for n in nodes if n.type == "blob"][:max_files]
    return _ok(files=files, total=len(nodes))


async def _get_file_content(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    content = await gh.get_file_content(
        args["owner"], args["repo"], args["path"], ref=args.get("ref")
    )
    return _ok(content=content)


async def _get_open_issues(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    issues = await gh.get_open_issues(args["owner"], args["repo"], per_page=args.get("limit", 20))
    return _ok(issues=[
        {"number": i.number, "title": i.title, "body": i.body, "labels": i.labels}
        for i in issues
    ])
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
pytest tests/unit/test_mcp_server.py::TestSearchRepos tests/unit/test_mcp_server.py::TestGetFileContent tests/unit/test_mcp_server.py::TestGetFileTree -v
```

Expected: `5 passed`

- [ ] **Step 5: Commit**

```bash
git add contribai/mcp_server.py tests/unit/test_mcp_server.py
git commit -m "feat: implement GitHub Read tools in MCP server"
```

---

## Chunk 3: GitHub Write tools + Memory/Safety tools

### Task 5: Implement GitHub Write tools (5 tools)

**Files:**
- Modify: `contribai/mcp_server.py`
- Test: `tests/unit/test_mcp_server.py`

- [ ] **Step 1: Add failing tests**

Append to `tests/unit/test_mcp_server.py`:

```python
class TestForkRepo:
    @pytest.mark.asyncio
    async def test_returns_fork_name(self):
        from contribai.mcp_server import _fork_repo
        fork = MagicMock(full_name="me/upstream-repo")
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.fork_repository = AsyncMock(return_value=fork)
            mock_get_gh.return_value = gh
            result = await _fork_repo({"owner": "upstream", "repo": "upstream-repo"})
        data = _text(result)
        assert data["fork_full_name"] == "me/upstream-repo"


class TestCreateBranch:
    @pytest.mark.asyncio
    async def test_returns_branch_ref(self):
        from contribai.mcp_server import _create_branch
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.create_branch = AsyncMock(return_value={"ref": "refs/heads/fix-typo"})
            mock_get_gh.return_value = gh
            result = await _create_branch({"fork_owner": "me", "repo": "r", "branch_name": "fix-typo"})
        data = _text(result)
        assert data["ref"] == "refs/heads/fix-typo"


class TestPushFileChange:
    @pytest.mark.asyncio
    async def test_returns_commit_sha(self):
        from contribai.mcp_server import _push_file_change
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.create_or_update_file = AsyncMock(return_value={"commit": {"sha": "abc123"}})
            mock_get_gh.return_value = gh
            result = await _push_file_change({
                "fork_owner": "me", "repo": "r", "branch": "fix-typo",
                "path": "README.md", "content": "# Fixed", "commit_msg": "fix: typo"
            })
        data = _text(result)
        assert data["commit_sha"] == "abc123"


class TestCreatePR:
    @pytest.mark.asyncio
    async def test_returns_pr_info(self):
        from contribai.mcp_server import _create_pr
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.create_pull_request = AsyncMock(return_value={"number": 42, "html_url": "https://github.com/owner/repo/pull/42"})
            mock_get_gh.return_value = gh
            with patch("contribai.mcp_server.get_memory") as mock_get_mem:
                mem = AsyncMock()
                mock_get_mem.return_value = mem
                result = await _create_pr({
                    "owner": "owner", "repo": "repo",
                    "title": "fix: typo", "body": "Fixed a typo",
                    "head_branch": "me:fix-typo",
                })
        data = _text(result)
        assert data["pr_number"] == 42
        assert "pull/42" in data["pr_url"]


class TestClosePR:
    @pytest.mark.asyncio
    async def test_returns_success_true(self):
        from contribai.mcp_server import _close_pr
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.close_pull_request = AsyncMock(return_value=None)
            mock_get_gh.return_value = gh
            result = await _close_pr({"owner": "o", "repo": "r", "pr_number": 1})
        data = _text(result)
        assert data["success"] is True

    @pytest.mark.asyncio
    async def test_returns_success_false_on_error(self):
        from contribai.mcp_server import _close_pr
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.close_pull_request = AsyncMock(side_effect=Exception("API error"))
            mock_get_gh.return_value = gh
            result = await _close_pr({"owner": "o", "repo": "r", "pr_number": 99})
        data = _text(result)
        assert data["success"] is False
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_mcp_server.py::TestForkRepo tests/unit/test_mcp_server.py::TestCreateBranch tests/unit/test_mcp_server.py::TestPushFileChange tests/unit/test_mcp_server.py::TestCreatePR tests/unit/test_mcp_server.py::TestClosePR -v
```

Expected: all `FAILED` — `not implemented`

- [ ] **Step 3: Implement GitHub Write tools in `contribai/mcp_server.py`**

```python
async def _fork_repo(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    fork = await gh.fork_repository(args["owner"], args["repo"])
    return _ok(fork_full_name=fork.full_name)


async def _create_branch(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    ref = await gh.create_branch(
        args["fork_owner"], args["repo"], args["branch_name"],
        from_branch=args.get("from_branch"),
    )
    return _ok(ref=ref.get("ref", ""))


async def _push_file_change(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    result = await gh.create_or_update_file(
        owner=args["fork_owner"],
        repo=args["repo"],
        path=args["path"],
        content=args["content"],
        message=args["commit_msg"],
        branch=args["branch"],
        sha=args.get("sha"),
    )
    return _ok(commit_sha=result.get("commit", {}).get("sha", ""))


async def _create_pr(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    mem = await get_memory()
    pr_data = await gh.create_pull_request(
        owner=args["owner"],
        repo=args["repo"],
        title=args["title"],
        body=args["body"],
        head=args["head_branch"],
        base=args.get("base_branch"),
    )
    pr_number = pr_data["number"]
    pr_url = pr_data["html_url"]
    # Record to memory so status/duplicate checks work
    await mem.record_pr(
        repo=f"{args['owner']}/{args['repo']}",
        pr_number=pr_number,
        pr_url=pr_url,
        title=args["title"],
        pr_type="mcp",
    )
    return _ok(pr_number=pr_number, pr_url=pr_url)


async def _close_pr(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    try:
        await gh.close_pull_request(args["owner"], args["repo"], args["pr_number"])
        return _ok(success=True)
    except Exception as e:
        return _ok(success=False, reason=str(e))
```

- [ ] **Step 4: Run tests**

```bash
pytest tests/unit/test_mcp_server.py::TestForkRepo tests/unit/test_mcp_server.py::TestCreateBranch tests/unit/test_mcp_server.py::TestPushFileChange tests/unit/test_mcp_server.py::TestCreatePR tests/unit/test_mcp_server.py::TestClosePR -v
```

Expected: `6 passed`

- [ ] **Step 5: Commit**

```bash
git add contribai/mcp_server.py tests/unit/test_mcp_server.py
git commit -m "feat: implement GitHub Write tools in MCP server"
```

---

### Task 6: Implement Memory & Safety tools (3 tools)

**Files:**
- Modify: `contribai/mcp_server.py`
- Test: `tests/unit/test_mcp_server.py`

- [ ] **Step 1: Add failing tests**

Append to `tests/unit/test_mcp_server.py`:

```python
class TestCheckDuplicatePR:
    @pytest.mark.asyncio
    async def test_no_duplicate(self):
        from contribai.mcp_server import _check_duplicate_pr
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_repo_prs = AsyncMock(return_value=[])
            mock_get_mem.return_value = mem
            result = await _check_duplicate_pr({"owner": "o", "repo": "r"})
        data = _text(result)
        assert data["is_duplicate"] is False

    @pytest.mark.asyncio
    async def test_finds_existing_open_pr(self):
        from contribai.mcp_server import _check_duplicate_pr
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_repo_prs = AsyncMock(return_value=[
                {"status": "open", "pr_url": "https://github.com/o/r/pull/5"}
            ])
            mock_get_mem.return_value = mem
            result = await _check_duplicate_pr({"owner": "o", "repo": "r"})
        data = _text(result)
        assert data["is_duplicate"] is True
        assert "pull/5" in data["existing_pr_url"]


class TestCheckAIPolicy:
    @pytest.mark.asyncio
    async def test_not_banned_when_no_policy_file(self):
        from contribai.mcp_server import _check_ai_policy
        from contribai.core.exceptions import GitHubAPIError
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content = AsyncMock(side_effect=GitHubAPIError("not found", 404))
            mock_get_gh.return_value = gh
            result = await _check_ai_policy({"owner": "o", "repo": "r"})
        data = _text(result)
        assert data["banned"] is False

    @pytest.mark.asyncio
    async def test_banned_when_policy_prohibits_ai(self):
        from contribai.mcp_server import _check_ai_policy
        policy_content = "We do not accept AI-generated contributions."
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content = AsyncMock(return_value=policy_content)
            mock_get_gh.return_value = gh
            result = await _check_ai_policy({"owner": "o", "repo": "r"})
        data = _text(result)
        assert data["banned"] is True


class TestGetStats:
    @pytest.mark.asyncio
    async def test_returns_stats(self):
        from contribai.mcp_server import _get_stats
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_stats = AsyncMock(return_value={
                "total_repos_analyzed": 10,
                "total_prs_submitted": 5,
                "prs_merged": 3,
            })
            mem.get_outcome_stats = AsyncMock(return_value={"avg_merge_rate": 0.6})
            mock_get_mem.return_value = mem
            result = await _get_stats({})
        data = _text(result)
        assert data["prs_submitted"] == 5
        assert data["merge_rate"] == "60%"
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_mcp_server.py::TestCheckDuplicatePR tests/unit/test_mcp_server.py::TestCheckAIPolicy tests/unit/test_mcp_server.py::TestGetStats -v
```

Expected: all `FAILED`

- [ ] **Step 3: Implement Memory & Safety tools in `contribai/mcp_server.py`**

```python
# AI policy keywords (inlined from pipeline._check_ai_policy)
_AI_BAN_KEYWORDS = [
    "no ai", "no-ai", "not accept ai", "prohibit ai", "ban ai",
    "ai generated", "ai-generated", "no llm", "human only",
]
_AI_POLICY_PATHS = ["AI_POLICY.md", ".github/AI_POLICY.md", "ai_policy.md", ".github/ai_policy.md"]


async def _check_duplicate_pr(args: dict) -> list[types.TextContent]:
    mem = await get_memory()
    repo = f"{args['owner']}/{args['repo']}"
    prs = await mem.get_repo_prs(repo)
    open_prs = [p for p in prs if p.get("status") == "open"]
    if open_prs:
        return _ok(is_duplicate=True, existing_pr_url=open_prs[0]["pr_url"])
    return _ok(is_duplicate=False, existing_pr_url=None)


async def _check_ai_policy(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    from contribai.core.exceptions import GitHubAPIError
    for path in _AI_POLICY_PATHS:
        try:
            content = await gh.get_file_content(args["owner"], args["repo"], path)
            lower = content.lower()
            banned = any(kw in lower for kw in _AI_BAN_KEYWORDS)
            return _ok(banned=banned, reason=path if banned else None)
        except GitHubAPIError:
            continue
    return _ok(banned=False, reason=None)


async def _get_stats(args: dict) -> list[types.TextContent]:
    mem = await get_memory()
    stats = await mem.get_stats()
    try:
        outcome = await mem.get_outcome_stats()
        rate = outcome.get("avg_merge_rate", 0)
    except Exception:
        rate = 0
    return _ok(
        repos_analyzed=stats.get("total_repos_analyzed", 0),
        prs_submitted=stats.get("total_prs_submitted", 0),
        prs_merged=stats.get("prs_merged", 0),
        merge_rate=f"{rate:.0%}",
    )
```

- [ ] **Step 4: Run tests**

```bash
pytest tests/unit/test_mcp_server.py::TestCheckDuplicatePR tests/unit/test_mcp_server.py::TestCheckAIPolicy tests/unit/test_mcp_server.py::TestGetStats -v
```

Expected: `5 passed`

- [ ] **Step 5: Commit**

```bash
git add contribai/mcp_server.py tests/unit/test_mcp_server.py
git commit -m "feat: implement Memory and Safety tools in MCP server"
```

---

## Chunk 4: Maintenance tools + integration

### Task 7: Implement `patrol_prs` and `cleanup_forks`

**Files:**
- Modify: `contribai/mcp_server.py`
- Test: `tests/unit/test_mcp_server.py`

- [ ] **Step 1: Add failing tests**

Append to `tests/unit/test_mcp_server.py`:

```python
class TestPatrolPRs:
    @pytest.mark.asyncio
    async def test_returns_review_list(self):
        from contribai.mcp_server import _patrol_prs
        open_pr = {"repo": "owner/repo", "pr_number": 7, "pr_url": "https://github.com/owner/repo/pull/7"}
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_prs = AsyncMock(return_value=[open_pr])
            mock_get_mem.return_value = mem
            with patch("contribai.mcp_server.get_github") as mock_get_gh:
                gh = AsyncMock()
                gh.get_pr_comments = AsyncMock(return_value=[
                    {"user": {"login": "maintainer"}, "body": "Please add tests", "id": 1}
                ])
                gh.get_pr_review_comments = AsyncMock(return_value=[])
                mock_get_gh.return_value = gh
                result = await _patrol_prs({"dry_run": True})
        data = _text(result)
        assert data["prs_checked"] == 1
        assert len(data["reviews_list"]) == 1
        assert data["reviews_list"][0]["comment_author"] == "maintainer"

    @pytest.mark.asyncio
    async def test_returns_empty_when_no_open_prs(self):
        from contribai.mcp_server import _patrol_prs
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_prs = AsyncMock(return_value=[])
            mock_get_mem.return_value = mem
            result = await _patrol_prs({})
        data = _text(result)
        assert data["prs_checked"] == 0
        assert data["reviews_list"] == []


class TestCleanupForks:
    @pytest.mark.asyncio
    async def test_dry_run_lists_but_does_not_delete(self):
        from contribai.mcp_server import _cleanup_forks
        fork_data = {"full_name": "me/old-fork"}
        # PRs stored with fork="me/old-fork" (the fork column in submitted_prs)
        all_prs = [{"fork": "me/old-fork", "status": "merged", "repo": "upstream/repo", "pr_number": 1}]
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.list_user_forks = AsyncMock(return_value=[fork_data])
            gh.delete_repository = AsyncMock()
            mock_get_gh.return_value = gh
            with patch("contribai.mcp_server.get_memory") as mock_get_mem:
                mem = AsyncMock()
                mem.get_prs = AsyncMock(return_value=all_prs)
                mock_get_mem.return_value = mem
                result = await _cleanup_forks({"dry_run": True})
        data = _text(result)
        assert "me/old-fork" in data["forks_to_delete"]
        gh.delete_repository.assert_not_called()
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/unit/test_mcp_server.py::TestPatrolPRs tests/unit/test_mcp_server.py::TestCleanupForks -v
```

Expected: all `FAILED`

- [ ] **Step 3: Implement patrol_prs and cleanup_forks in `contribai/mcp_server.py`**

```python
async def _patrol_prs(args: dict) -> list[types.TextContent]:
    mem = await get_memory()
    gh = await get_github()
    open_prs = await mem.get_prs(status="open", limit=100)
    if not open_prs:
        return _ok(prs_checked=0, reviews_list=[])

    reviews_list = []
    for pr in open_prs:
        repo = pr["repo"]
        pr_number = pr["pr_number"]
        pr_url = pr.get("pr_url", "")
        try:
            owner, repo_name = repo.split("/", 1)
            # Issue comments (general PR comments)
            comments = await gh.get_pr_comments(owner, repo_name, pr_number)
            for c in comments:
                reviews_list.append({
                    "pr_number": pr_number,
                    "repo": repo,
                    "pr_url": pr_url,
                    "comment_author": c.get("user", {}).get("login", ""),
                    "comment_body": c.get("body", ""),
                    "is_inline": False,
                    "file_path": None,
                })
            # Inline review comments
            inline = await gh.get_pr_review_comments(owner, repo_name, pr_number)
            for c in inline:
                reviews_list.append({
                    "pr_number": pr_number,
                    "repo": repo,
                    "pr_url": pr_url,
                    "comment_author": c.get("user", {}).get("login", ""),
                    "comment_body": c.get("body", ""),
                    "is_inline": True,
                    "file_path": c.get("path"),
                })
        except Exception as e:
            logger.warning("Failed to fetch comments for %s#%d: %s", repo, pr_number, e)

    return _ok(prs_checked=len(open_prs), reviews_list=reviews_list)


async def _cleanup_forks(args: dict) -> list[types.TextContent]:
    dry_run = args.get("dry_run", True)
    gh = await get_github()
    mem = await get_memory()

    forks = await gh.list_user_forks()
    forks_to_delete = []
    forks_kept = []

    # PRs are recorded against the upstream repo (owner/repo), not the fork.
    # e.g., fork "me/react" → upstream PRs stored as "facebook/react".
    # Strategy: check ALL prs in memory whose branch includes this fork owner.
    # Simpler: get all submitted PRs and group by fork field.
    all_prs = await mem.get_prs(limit=10000)
    prs_by_fork: dict[str, list[dict]] = {}
    for pr in all_prs:
        fork_field = pr.get("fork", "")
        if fork_field:
            prs_by_fork.setdefault(fork_field, []).append(pr)

    for fork in forks:
        fork_name = fork["full_name"]
        # Match by fork full_name stored in the 'fork' column of submitted_prs
        prs = prs_by_fork.get(fork_name, [])
        has_open = any(p.get("status") == "open" for p in prs)
        if prs and not has_open:
            forks_to_delete.append(fork_name)
        else:
            forks_kept.append(fork_name)

    if not dry_run:
        for fork_name in forks_to_delete:
            try:
                owner, repo_name = fork_name.split("/", 1)
                await gh.delete_repository(owner, repo_name)
                logger.info("Deleted fork %s", fork_name)
            except Exception as e:
                logger.warning("Failed to delete %s: %s", fork_name, e)

    return _ok(forks_to_delete=forks_to_delete, forks_kept=forks_kept, dry_run=dry_run)
```

- [ ] **Step 4: Run tests**

```bash
pytest tests/unit/test_mcp_server.py::TestPatrolPRs tests/unit/test_mcp_server.py::TestCleanupForks -v
```

Expected: `4 passed`

- [ ] **Step 5: Run full MCP server test suite**

```bash
pytest tests/unit/test_mcp_server.py -v
```

Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add contribai/mcp_server.py tests/unit/test_mcp_server.py
git commit -m "feat: implement patrol_prs and cleanup_forks tools"
```

---

### Task 8: Claude Desktop integration

**Files:**
- No code changes — config only

- [ ] **Step 1: Find your Python executable path**

```bash
python -c "import sys; print(sys.executable)"
```

Note the output path (e.g. `C:\Users\hoang\AppData\Local\Programs\Python\Python312\python.exe`)

- [ ] **Step 2: Open Claude Desktop config**

File location: `%APPDATA%\Claude\claude_desktop_config.json`

If the file does not exist, create it. Add:

```json
{
  "mcpServers": {
    "contribai": {
      "command": "<YOUR_PYTHON_PATH>",
      "args": ["-m", "contribai.mcp_server"],
      "cwd": "C:\\Claude\\ContribAI",
      "env": {
        "GITHUB_TOKEN": "ghp_your_token_here"
      }
    }
  }
}
```

Replace `<YOUR_PYTHON_PATH>` with the path from Step 1.
Replace `ghp_your_token_here` with your GitHub token.

- [ ] **Step 3: Restart Claude Desktop**

Fully quit and reopen Claude Desktop.

- [ ] **Step 4: Verify tools appear**

In Claude Desktop, type:
> "What ContribAI tools do you have available?"

Expected: Claude lists all 14 tools (search_repos, get_repo_info, get_file_tree, etc.)

- [ ] **Step 5: Smoke test end-to-end**

In Claude Desktop, type:
> "Use ContribAI to search for Python repos with 500-2000 stars and show me the top 3"

Expected: Claude calls `search_repos` and returns a list of repos.

---

### Task 9: Final verification

- [ ] **Step 1: Run full test suite**

```bash
pytest tests/ -v --tb=short
```

Expected: all tests pass (no regressions)

- [ ] **Step 2: Lint**

```bash
ruff check contribai/mcp_server.py contribai/github/client.py
```

Expected: no errors

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "feat: complete ContribAI MCP server v1.0"
```
