# HyperHLE app-fix Telegram bot

A Telegram bot that lets people **request an app be fixed in HyperHLE** without
touching GitHub. It walks the user through the three things a fix actually needs:

1. the **IPA link(s)** (a direct download for the `.ipa` / zipped `.app`),
2. a **log file** from the failing run, and
3. a description of the **bug/crash** (with optional screenshots/video).

When the request is complete the bot:

- pins it to the **latest HyperHLE build from
  [Actions](https://github.com/HyperHLE/HyperHLE/actions)** (it queries the
  GitHub API for the newest successful *Build HyperHLE* run, falling back to the
  latest release),
- opens a GitHub issue using the same fields as the
  [`Request an app fix (IPA + logs)`](../.github/ISSUE_TEMPLATE/app_fix_request.yml)
  issue template, and
- **forwards** the whole request (and any screenshots/video) to the maintainer
  **[@Tog991](https://t.me/Tog991)** on Telegram.

## Conversation flow

The bot speaks **English and Russian**. The first `/start` begins with a
language picker; the choice is saved (and survives bot restarts via a pickle
file, see `PERSISTENCE_FILE`), so later `/start` runs skip straight to the
questions. `/language` re-opens the picker at any time, even mid-request.
(The GitHub issue and the maintainer forward stay in English.)

```
/start
 → 🇬🇧 English / 🇷🇺 Русский   (first time only — /language to change later)
 → app name
 → app version            (/skip)
 → IPA link(s)            (required — one or more http(s) URLs)
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
| `GITHUB_BUILD_WORKFLOW` | no | Build workflow file, default `HyperHLE_release.yml`. |
| `PERSISTENCE_FILE` | no | Pickle file for saved user languages, default `bot_state.pickle` next to the project. |

#### Forwarding to @Tog991

Telegram bots can't DM a username they've never met, so forwarding uses a
numeric chat id. Have **@Tog991 send the bot any message once**, then read the
`chat_id` from the bot's logs (or forward one of their messages to
[@userinfobot](https://t.me/userinfobot)) and put it in `FORWARD_CHAT_ID`.

#### GitHub token

With `GITHUB_TOKEN` set, the bot opens the issue directly and replies with the
link. Without it, the bot still forwards to the maintainer and replies with a
**prefilled "new issue" link** the user can click to file it themselves.

## How "latest version from Actions" is resolved

`bot/github_client.py` calls
`GET /repos/{owner}/{repo}/actions/workflows/{build}/runs?status=success&per_page=1`
and reports `branch @ shortsha` linked to that run. If no successful run is
visible (or there's no token for private data), it falls back to
`GET /repos/{owner}/{repo}/releases/latest`. The resolved build is written into
both the GitHub issue and the maintainer forward, so every request is anchored
to the newest build rather than whatever the user happened to have.

## Layout

```
telegram_bot/
├── pyproject.toml
├── .env.example
├── README.md
└── bot/
    ├── config.py          # env-driven configuration + .env loader
    ├── github_client.py   # latest-build lookup + issue creation
    ├── i18n.py            # English / Russian strings
    ├── request.py         # FixRequest model + issue/forward renderers
    ├── conversation.py    # the /start ConversationHandler
    └── main.py            # entry point (run_polling)
```
