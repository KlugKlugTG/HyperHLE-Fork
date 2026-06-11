"""The /fix conversation: collect a request, file it, forward it."""
from __future__ import annotations

import logging

from telegram import (
    InlineKeyboardButton,
    InlineKeyboardMarkup,
    ReplyKeyboardMarkup,
    ReplyKeyboardRemove,
    Update,
)
from telegram.constants import ParseMode
from telegram.ext import (
    Application,
    CallbackQueryHandler,
    CommandHandler,
    ContextTypes,
    ConversationHandler,
    MessageHandler,
    filters,
)

from .config import Config
from .github_client import GitHubClient
from .request import FixRequest, LogFile

log = logging.getLogger(__name__)

# Conversation states.
APP_NAME, APP_VERSION, IPA_LINKS, BUG, LOGS, ENV, MEDIA, CONFIRM = range(8)

# Telegram bot API caps downloads at 20 MB; logs above that are rejected.
MAX_LOG_BYTES = 20 * 1024 * 1024

_REQUEST_KEY = "request"


def _req(context: ContextTypes.DEFAULT_TYPE) -> FixRequest:
    req = context.user_data.get(_REQUEST_KEY)
    if req is None:
        req = FixRequest()
        context.user_data[_REQUEST_KEY] = req
    return req


def _reporter(update: Update) -> str:
    user = update.effective_user
    if user is None:
        return ""
    if user.username:
        return f"@{user.username}"
    return user.full_name or str(user.id)


