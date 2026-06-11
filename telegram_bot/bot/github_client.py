"""Thin async GitHub REST client for the bot.

Two responsibilities:

* Report the **latest HyperHLE build from Actions** so every request is pinned
  to the newest version (preferring the latest successful build-workflow run,
  falling back to the latest published release).
* Open an issue from a collected fix request.

All methods degrade gracefully: if there is no token, or the network call
fails, the bot keeps working (it just can't auto-file an issue and falls back
to a prefilled "new issue" link).
"""
from __future__ import annotations

import urllib.parse
from dataclasses import dataclass

import httpx

API_ROOT = "https://api.github.com"


@dataclass(frozen=True)
class LatestBuild:
    """A pointer to the newest HyperHLE build a request should target."""

    label: str  # human string, e.g. "v1.0.2" or "trunk @ abcdef0"
    url: str  # link to the run or release


@dataclass(frozen=True)
class CreatedIssue:
    number: int
    url: str


class GitHubClient:
    def __init__(
        self,
        owner: str,
        repo: str,
        token: str | None,
        build_workflow: str,
    ) -> None:
        self._owner = owner
        self._repo = repo
        self._token = token
        self._build_workflow = build_workflow

    def _headers(self) -> dict[str, str]:
        headers = {
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "hyperhle-fix-bot",
        }
        if self._token:
            headers["Authorization"] = f"Bearer {self._token}"
        return headers

    @property
    def can_open_issues(self) -> bool:
        return self._token is not None

    async def latest_build(self) -> LatestBuild | None:
        """Best-effort newest build, preferring a successful Actions run."""
        run = await self._latest_successful_build_run()
        if run is not None:
            return run
        return await self._latest_release()

    async def _latest_successful_build_run(self) -> LatestBuild | None:
        path = (
            f"/repos/{self._owner}/{self._repo}/actions/workflows/"
            f"{urllib.parse.quote(self._build_workflow)}/runs"
        )
        params = {"status": "success", "per_page": "1"}
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                resp = await client.get(
                    API_ROOT + path, headers=self._headers(), params=params
                )
            if resp.status_code != 200:
                return None
            runs = resp.json().get("workflow_runs") or []
            if not runs:
                return None
            run = runs[0]
            sha = (run.get("head_sha") or "")[:7]
            branch = run.get("head_branch") or "?"
            return LatestBuild(label=f"{branch} @ {sha}", url=run.get("html_url", ""))
        except (httpx.HTTPError, ValueError, KeyError):
            return None

    async def _latest_release(self) -> LatestBuild | None:
        path = f"/repos/{self._owner}/{self._repo}/releases/latest"
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                resp = await client.get(API_ROOT + path, headers=self._headers())
            if resp.status_code != 200:
                return None
            data = resp.json()
            tag = data.get("tag_name") or "latest"
            return LatestBuild(label=tag, url=data.get("html_url", ""))
        except (httpx.HTTPError, ValueError, KeyError):
            return None

    async def create_issue(
        self, title: str, body: str, labels: list[str]
    ) -> CreatedIssue | None:
        if not self._token:
            return None
        path = f"/repos/{self._owner}/{self._repo}/issues"
        payload: dict[str, object] = {"title": title, "body": body}
        if labels:
            payload["labels"] = labels
        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.post(
                    API_ROOT + path, headers=self._headers(), json=payload
                )
            if resp.status_code not in (200, 201):
                return None
            data = resp.json()
            return CreatedIssue(number=data["number"], url=data["html_url"])
        except (httpx.HTTPError, ValueError, KeyError):
            return None

    def new_issue_link(self, title: str, body: str) -> str:
        """A prefilled GitHub "new issue" URL (fallback when no token)."""
        query = urllib.parse.urlencode(
            {
                "template": "app_fix_request.yml",
                "title": title,
                "body": body,
            }
        )
        return f"https://github.com/{self._owner}/{self._repo}/issues/new?{query}"
