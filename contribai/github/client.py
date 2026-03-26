"""Async GitHub API client.

Handles all GitHub REST API interactions: repo metadata,
file content, forking, branching, committing, and PR creation.
"""

from __future__ import annotations

import base64
import logging
from typing import Any

import httpx

from contribai.core.exceptions import GitHubAPIError, RateLimitError
from contribai.core.models import FileNode, Issue, Repository

logger = logging.getLogger(__name__)

GITHUB_API = "https://api.github.com"


class GitHubClient:
    """Async GitHub REST API client."""

    def __init__(self, token: str, rate_limit_buffer: int = 100):
        self._token = token
        self._rate_limit_buffer = rate_limit_buffer
        self._client = httpx.AsyncClient(
            base_url=GITHUB_API,
            headers={
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github+json",
                "X-GitHub-Api-Version": "2022-11-28",
            },
            timeout=30.0,
        )

    async def close(self):
        await self._client.aclose()

    # ── Core HTTP ──────────────────────────────────────────────────────────

    async def _request(self, method: str, url: str, *, _retries: int = 3, **kwargs) -> Any:
        """Make an authenticated GitHub API request with error handling and retry."""
        import asyncio

        last_error = None
        for attempt in range(1, _retries + 1):
            try:
                response = await self._client.request(method, url, **kwargs)
            except httpx.HTTPError as e:
                raise GitHubAPIError(f"HTTP error: {e}") from e

            if response.status_code == 403:
                remaining = response.headers.get("x-ratelimit-remaining", "?")
                reset = response.headers.get("x-ratelimit-reset")
                if remaining == "0":
                    raise RateLimitError(reset_at=int(reset) if reset else None)
                raise GitHubAPIError(f"Forbidden: {response.text}", status_code=403)

            if response.status_code == 404:
                raise GitHubAPIError(f"Not found: {url}", status_code=404)

            # Retry on 5xx server errors (502, 503, 504)
            if response.status_code >= 500:
                last_error = GitHubAPIError(
                    f"GitHub API error {response.status_code}: {response.text}",
                    status_code=response.status_code,
                )
                if attempt < _retries:
                    wait = 2**attempt  # 2s, 4s, 8s
                    logger.warning(
                        "GitHub %d error on %s %s, retrying in %ds (attempt %d/%d)",
                        response.status_code,
                        method,
                        url,
                        wait,
                        attempt,
                        _retries,
                    )
                    await asyncio.sleep(wait)
                    continue
                raise last_error

            if response.status_code >= 400:
                raise GitHubAPIError(
                    f"GitHub API error {response.status_code}: {response.text}",
                    status_code=response.status_code,
                )

            return response.json() if response.content else None

        raise last_error  # should never reach here

    async def _get(self, url: str, **kwargs) -> Any:
        return await self._request("GET", url, **kwargs)

    async def _post(self, url: str, **kwargs) -> Any:
        return await self._request("POST", url, **kwargs)

    async def _put(self, url: str, **kwargs) -> Any:
        return await self._request("PUT", url, **kwargs)

    async def _delete(self, url: str, **kwargs) -> Any:
        return await self._request("DELETE", url, **kwargs)

    # ── Rate Limit ─────────────────────────────────────────────────────────

    async def check_rate_limit(self) -> dict:
        """Check current rate limit status."""
        data = await self._get("/rate_limit")
        core = data["resources"]["core"]
        logger.info(
            "Rate limit: %d/%d remaining (resets at %s)",
            core["remaining"],
            core["limit"],
            core["reset"],
        )
        return core

    async def _ensure_rate_limit(self):
        """Ensure we have enough API calls remaining."""
        core = await self.check_rate_limit()
        if core["remaining"] < self._rate_limit_buffer:
            raise RateLimitError(
                reset_at=core["reset"],
                details={"remaining": core["remaining"], "buffer": self._rate_limit_buffer},
            )

    # ── Repository Operations ──────────────────────────────────────────────

    async def search_repositories(
        self,
        query: str,
        sort: str = "stars",
        order: str = "desc",
        per_page: int = 30,
    ) -> list[Repository]:
        """Search GitHub repositories."""
        data = await self._get(
            "/search/repositories",
            params={"q": query, "sort": sort, "order": order, "per_page": per_page},
        )
        return [self._parse_repo(item) for item in data.get("items", [])]

    async def get_repo_details(self, owner: str, repo: str) -> Repository:
        """Get detailed repository information."""
        data = await self._get(f"/repos/{owner}/{repo}")
        return self._parse_repo(data)

    async def get_file_tree(
        self, owner: str, repo: str, branch: str | None = None
    ) -> list[FileNode]:
        """Get the full file tree of a repository."""
        if not branch:
            details = await self.get_repo_details(owner, repo)
            branch = details.default_branch

        data = await self._get(
            f"/repos/{owner}/{repo}/git/trees/{branch}",
            params={"recursive": "1"},
        )
        return [
            FileNode(
                path=item["path"],
                type=item["type"],
                size=item.get("size", 0),
                sha=item["sha"],
            )
            for item in data.get("tree", [])
        ]

    async def get_file_content(
        self, owner: str, repo: str, path: str, ref: str | None = None
    ) -> str:
        """Get the content of a file from the repository."""
        params = {"ref": ref} if ref else None
        data = await self._get(f"/repos/{owner}/{repo}/contents/{path}", params=params)
        if data.get("encoding") == "base64":
            return base64.b64decode(data["content"]).decode("utf-8")
        return data.get("content", "")

    async def get_file_content_with_sha(
        self, owner: str, repo: str, path: str, ref: str | None = None
    ) -> tuple[str, str]:
        """Get file content and blob SHA (needed for subsequent updates).

        Returns:
            (content, sha) where sha is the blob SHA required by push_file_change.
        """
        params = {"ref": ref} if ref else None
        data = await self._get(f"/repos/{owner}/{repo}/contents/{path}", params=params)
        if data.get("encoding") == "base64":
            content = base64.b64decode(data["content"]).decode("utf-8")
        else:
            content = data.get("content", "")
        return content, data.get("sha", "")

    async def get_open_issues(
        self, owner: str, repo: str, per_page: int = 30, labels: str | None = None
    ) -> list[Issue]:
        """Get open issues for a repository."""
        params: dict[str, Any] = {"state": "open", "per_page": per_page}
        if labels:
            params["labels"] = labels

        data = await self._get(f"/repos/{owner}/{repo}/issues", params=params)
        return [
            Issue(
                number=item["number"],
                title=item["title"],
                body=item.get("body"),
                labels=[lbl["name"] for lbl in item.get("labels", [])],
                state=item["state"],
                html_url=item["html_url"],
            )
            for item in data
            if "pull_request" not in item  # exclude PRs from issues
        ]

    async def get_contributing_guide(self, owner: str, repo: str) -> str | None:
        """Try to fetch CONTRIBUTING.md."""
        for path in ["CONTRIBUTING.md", "contributing.md", ".github/CONTRIBUTING.md"]:
            try:
                return await self.get_file_content(owner, repo, path)
            except GitHubAPIError:
                continue
        return None

    # ── Fork & Branch ──────────────────────────────────────────────────────

    async def fork_repository(self, owner: str, repo: str) -> Repository:
        """Fork a repository to the authenticated user's account."""
        data = await self._post(f"/repos/{owner}/{repo}/forks")
        logger.info("Forked %s/%s → %s", owner, repo, data["full_name"])
        return self._parse_repo(data)

    async def create_branch(
        self, owner: str, repo: str, branch_name: str, from_branch: str | None = None
    ) -> dict:
        """Create a new branch from the default or specified branch."""
        if not from_branch:
            details = await self.get_repo_details(owner, repo)
            from_branch = details.default_branch

        # Get the SHA of the source branch
        ref_data = await self._get(f"/repos/{owner}/{repo}/git/ref/heads/{from_branch}")
        sha = ref_data["object"]["sha"]

        data = await self._post(
            f"/repos/{owner}/{repo}/git/refs",
            json={"ref": f"refs/heads/{branch_name}", "sha": sha},
        )
        logger.info("Created branch %s on %s/%s", branch_name, owner, repo)
        return data

    # ── Commit & PR ────────────────────────────────────────────────────────

    async def create_or_update_file(
        self,
        owner: str,
        repo: str,
        path: str,
        content: str,
        message: str,
        branch: str,
        sha: str | None = None,
        signoff: str | None = None,
    ) -> dict:
        """Create or update a file in the repository.

        Args:
            signoff: If provided, appends ``Signed-off-by: <signoff>`` to the
                     commit message for DCO compliance.  Value should be
                     ``"Name <email>"``.
        """
        # Append DCO signoff trailer if requested
        if signoff and "Signed-off-by:" not in message:
            message = f"{message}\n\nSigned-off-by: {signoff}"

        encoded = base64.b64encode(content.encode("utf-8")).decode("utf-8")
        payload: dict[str, Any] = {
            "message": message,
            "content": encoded,
            "branch": branch,
        }
        if sha:
            payload["sha"] = sha

        return await self._put(f"/repos/{owner}/{repo}/contents/{path}", json=payload)

    async def create_pull_request(
        self,
        owner: str,
        repo: str,
        title: str,
        body: str,
        head: str,
        base: str | None = None,
    ) -> dict:
        """Create a pull request."""
        if not base:
            details = await self.get_repo_details(owner, repo)
            base = details.default_branch

        data = await self._post(
            f"/repos/{owner}/{repo}/pulls",
            json={"title": title, "body": body, "head": head, "base": base},
        )
        logger.info("Created PR #%d on %s/%s: %s", data["number"], owner, repo, title)
        return data

    async def update_pull_request(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        *,
        title: str | None = None,
        body: str | None = None,
    ) -> dict:
        """Update a PR's title and/or body."""
        payload: dict[str, str] = {}
        if title is not None:
            payload["title"] = title
        if body is not None:
            payload["body"] = body
        data = await self._request(
            "PATCH", f"/repos/{owner}/{repo}/pulls/{pr_number}", json=payload
        )
        logger.info("Updated PR #%d on %s/%s", pr_number, owner, repo)
        return data

    async def create_issue(
        self,
        owner: str,
        repo: str,
        title: str,
        body: str,
        labels: list[str] | None = None,
    ) -> dict:
        """Create an issue on a repository."""
        payload: dict[str, Any] = {"title": title, "body": body}
        if labels:
            payload["labels"] = labels
        data = await self._post(f"/repos/{owner}/{repo}/issues", json=payload)
        logger.info("Created issue #%d on %s/%s: %s", data["number"], owner, repo, title)
        return data

    async def get_pr_comments(self, owner: str, repo: str, pr_number: int) -> list[dict]:
        """Get comments on a pull request (issue comments)."""
        return await self._get(f"/repos/{owner}/{repo}/issues/{pr_number}/comments")

    async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> dict:
        """Post a comment on a pull request."""
        return await self._post(
            f"/repos/{owner}/{repo}/issues/{pr_number}/comments",
            json={"body": body},
        )

    # ── PR Reviews ─────────────────────────────────────────────────────────

    async def get_pr_reviews(self, owner: str, repo: str, pr_number: int) -> list[dict]:
        """Get reviews on a pull request (APPROVED, CHANGES_REQUESTED, COMMENTED)."""
        return await self._get(f"/repos/{owner}/{repo}/pulls/{pr_number}/reviews")

    async def get_pr_review_comments(self, owner: str, repo: str, pr_number: int) -> list[dict]:
        """Get inline code-level review comments on a pull request."""
        return await self._get(f"/repos/{owner}/{repo}/pulls/{pr_number}/comments")

    async def create_pr_review_comment_reply(
        self, owner: str, repo: str, pr_number: int, comment_id: int, body: str
    ) -> dict:
        """Reply to an inline review comment on a PR."""
        return await self._post(
            f"/repos/{owner}/{repo}/pulls/{pr_number}/comments/{comment_id}/replies",
            json={"body": body},
        )

    async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
        """Get the diff of a pull request."""
        resp = await self._client.get(
            f"/repos/{owner}/{repo}/pulls/{pr_number}",
            headers={"Accept": "application/vnd.github.v3.diff"},
        )
        resp.raise_for_status()
        return resp.text

    async def get_authenticated_user(self) -> dict:
        """Get the authenticated user's profile."""
        return await self._get("/user")

    # ── Helpers ────────────────────────────────────────────────────────────

    async def list_pull_requests(
        self,
        owner: str,
        repo: str,
        *,
        state: str = "all",
        per_page: int = 30,
    ) -> list[dict]:
        """List pull requests on a repository.

        Args:
            owner: Repo owner
            repo: Repo name
            state: 'open', 'closed', or 'all'
            per_page: Number of PRs to return (max 100)
        """
        return await self._get(
            f"/repos/{owner}/{repo}/pulls",
            params={
                "state": state,
                "per_page": min(per_page, 100),
                "sort": "created",
                "direction": "desc",
            },
        )

    # ── Issues ─────────────────────────────────────────────────────────────

    async def list_issues(
        self,
        owner: str,
        repo: str,
        *,
        labels: list[str] | None = None,
        state: str = "open",
        per_page: int = 30,
        assignee: str = "none",
    ) -> list[dict]:
        """List issues on a repository.

        Args:
            owner: Repo owner
            repo: Repo name
            labels: Comma-separated label names to filter by
            state: 'open', 'closed', or 'all'
            per_page: Number of issues to return (max 100)
            assignee: 'none' to get unassigned issues only, '*' for all
        """
        params: dict = {
            "state": state,
            "per_page": min(per_page, 100),
            "sort": "created",
            "direction": "desc",
        }
        if labels:
            params["labels"] = ",".join(labels)
        if assignee:
            params["assignee"] = assignee

        results = await self._get(f"/repos/{owner}/{repo}/issues", params=params)

        # GitHub's issues endpoint also returns PRs — filter them out
        return [issue for issue in results if "pull_request" not in issue]

    async def get_assigned_issues(
        self,
        owner: str,
        repo: str,
        username: str,
    ) -> list[dict]:
        """Get open issues assigned to a specific user.

        Args:
            owner: Repo owner
            repo: Repo name
            username: GitHub login to check assignments for
        """
        return await self.list_issues(
            owner,
            repo,
            assignee=username,
            state="open",
            per_page=20,
        )

    async def get_issue_comments(self, owner: str, repo: str, issue_number: int) -> list[dict]:
        """Get comments on an issue."""
        return await self._get(f"/repos/{owner}/{repo}/issues/{issue_number}/comments")

    async def get_issue_timeline(self, owner: str, repo: str, issue_number: int) -> list[dict]:
        """Get timeline events for an issue (includes cross-references to PRs)."""
        try:
            return await self._get(
                f"/repos/{owner}/{repo}/issues/{issue_number}/timeline",
            )
        except Exception:
            return []

    # ── CI / Check Runs ────────────────────────────────────────────────────

    async def get_combined_status(self, owner: str, repo: str, ref: str) -> dict:
        """Get combined CI status for a commit ref.

        Returns dict with 'state' (success/failure/pending) and 'statuses'.
        Also fetches check runs for GitHub Actions.
        """
        # Get check suites/runs (GitHub Actions)
        try:
            checks = await self._get(
                f"/repos/{owner}/{repo}/commits/{ref}/check-runs",
                params={"per_page": 100},
            )
        except Exception:
            checks = {"check_runs": []}

        runs = checks.get("check_runs", [])
        if not runs:
            return {"state": "pending", "total": 0, "failed": [], "passed": []}

        failed = [r["name"] for r in runs if r.get("conclusion") == "failure"]
        passed = [r["name"] for r in runs if r.get("conclusion") == "success"]
        in_progress = [r["name"] for r in runs if r.get("status") in ("queued", "in_progress")]

        if in_progress:
            state = "pending"
        elif failed:
            state = "failure"
        else:
            state = "success"

        return {
            "state": state,
            "total": len(runs),
            "failed": failed,
            "passed": passed,
            "in_progress": in_progress,
        }

    async def close_pull_request(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        *,
        comment: str | None = None,
    ) -> None:
        """Close a PR with an optional comment explaining why."""
        if comment:
            await self._post(
                f"/repos/{owner}/{repo}/issues/{pr_number}/comments",
                json={"body": comment},
            )
        await self._request(
            "PATCH",
            f"/repos/{owner}/{repo}/pulls/{pr_number}",
            json={"state": "closed"},
        )
        logger.info("Closed PR #%d on %s/%s", pr_number, owner, repo)

    async def list_user_forks(self) -> list[dict]:
        """List all forks owned by the authenticated user."""
        return await self._get("/user/repos", params={"type": "fork", "per_page": "100"})

    async def delete_repository(self, owner: str, repo: str) -> None:
        """Delete a repository (must be owner or have admin access)."""
        await self._delete(f"/repos/{owner}/{repo}")

    @staticmethod
    def _parse_repo(data: dict) -> Repository:
        """Parse raw API response into Repository model."""
        owner = data.get("owner", {})
        return Repository(
            owner=owner.get("login", ""),
            name=data.get("name", ""),
            full_name=data.get("full_name", ""),
            description=data.get("description"),
            language=data.get("language"),
            stars=data.get("stargazers_count", 0),
            forks=data.get("forks_count", 0),
            open_issues=data.get("open_issues_count", 0),
            topics=data.get("topics", []),
            default_branch=data.get("default_branch", "main"),
            html_url=data.get("html_url", ""),
            clone_url=data.get("clone_url", ""),
            has_license=data.get("license") is not None,
        )
