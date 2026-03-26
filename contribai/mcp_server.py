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
from contribai.core.exceptions import GitHubAPIError
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
                    "from_branch": {
                        "type": "string",
                        "description": "Source branch (defaults to repo default)",
                    },
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
                    "sha": {
                        "type": "string",
                        "description": "Blob SHA of existing file (required for updates)",
                    },
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
                    "base_branch": {
                        "type": "string",
                        "description": "Target branch (defaults to default branch)",
                    },
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
            description=(
                "Collect raw review comments from open PRs for Claude to classify and act on"
            ),
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


# ── Tool implementations (stubs — filled in subsequent tasks) ──────────────────


async def _search_repos(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    language = args["language"]
    stars_min = args.get("stars_min", 50)
    stars_max = args.get("stars_max", 10000)
    limit = args.get("limit", 10)
    query = f"language:{language} stars:{stars_min}..{stars_max}"
    repos = await gh.search_repositories(query, per_page=limit)
    return _ok(
        repos=[
            {
                "full_name": r.full_name,
                "stars": r.stars,
                "language": r.language,
                "description": r.description,
            }
            for r in repos
        ]
    )


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
    nodes = await gh.get_file_tree(owner, repo)
    blobs = [n.path for n in nodes if n.type == "blob"]
    return _ok(files=blobs[:max_files], total=len(blobs))


async def _get_file_content(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    content, sha = await gh.get_file_content_with_sha(
        args["owner"], args["repo"], args["path"], ref=args.get("ref")
    )
    return _ok(content=content, sha=sha)


async def _get_open_issues(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    issues = await gh.get_open_issues(args["owner"], args["repo"], per_page=args.get("limit", 20))
    return _ok(
        issues=[
            {"number": i.number, "title": i.title, "body": i.body, "labels": i.labels}
            for i in issues
        ]
    )


async def _fork_repo(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    fork = await gh.fork_repository(args["owner"], args["repo"])
    return _ok(fork_full_name=fork.full_name)


async def _create_branch(args: dict) -> list[types.TextContent]:
    gh = await get_github()
    ref = await gh.create_branch(
        args["fork_owner"],
        args["repo"],
        args["branch_name"],
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
    return _ok(
        commit_sha=result.get("commit", {}).get("sha", ""),
        content_url=result.get("content", {}).get("html_url", ""),
    )


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


# AI policy keywords (inlined from pipeline._check_ai_policy)
_AI_BAN_KEYWORDS = [
    "no ai",
    "no-ai",
    "not accept ai",
    "prohibit ai",
    "ban ai",
    "ai generated",
    "ai-generated",
    "no llm",
    "human only",
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
                reviews_list.append(
                    {
                        "pr_number": pr_number,
                        "repo": repo,
                        "pr_url": pr_url,
                        "comment_author": c.get("user", {}).get("login", ""),
                        "comment_body": c.get("body", ""),
                        "is_inline": False,
                        "file_path": None,
                    }
                )
            # Inline review comments
            inline = await gh.get_pr_review_comments(owner, repo_name, pr_number)
            for c in inline:
                reviews_list.append(
                    {
                        "pr_number": pr_number,
                        "repo": repo,
                        "pr_url": pr_url,
                        "comment_author": c.get("user", {}).get("login", ""),
                        "comment_body": c.get("body", ""),
                        "is_inline": True,
                        "file_path": c.get("path"),
                    }
                )
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

    # Get all submitted PRs and group by fork field
    all_prs = await mem.get_prs(limit=10000)
    prs_by_fork: dict[str, list[dict]] = {}
    for pr in all_prs:
        fork_field = pr.get("fork", "")
        if fork_field:
            prs_by_fork.setdefault(fork_field, []).append(pr)

    for fork in forks:
        fork_name = fork["full_name"]
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


# ── Entry point ────────────────────────────────────────────────────────────────


async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


if __name__ == "__main__":
    asyncio.run(main())
