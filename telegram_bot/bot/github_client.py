"""Thin async GitHub REST client for the bot.

Two responsibilities:

* Look up the **latest commit** on a branch, so the build hash found in the
  user's log can be checked against the newest code (HyperHLE logs start with
  ``touchHLE UNOFFICIAL <shortsha> — …`` and name the branch they were built
  from).
* Open an issue from a collected fix request.

All methods degrade gracefully: if there is no token, or the network call
fails, the bot keeps working (the build check is reported as "could not
verify" and issue filing falls back to a prefilled "new issue" link).
"""
from __future__ import annotations

import urllib.parse
from dataclasses import dataclass

import httpx

API_ROOT = "https://api.github.com"


@dataclass(frozen=True)
class LatestCommit:
    sha: str
    url: str


@dataclass(frozen=True)
class CreatedIssue:
    number: int
    url: str


class GitHubClient:
    def __init__(self, owner: str, repo: str, token: str | None) -> None:
        self._owner = owner
        self._repo = repo
        self._token = token

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

    async def latest_commit(self, branch: str = "trunk") -> LatestCommit | None:
        """Head commit of `branch`, or None if it can't be fetched."""
        path = (
            f"/repos/{self._owner}/{self._repo}/commits/"
            f"{urllib.parse.quote(branch)}"
        )
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                resp = await client.get(API_ROOT + path, headers=self._headers())
            if resp.status_code != 200:
                return None
            data = resp.json()
            return LatestCommit(sha=data["sha"], url=data.get("html_url", ""))
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