async def start(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    context.user_data[_REQUEST_KEY] = FixRequest(reporter=_reporter(update))
    await update.message.reply_text(
        "🛠️ *HyperHLE app-fix request*\n\n"
        "I'll collect everything needed to get an app fixed:\n"
        "1️⃣ the *IPA link(s)*\n"
        "2️⃣ a *log file*\n"
        "3️⃣ a description of the *bug/crash*\n\n"
        "Your request will be filed against the *latest HyperHLE build from "
        "Actions* and forwarded to a maintainer.\n\n"
        "Send /cancel any time to stop.\n\n"
        "First — what's the *app / game name*?",
        parse_mode=ParseMode.MARKDOWN,
        reply_markup=ReplyKeyboardRemove(),
    )
    return APP_NAME


async def got_app_name(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).app_name = update.message.text.strip()
    await update.message.reply_text(
        "Got it. What's the *app version*? (e.g. `1.0`)\n"
        "Send /skip if you don't know.",
        parse_mode=ParseMode.MARKDOWN,
    )
    return APP_VERSION


async def got_app_version(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).app_version = update.message.text.strip()
    return await _ask_ipa(update)


async def skip_app_version(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _ask_ipa(update)


async def _ask_ipa(update: Update) -> int:
    await update.message.reply_text(
        "🔗 Now send the *IPA link(s)* — a direct download URL to the `.ipa` "
        "(or zipped `.app`). You can paste several, one per line.",
        parse_mode=ParseMode.MARKDOWN,
    )
    return IPA_LINKS


async def got_ipa_links(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    links = req.extract_links(update.message.text)
    if not links:
        await update.message.reply_text(
            "I couldn't find a valid `http(s)://…` link there. Please paste a "
            "direct download URL to the IPA.",
            parse_mode=ParseMode.MARKDOWN,
        )
        return IPA_LINKS
    req.ipa_links = links
    await update.message.reply_text(
        f"✅ Saved {len(links)} link(s).\n\n"
        "🐞 Now *describe the bug or crash*. What happens, and where does it "
        "fail (boot, menu, level, gameplay)?",
        parse_mode=ParseMode.MARKDOWN,
    )
    return BUG


async def got_bug(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).bug_description = update.message.text.strip()
    await update.message.reply_text(
        "📄 Now send the *log file(s)*. Attach the log as a document, or paste "
        "the log text directly. Send /done when you've added at least one.",
        parse_mode=ParseMode.MARKDOWN,
    )
    return LOGS


async def got_log_document(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    doc = update.message.document
    if doc.file_size and doc.file_size > MAX_LOG_BYTES:
        await update.message.reply_text(
            "That file is larger than 20 MB, which is the most I can download. "
            "Please trim the log or paste the relevant part as text."
        )
        return LOGS
    try:
        tg_file = await context.bot.get_file(doc.file_id)
        data = await tg_file.download_as_bytearray()
        text = bytes(data).decode("utf-8", errors="replace")
    except Exception:  # noqa: BLE001 - surface a friendly message, keep collecting
        log.exception("failed to download log document")
        await update.message.reply_text(
            "Sorry, I couldn't download that file. Try again or paste the text."
        )
        return LOGS
    name = doc.file_name or f"log-{len(_req(context).logs) + 1}.txt"
    _req(context).logs.append(LogFile(name=name, content=text))
    await update.message.reply_text(
        f"✅ Added log `{name}`. Send another, or /done to continue.",
        parse_mode=ParseMode.MARKDOWN,
    )
    return LOGS


async def got_log_text(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    name = f"pasted-log-{len(req.logs) + 1}.txt"
    req.logs.append(LogFile(name=name, content=update.message.text))
    await update.message.reply_text(
        "✅ Added pasted log. Send another log, or /done to continue."
    )
    return LOGS


async def logs_done(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    if not req.logs:
        await update.message.reply_text(
            "A log is required so the crash can be investigated. Please attach "
            "or paste at least one log before /done."
        )
        return LOGS
    await update.message.reply_text(
        "🖥️ Optional: what *OS* and *GPU* did you test on? (e.g. "
        "`Windows 11 / NVIDIA GTX 1660`)\nSend /skip to leave it out.",
        parse_mode=ParseMode.MARKDOWN,
    )
    return ENV


async def got_env(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    text = update.message.text.strip()
    if "/" in text:
        os_part, _, gpu_part = text.partition("/")
        req.operating_system = os_part.strip()
        req.gpu = gpu_part.strip()
    else:
        req.operating_system = text
    return await _ask_media(update)


async def skip_env(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _ask_media(update)


async def _ask_media(update: Update) -> int:
    await update.message.reply_text(
        "📷 Optional: send any *screenshots or a short video* that show the "
        "problem. They'll be forwarded to the maintainer.\nSend /done when "
        "you're finished (or to skip).",
        parse_mode=ParseMode.MARKDOWN,
    )
    return MEDIA


async def got_media(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    # Remember the message so it can be forwarded verbatim to the maintainer.
    media_ids: list[int] = context.user_data.setdefault("media_message_ids", [])
    media_ids.append(update.message.message_id)
    count = len(media_ids)
    _req(context).media_note = f"{count} attachment(s) forwarded to the maintainer."
    await update.message.reply_text(
        f"✅ Saved attachment {count}. Send more, or /done to review."
    )
    return MEDIA


async def media_done(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _show_confirmation(update, context)


async def _show_confirmation(
    update: Update, context: ContextTypes.DEFAULT_TYPE
) -> int:
    req = _req(context)
    missing = req.missing()
    if missing:
        await update.message.reply_text(
            "Still missing: " + ", ".join(missing) + ". Use /cancel to start over."
        )
        return MEDIA

    summary = (
        f"*Please review your request:*\n\n"
        f"*App:* {req.app_name}"
        + (f" {req.app_version}" if req.app_version else "")
        + "\n"
        f"*IPA link(s):* {len(req.ipa_links)}\n"
        f"*Logs:* {len(req.logs)}\n"
        f"*Bug:* {req.bug_description[:300]}"
    )
    keyboard = InlineKeyboardMarkup(
        [
            [
                InlineKeyboardButton("✅ Submit", callback_data="submit"),
                InlineKeyboardButton("❌ Cancel", callback_data="abort"),
            ]
        ]
    )
    await update.message.reply_text(
        summary, parse_mode=ParseMode.MARKDOWN, reply_markup=keyboard
    )
    return CONFIRM


async def on_confirm(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    query = update.callback_query
    await query.answer()
    if query.data == "abort":
        context.user_data.clear()
        await query.edit_message_text("Cancelled. Send /fix to start again.")
        return ConversationHandler.END

    req: FixRequest = _req(context)
    cfg: Config = context.bot_data["config"]
    gh: GitHubClient = context.bot_data["github"]

    await query.edit_message_text("⏳ Submitting your request…")

    # Pin the request to the latest build from Actions.
    build = await gh.latest_build()
    if build is not None:
        req.hyperhle_version = build.label
        req.hyperhle_version_url = build.url

    # 1) Open a GitHub issue (or fall back to a prefilled link).
    issue_line = ""
    issue = await gh.create_issue(req.issue_title(), req.issue_body(), cfg.issue_labels)
    if issue is not None:
        issue_line = f"\n📌 GitHub issue: {issue.url}"
    elif not gh.can_open_issues:
        issue_line = (
            "\n📌 No GitHub token configured — prefilled issue link:\n"
            + gh.new_issue_link(req.issue_title(), req.issue_body())
        )
    else:
        issue_line = "\n⚠️ Couldn't open the GitHub issue automatically (it was still forwarded)."

    # 2) Forward to the maintainer.
    forwarded = await _forward_to_maintainer(update, context, req, issue)

    build_line = (
        f"\n🧪 Filed against latest build: {req.hyperhle_version}"
        if req.hyperhle_version
        else ""
    )
    forward_status = (
        f"\n📨 Forwarded to {cfg.forward_username}."
        if forwarded
        else f"\n⚠️ Couldn't forward to {cfg.forward_username} (is FORWARD_CHAT_ID set and has {cfg.forward_username} started the bot?)."
    )
    await context.bot.send_message(
        chat_id=update.effective_chat.id,
        text="✅ *Request submitted!* Thanks for the details."
        + build_line
        + issue_line
        + forward_status,
        parse_mode=ParseMode.MARKDOWN,
        disable_web_page_preview=True,
    )
    context.user_data.clear()
    return ConversationHandler.END


async def _forward_to_maintainer(
    update: Update,
    context: ContextTypes.DEFAULT_TYPE,
    req: FixRequest,
    issue,
) -> bool:
    cfg: Config = context.bot_data["config"]
    if cfg.forward_chat_id is None:
        return False
    try:
        text = req.forward_text()
        if issue is not None:
            text += f"\n\nIssue: {issue.url}"
        await context.bot.send_message(
            chat_id=cfg.forward_chat_id,
            text=text,
            disable_web_page_preview=True,
        )
        # Forward each saved screenshot/video verbatim.
        for msg_id in context.user_data.get("media_message_ids", []):
            try:
                await context.bot.forward_message(
                    chat_id=cfg.forward_chat_id,
                    from_chat_id=update.effective_chat.id,
                    message_id=msg_id,
                )
            except Exception:  # noqa: BLE001 - best effort per attachment
                log.warning("could not forward media message %s", msg_id)
        return True
    except Exception:  # noqa: BLE001
        log.exception("failed to forward to maintainer")
        return False


async def cancel(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    context.user_data.clear()
    await update.message.reply_text(
        "Cancelled. Send /fix whenever you're ready.",
        reply_markup=ReplyKeyboardRemove(),
    )
    return ConversationHandler.END


async def help_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    cfg: Config = context.bot_data["config"]
    await update.message.reply_text(
        "I file *app fix requests* for HyperHLE.\n\n"
        "/fix — start a new request (IPA link, log file, bug description)\n"
        "/cancel — abort the current request\n"
        "/help — this message\n\n"
        f"Requests are pinned to the latest build from {cfg.actions_url} and "
        f"forwarded to {cfg.forward_username}.",
        parse_mode=ParseMode.MARKDOWN,
        disable_web_page_preview=True,
    )


def build_conversation() -> ConversationHandler:
    return ConversationHandler(
        entry_points=[CommandHandler("fix", start)],
        states={
            APP_NAME: [MessageHandler(filters.TEXT & ~filters.COMMAND, got_app_name)],
            APP_VERSION: [
                CommandHandler("skip", skip_app_version),
                MessageHandler(filters.TEXT & ~filters.COMMAND, got_app_version),
            ],
            IPA_LINKS: [MessageHandler(filters.TEXT & ~filters.COMMAND, got_ipa_links)],
            BUG: [MessageHandler(filters.TEXT & ~filters.COMMAND, got_bug)],
            LOGS: [
                CommandHandler("done", logs_done),
                MessageHandler(filters.Document.ALL, got_log_document),
                MessageHandler(filters.TEXT & ~filters.COMMAND, got_log_text),
            ],
            ENV: [
                CommandHandler("skip", skip_env),
                MessageHandler(filters.TEXT & ~filters.COMMAND, got_env),
            ],
            MEDIA: [
                CommandHandler("done", media_done),
                MessageHandler(
                    (filters.PHOTO | filters.VIDEO | filters.Document.ALL), got_media
                ),
            ],
            CONFIRM: [CallbackQueryHandler(on_confirm)],
        },
        fallbacks=[CommandHandler("cancel", cancel)],
        allow_reentry=True,
    )


def register_handlers(application: Application) -> None:
    application.add_handler(build_conversation())
    application.add_handler(CommandHandler(["start", "help"], help_command))
