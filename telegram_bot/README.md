# HyperHLE app-fix Telegram bot

A Telegram bot that lets people **request an app be fixed in HyperHLE** without
touching GitHub. It walks the user through the three things a fix actually needs:

1. the **IPA link(s)** (a direct download for the `.ipa` / zipped `.app`) —
   or the **IPA file itself**, attached in Telegram (it is forwarded to the
   maintainer, and the GitHub issue notes the file was attached via Telegram),
2. a **log file** from the failing run, and
3. a description of the **bug/crash** (with optional screenshots/video).

When the request is complete the bot:

- **verifies the build from the log itself**: it reads the build hash out of
  the log header and compares it with the latest commit on the branch the log
  was built from, warning the user (and flagging the issue) if their build is
  outdated relative to
  [Actions](https://github.com/HyperHLE/HyperHLE/actions),
- opens a GitHub issue using the same fields as the
  [`Request an app fix (IPA + logs)`](../.github/ISSUE_TEMPLATE/app_fix_request.yml)
  issue template, and
- **forwards** the whole request (and any screenshots/video) to the maintainer
  **[@Tog991](https://t.me/Tog991)** on Telegram.

Logs are validated as they come in: attached files must be plain-text
`.txt`/`.log` files, and empty logs (attached or pasted) are rejected.

## Conversation flow

The bot speaks **English, Russian and Arabic**. The first `/start` begins with a
language picker; the choice is saved (and survives bot restarts via a pickle
file, see `PERSISTENCE_FILE`), so later `/start` runs skip straight to the
questions. `/language` re-opens the picker at any time, even mid-request.
(The GitHub issue and the maintainer forward stay in English.)

```
/start
 → 🇺🇸 English / 🇷🇺 Русский / 🇸🇦 العربية   (first time only — /language to change later)
 → app name
 → app version            (/skip)
 → IPA link(s) or file(s) (required — http(s) URLs, or attach the .ipa/.zip itself, then /done)
 → bug / crash            (required)
 → log file(s)            (required — attach a file or paste text, then /done)
 → OS / GPU               (/skip)
 → screenshots / video    (optional, then /done)
 → review → ✅ Submit
```

`/cancel` aborts at any point. `/help` explains the commands.

## Setup

```bash
cd telegram_bot
python -m venv .venv
source .venv/bin/activate
pip install -e .

cp .env.example .env
# edit .env (see below), then:
python -m bot.main
```

### Configuration (`.env`)

| Variable | Required | Purpose |
| --- | --- | --- |
| `TELEGRAM_BOT_TOKEN` | yes | Bot token from [@BotFather](https://t.me/BotFather). |
| `FORWARD_CHAT_ID` | for forwarding | Numeric chat id to forward requests to (Tog991). |
| `FORWARD_USERNAME` | no | Display handle, defaults to `@Tog991`. |
| `GITHUB_TOKEN` | for auto-filing | Token with `repo` / `issues:write` scope. |
| `GITHUB_OWNER` / `GITHUB_REPO` | no | Defaults to `HyperHLE` / `HyperHLE`. |
| `GITHUB_ISSUE_LABELS` | no | Comma-separated labels, default `app fix request`. |
| `PERSISTENCE_FILE` | no | Pickle file for saved user languages, default `bot_state.pickle` next to the project. |

#### Forwarding to @Tog991

Telegram bots can't DM a username they've never met, so forwarding uses a
numeric chat id. Have **@Tog991 send the bot any message once**, then read the
`chat_id` from the bot's logs (or forward one of their messages to
[@userinfobot](https://t.me/userinfobot)) and put it in `FORWARD_CHAT_ID`.

#### GitHub token

With `GITHUB_TOKEN` set, the bot opens the issue directly and replies with the
link. Without it, the bot still forwards to the maintainer and replies with a
**prefilled "new issue" link** the user can click to file it themselves. A
token also lifts the anonymous API rate limit on the latest-commit check.

## How the "latest version" check works

HyperHLE logs identify their own build in the first two lines:

```
touchHLE UNOFFICIAL 8d65eca — https://touchhle.org/
Built from branch "trunk" of "HyperHLE/HyperHLE" by GitHub Actions workflow run https://github.com/HyperHLE/HyperHLE/actions/runs/27085497648.
```

At submit time `bot/request.py` parses the commit hash, branch and workflow-run
URL out of the uploaded log, and `bot/github_client.py` fetches
`GET /repos/{owner}/{repo}/commits/{branch}` to get the branch head. If the
log's hash is a prefix of the head SHA the build is **up to date**; otherwise
the user is warned to grab the newest build from Actions (the request is still
submitted), and the GitHub issue and the maintainer forward both show
`outdated — latest is <sha>`. If the log has no hash (e.g. a release build like
`v1.0.2`) or the API can't be reached, the build is reported as unverified
rather than blocking the request.

## Layout

```
telegram_bot/
├── pyproject.toml
├── .env.example
├── README.md
└── bot/
    ├── config.py          # env-driven configuration + .env loader
    ├── github_client.py   # latest-commit lookup + issue creation
    ├── i18n.py            # English / Russian / Arabic strings
    ├── request.py         # FixRequest model + issue/forward renderers
    ├── conversation.py    # the /start ConversationHandler
    └── main.py            # entry point (run_polling)
```
