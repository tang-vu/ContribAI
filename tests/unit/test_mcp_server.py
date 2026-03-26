"""Tests for ContribAI MCP server tool implementations."""
import json
from unittest.mock import AsyncMock, MagicMock, patch

import mcp.types as types
import pytest


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
            result = await _search_repos(
                {"language": "python", "stars_min": 100, "stars_max": 5000, "limit": 5}
            )

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
            await _get_file_content(
                {"owner": "o", "repo": "r", "path": "f.py", "ref": "fix-branch"}
            )
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
            gh.get_file_tree = AsyncMock(return_value=nodes)
            mock_get_gh.return_value = gh
            result = await _get_file_tree({"owner": "o", "repo": "r", "max_files": 10})
        data = _text(result)
        assert len(data["files"]) == 10
        assert data["total"] == 500

    @pytest.mark.asyncio
    async def test_excludes_tree_nodes(self):
        from contribai.mcp_server import _get_file_tree
        nodes = [
            MagicMock(path="src/", type="tree"),
            MagicMock(path="src/main.py", type="blob"),
            MagicMock(path="src/utils/", type="tree"),
            MagicMock(path="src/utils/helper.py", type="blob"),
        ]
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_tree = AsyncMock(return_value=nodes)
            mock_get_gh.return_value = gh
            result = await _get_file_tree({"owner": "o", "repo": "r"})
        data = _text(result)
        assert len(data["files"]) == 2
        assert data["total"] == 2
        assert "src/" not in data["files"]
        assert "src/utils/" not in data["files"]


class TestGetRepoInfo:
    @pytest.mark.asyncio
    async def test_returns_repo_metadata(self):
        from contribai.mcp_server import _get_repo_info
        mock_repo = MagicMock()
        mock_repo.full_name = "owner/repo"
        mock_repo.stars = 500
        mock_repo.language = "Python"
        mock_repo.open_issues = 10
        mock_repo.default_branch = "main"
        mock_repo.description = "A repo"
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_repo_details = AsyncMock(return_value=mock_repo)
            mock_get_gh.return_value = gh
            result = await _get_repo_info({"owner": "owner", "repo": "repo"})
        data = _text(result)
        assert data["stars"] == 500
        assert data["default_branch"] == "main"
        assert data["open_issues"] == 10


class TestGetOpenIssues:
    @pytest.mark.asyncio
    async def test_returns_issue_list(self):
        from contribai.mcp_server import _get_open_issues
        mock_issue = MagicMock()
        mock_issue.number = 1
        mock_issue.title = "Bug report"
        mock_issue.body = "Something is broken"
        mock_issue.labels = ["bug"]
        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_open_issues = AsyncMock(return_value=[mock_issue])
            mock_get_gh.return_value = gh
            result = await _get_open_issues({"owner": "o", "repo": "r", "limit": 5})
        data = _text(result)
        assert "issues" in data
        assert data["issues"][0]["number"] == 1
        assert data["issues"][0]["labels"] == ["bug"]
