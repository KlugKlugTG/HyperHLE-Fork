"""The data model for a collected fix request and its renderers."""
from __future__ import annotations

import re
from dataclasses import dataclass, field

# A log embedded in an issue body is truncated so a multi-megabyte paste can't
# blow past GitHub's body size limit.
MAX_LOG_CHARS = 20_000

_URL_RE = re.compile(r"https?://\S+")

# First line of every HyperHLE / touchHLE log, e.g.
#   touchHLE UNOFFICIAL 8d65eca — https://touchhle.org/
#   HyperHLE v1.0.2 — https://touchhle.org/
_LOG_HEADER_RE = re.compile(
    r"^(?:touchHLE|HyperHLE)\s+(?:UNOFFICIAL\s+)?(?P<ver>[\w.]+)\s+[—–-]",
    re.MULTILINE | re.IGNORECASE,
)
# Second line on Actions-built binaries, e.g.
#   Built from branch "trunk" of "HyperHLE/HyperHLE" by GitHub Actions
#   workflow run https://github.com/HyperHLE/HyperHLE/actions/runs/123.
_BUILT_FROM_RE = re.compile(
    r'Built from branch "(?P<branch>[^"]+)" of "(?P<repo>[^"]+)"'
    r"\s+by GitHub Actions workflow run\s+(?P<url>https?://\S+?)\.?\s*$",
    re.MULTILINE,
)
_HEX_RE = re.compile(r"[0-9a-fA-F]{7,40}")


@dataclass(frozen=True)
class LogBuildInfo:
    """Build identity extracted from a HyperHLE log header."""

    version: str = ""  # raw header version, e.g. "8d65eca" or "v1.0.2"
    commit: str = ""  # set only when the version is a commit hash
    branch: str = ""
    run_url: str = ""


def extract_build_info(text: str) -> LogBuildInfo:
    version = commit = branch = run_url = ""
    if (m := _LOG_HEADER_RE.search(text)):
        version = m.group("ver").strip()
        if _HEX_RE.fullmatch(version):
            commit = version.lower()
    if (m := _BUILT_FROM_RE.search(text)):
        branch = m.group("branch").strip()
        run_url = m.group("url").strip()
    return LogBuildInfo(version=version, commit=commit, branch=branch, run_url=run_url)


@dataclass
class LogFile:
    name: str
    content: str


@dataclass
class FixRequest:
    app_name: str = ""
    app_version: str = ""
    ipa_links: list[str] = field(default_factory=list)
    ipa_files: list[str] = field(default_factory=list)  # names of attached IPAs
    bug_description: str = ""
    logs: list[LogFile] = field(default_factory=list)
    media_note: str = ""  # text note about attached screenshots/video
    operating_system: str = ""
    gpu: str = ""

    # Filled in at submit time from the log header + the GitHub commits API.
    hyperhle_version: str = ""  # build from the user's log
    hyperhle_version_url: str = ""  # workflow run that built it, if known
    latest_commit_sha: str = ""  # head of the branch the log was built from
    up_to_date: bool | None = None  # None = could not verify

    # Who filed it, for attribution in the forwarded message / issue footer.
    reporter: str = ""

    def extract_links(self, text: str) -> list[str]:
        return _URL_RE.findall(text or "")

    @property
    def has_required(self) -> bool:
        return bool(
            self.app_name
            and (self.ipa_links or self.ipa_files)
            and self.bug_description
            and self.logs
        )

    def missing(self) -> list[str]:
        """Stable keys for whatever is still missing (localized by the UI)."""
        out = []
        if not self.app_name:
            out.append("app_name")
        if not (self.ipa_links or self.ipa_files):
            out.append("ipa")
        if not self.logs:
            out.append("logs")
        if not self.bug_description:
            out.append("bug")
        return out

    def _version_line(self) -> str:
        if not self.hyperhle_version:
            return "(no build hash found in the log)"
        if self.hyperhle_version_url:
            base = f"[`{self.hyperhle_version}`]({self.hyperhle_version_url})"
        else:
            base = f"`{self.hyperhle_version}`"
        if self.up_to_date is True:
            return base + " — ✅ matches the latest commit"
        if self.up_to_date is False:
            return (
                base
                + f" — ⚠️ **outdated**, latest is `{self.latest_commit_sha[:7]}`"
            )
        return base + " — could not verify against the latest commit"

    def _logs_block(self) -> str:
        parts = []
        for log in self.logs:
            content = log.content
            if len(content) > MAX_LOG_CHARS:
                content = content[:MAX_LOG_CHARS] + "\n… (truncated)"
            parts.append(
                f"<details><summary>{log.name}</summary>\n\n"
                f"```\n{content}\n```\n\n</details>"
            )
        return "\n\n".join(parts)

    def issue_title(self) -> str:
        name = self.app_name or "Unknown app"
        ver = f" {self.app_version}" if self.app_version else ""
        return f"[App fix] {name}{ver}"

    def _ipa_block(self) -> str:
        lines = [f"- {link}" for link in self.ipa_links]
        lines += [
            f"- `{name}` — IPA file attached via Telegram and forwarded to "
            "the maintainer (no public link)"
            for name in self.ipa_files
        ]
        return "\n".join(lines)

    def issue_body(self) -> str:
        ipa = self._ipa_block()
        lines = [
            f"**App / game:** {self.app_name}",
        ]
        if self.app_version:
            lines.append(f"**App version:** {self.app_version}")
        lines += [
            f"**HyperHLE build (from the log):** {self._version_line()}",
        ]
        if self.operating_system:
            lines.append(f"**Operating system:** {self.operating_system}")
        if self.gpu:
            lines.append(f"**GPU:** {self.gpu}")
        lines += [
            "",
            "### IPA link(s)",
            ipa,
            "",
            "### Bug / crash",
            self.bug_description,
            "",
            "### Log file(s)",
            self._logs_block() or "_none provided_",
        ]
        if self.media_note:
            lines += ["", "### Screenshots / video", self.media_note]
        lines += [
            "",
            "---",
            f"_Filed via the HyperHLE Telegram fix bot{(' by ' + self.reporter) if self.reporter else ''}._",
        ]
        return "\n".join(lines)

    def forward_text(self) -> str:
        """Plain-text summary for the maintainer DM."""
        ipa = "\n".join(
            [f"  • {link}" for link in self.ipa_links]
            + [f"  • {name} (file — forwarded below)" for name in self.ipa_files]
        )
        log_names = ", ".join(log.name for log in self.logs) or "none"
        if self.up_to_date is True:
            build_status = " (up to date)"
        elif self.up_to_date is False:
            build_status = f" (OUTDATED — latest is {self.latest_commit_sha[:7]})"
        else:
            build_status = " (unverified)"
        out = [
            "🛠️ New app fix request",
            f"App: {self.app_name}" + (f" {self.app_version}" if self.app_version else ""),
            f"HyperHLE build (from log): {self.hyperhle_version or 'unknown'}{build_status}",
        ]
        if self.operating_system:
            out.append(f"OS: {self.operating_system}")
        if self.gpu:
            out.append(f"GPU: {self.gpu}")
        out += [
            "",
            "IPA link(s):",
            ipa,
            "",
            f"Bug: {self.bug_description}",
            "",
            f"Logs attached: {log_names}",
        ]
        if self.media_note:
            out.append(f"Media: {self.media_note}")
        if self.reporter:
            out += ["", f"Reporter: {self.reporter}"]
        return "\n".join(out)
