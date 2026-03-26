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
