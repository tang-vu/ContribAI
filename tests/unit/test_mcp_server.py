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
    async def test_returns_content_and_sha(self):
        from contribai.mcp_server import _get_file_content

        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content_with_sha = AsyncMock(return_value=("print('hello')", "abc123sha"))
            mock_get_gh.return_value = gh
            result = await _get_file_content({"owner": "o", "repo": "r", "path": "main.py"})
        data = _text(result)
        assert data["content"] == "print('hello')"
        assert data["sha"] == "abc123sha"

    @pytest.mark.asyncio
    async def test_passes_ref_param(self):
        from contribai.mcp_server import _get_file_content

        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.get_file_content_with_sha = AsyncMock(return_value=("x = 1", "def456sha"))
            mock_get_gh.return_value = gh
            await _get_file_content(
                {"owner": "o", "repo": "r", "path": "f.py", "ref": "fix-branch"}
            )
            gh.get_file_content_with_sha.assert_called_once_with("o", "r", "f.py", ref="fix-branch")


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
            result = await _create_branch(
                {"fork_owner": "me", "repo": "r", "branch_name": "fix-typo"}
            )
        data = _text(result)
        assert data["ref"] == "refs/heads/fix-typo"


class TestPushFileChange:
    @pytest.mark.asyncio
    async def test_returns_commit_sha(self):
        from contribai.mcp_server import _push_file_change

        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.create_or_update_file = AsyncMock(
                return_value={
                    "commit": {"sha": "abc123"},
                    "content": {"html_url": "https://github.com/me/r/blob/fix-typo/README.md"},
                }
            )
            mock_get_gh.return_value = gh
            result = await _push_file_change(
                {
                    "fork_owner": "me",
                    "repo": "r",
                    "branch": "fix-typo",
                    "path": "README.md",
                    "content": "# Fixed",
                    "commit_msg": "fix: typo",
                }
            )
        data = _text(result)
        assert data["commit_sha"] == "abc123"
        assert "README.md" in data["content_url"]


class TestCreatePR:
    @pytest.mark.asyncio
    async def test_returns_pr_info(self):
        from contribai.mcp_server import _create_pr

        with patch("contribai.mcp_server.get_github") as mock_get_gh:
            gh = AsyncMock()
            gh.create_pull_request = AsyncMock(
                return_value={"number": 42, "html_url": "https://github.com/owner/repo/pull/42"}
            )
            mock_get_gh.return_value = gh
            with patch("contribai.mcp_server.get_memory") as mock_get_mem:
                mem = AsyncMock()
                mock_get_mem.return_value = mem
                result = await _create_pr(
                    {
                        "owner": "owner",
                        "repo": "repo",
                        "title": "fix: typo",
                        "body": "Fixed a typo",
                        "head_branch": "me:fix-typo",
                    }
                )
        data = _text(result)
        assert data["pr_number"] == 42
        assert "pull/42" in data["pr_url"]
        mem.record_pr.assert_called_once_with(
            repo="owner/repo",
            pr_number=42,
            pr_url="https://github.com/owner/repo/pull/42",
            title="fix: typo",
            pr_type="mcp",
        )


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
            mem.get_repo_prs = AsyncMock(
                return_value=[{"status": "open", "pr_url": "https://github.com/o/r/pull/5"}]
            )
            mock_get_mem.return_value = mem
            result = await _check_duplicate_pr({"owner": "o", "repo": "r"})
        data = _text(result)
        assert data["is_duplicate"] is True
        assert "pull/5" in data["existing_pr_url"]


class TestCheckAIPolicy:
    @pytest.mark.asyncio
    async def test_not_banned_when_no_policy_file(self):
        from contribai.core.exceptions import GitHubAPIError
        from contribai.mcp_server import _check_ai_policy

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
            mem.get_stats = AsyncMock(
                return_value={
                    "total_repos_analyzed": 10,
                    "total_prs_submitted": 5,
                    "prs_merged": 3,
                }
            )
            mem.get_outcome_stats = AsyncMock(return_value={"avg_merge_rate": 0.6})
            mock_get_mem.return_value = mem
            result = await _get_stats({})
        data = _text(result)
        assert data["prs_submitted"] == 5
        assert data["merge_rate"] == "60%"


class TestPatrolPRs:
    @pytest.mark.asyncio
    async def test_returns_review_list(self):
        from contribai.mcp_server import _patrol_prs

        open_pr = {
            "repo": "owner/repo",
            "pr_number": 7,
            "pr_url": "https://github.com/owner/repo/pull/7",
        }
        with patch("contribai.mcp_server.get_memory") as mock_get_mem:
            mem = AsyncMock()
            mem.get_prs = AsyncMock(return_value=[open_pr])
            mock_get_mem.return_value = mem
            with patch("contribai.mcp_server.get_github") as mock_get_gh:
                gh = AsyncMock()
                gh.get_pr_comments = AsyncMock(
                    return_value=[
                        {"user": {"login": "maintainer"}, "body": "Please add tests", "id": 1}
                    ]
                )
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
        all_prs = [
            {"fork": "me/old-fork", "status": "merged", "repo": "upstream/repo", "pr_number": 1}
        ]
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
