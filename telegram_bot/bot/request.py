"""The data model for a collected fix request and its renderers."""
from __future__ import annotations

import re
from dataclasses import dataclass, field

# A log embedded in an issue body is truncated so a multi-megabyte paste can't
# blow past GitHub's body size limit.
MAX_LOG_CHARS = 20_000

_URL_RE = re.compile(r"https?://\S+")


@dataclass
class LogFile:
    name: str
    content: str


@dataclass
class FixRequest:
    app_name: str = ""
    app_version: str = ""
    ipa_links: list[str] = field(default_factory=list)
    bug_description: str = ""
    logs: list[LogFile] = field(default_factory=list)
    media_note: str = ""  # text note about attached screenshots/video
    operating_system: str = ""
    gpu: str = ""

    # Filled in at submit time from GitHub Actions.
    hyperhle_version: str = ""
    hyperhle_version_url: str = ""

    # Who filed it, for attribution in the forwarded message / issue footer.
    reporter: str = ""

    def extract_links(self, text: str) -> list[str]:
        return _URL_RE.findall(text or "")

    @property
    def has_required(self) -> bool:
        return bool(self.app_name and self.ipa_links and self.bug_description and self.logs)

    def missing(self) -> list[str]:
        out = []
        if not self.app_name:
            out.append("app name")
        if not self.ipa_links:
            out.append("IPA link(s)")
        if not self.logs:
            out.append("log file(s)")
        if not self.bug_description:
            out.append("bug/crash description")
        return out

    def _version_line(self) -> str:
        if self.hyperhle_version and self.hyperhle_version_url:
            return f"[{self.hyperhle_version}]({self.hyperhle_version_url})"
        return self.hyperhle_version or "(latest from Actions)"

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

    def issue_body(self) -> str:
        ipa = "\n".join(f"- {link}" for link in self.ipa_links)
        lines = [
            f"**App / game:** {self.app_name}",
        ]
        if self.app_version:
            lines.append(f"**App version:** {self.app_version}")
        lines += [
            f"**HyperHLE version tested (latest from Actions):** {self._version_line()}",
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
        ipa = "\n".join(f"  • {link}" for link in self.ipa_links)
        log_names = ", ".join(log.name for log in self.logs) or "none"
        out = [
            "🛠️ New app fix request",
            f"App: {self.app_name}" + (f" {self.app_version}" if self.app_version else ""),
            f"HyperHLE build: {self.hyperhle_version or '(latest)'}",
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
