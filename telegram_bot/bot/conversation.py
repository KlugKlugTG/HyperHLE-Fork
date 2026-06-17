"""The /start conversation: pick a language, collect a request, file it, forward it."""
from __future__ import annotations

import logging

from telegram import (
    InlineKeyboardButton,
    InlineKeyboardMarkup,
    ReplyKeyboardRemove,
    Update,
)
from telegram.constants import ParseMode
from telegram.helpers import escape_markdown
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
from .i18n import t
from .request import FixRequest, LogBuildInfo, LogFile, extract_build_info

log = logging.getLogger(__name__)

# Conversation states.
LANGUAGE, APP_NAME, APP_VERSION, IPA_LINKS, BUG, LOGS, ENV, MEDIA, CONFIRM = range(9)

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


def _language_keyboard(prefix: str = "lang") -> InlineKeyboardMarkup:
    return InlineKeyboardMarkup(
        [
            [
                InlineKeyboardButton("🇺🇸 English", callback_data=f"{prefix}:en"),
                InlineKeyboardButton("🇷🇺 Русский", callback_data=f"{prefix}:ru"),
                InlineKeyboardButton("🇸🇦 العربية", callback_data=f"{prefix}:ar"),
            ]
        ]
    )


async def start(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    # A returning user's language survives the reset; they are only asked to
    # pick once. /language changes it later.
    lang = context.user_data.get("lang")
    context.user_data.clear()
    if lang:
        context.user_data["lang"] = lang
    context.user_data[_REQUEST_KEY] = FixRequest(reporter=_reporter(update))
    if lang:
        await update.message.reply_text(
            t(context, "intro"), parse_mode=ParseMode.MARKDOWN
        )
        return APP_NAME
    await update.message.reply_text(
        t(context, "choose_language"),
        reply_markup=_language_keyboard(),
    )
    return LANGUAGE


async def on_language(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    query = update.callback_query
    await query.answer()
    context.user_data["lang"] = query.data.removeprefix("lang:")
    await query.edit_message_text(
        t(context, "intro"),
        parse_mode=ParseMode.MARKDOWN,
    )
    return APP_NAME


async def language_reprompt(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    await update.message.reply_text(
        t(context, "choose_language"),
        reply_markup=_language_keyboard(),
    )
    return LANGUAGE


async def got_app_name(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).app_name = update.message.text.strip()
    await update.message.reply_text(
        t(context, "ask_version"), parse_mode=ParseMode.MARKDOWN
    )
    return APP_VERSION


async def got_app_version(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).app_version = update.message.text.strip()
    return await _ask_ipa(update, context)


async def skip_app_version(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _ask_ipa(update, context)


async def _ask_ipa(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    await update.message.reply_text(
        t(context, "ask_ipa"), parse_mode=ParseMode.MARKDOWN
    )
    return IPA_LINKS


async def got_ipa_links(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    links = req.extract_links(update.message.text)
    if not links:
        await update.message.reply_text(
            t(context, "no_link"), parse_mode=ParseMode.MARKDOWN
        )
        return IPA_LINKS
    req.ipa_links = links
    await update.message.reply_text(
        t(context, "links_saved", n=len(links)), parse_mode=ParseMode.MARKDOWN
    )
    return BUG


async def got_ipa_file(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    """An IPA sent as a Telegram document instead of a link.

    The file is never downloaded — its message is forwarded verbatim to the
    maintainer at submit time (forwarding has no size limit), and the GitHub
    issue notes that the IPA was attached via Telegram.
    """
    req = _req(context)
    doc = update.message.document
    if doc.file_name and not doc.file_name.lower().endswith((".ipa", ".zip")):
        await update.message.reply_text(
            t(context, "ipa_not_ipa"), parse_mode=ParseMode.MARKDOWN
        )
        return IPA_LINKS
    name = doc.file_name or f"app-{len(req.ipa_files) + 1}.ipa"
    req.ipa_files.append(name)
    context.user_data.setdefault("ipa_message_ids", []).append(
        update.message.message_id
    )
    await update.message.reply_text(
        t(context, "ipa_file_saved", name=name), parse_mode=ParseMode.MARKDOWN
    )
    return IPA_LINKS


async def ipa_done(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    if not (req.ipa_links or req.ipa_files):
        await update.message.reply_text(t(context, "ipa_required"))
        return IPA_LINKS
    await update.message.reply_text(
        t(context, "ask_bug"), parse_mode=ParseMode.MARKDOWN
    )
    return BUG


async def got_bug(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    _req(context).bug_description = update.message.text.strip()
    await update.message.reply_text(
        t(context, "ask_logs"), parse_mode=ParseMode.MARKDOWN
    )
    return LOGS


async def got_log_document(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    doc = update.message.document
    if doc.file_name and not doc.file_name.lower().endswith((".txt", ".log")):
        await update.message.reply_text(
            t(context, "log_not_txt"), parse_mode=ParseMode.MARKDOWN
        )
        return LOGS
    if doc.file_size and doc.file_size > MAX_LOG_BYTES:
        await update.message.reply_text(t(context, "file_too_big"))
        return LOGS
    try:
        tg_file = await context.bot.get_file(doc.file_id)
        data = await tg_file.download_as_bytearray()
        text = bytes(data).decode("utf-8", errors="replace")
    except Exception:  # noqa: BLE001 - surface a friendly message, keep collecting
        log.exception("failed to download log document")
        await update.message.reply_text(t(context, "download_failed"))
        return LOGS
    if not text.strip():
        await update.message.reply_text(t(context, "log_empty"))
        return LOGS
    name = doc.file_name or f"log-{len(_req(context).logs) + 1}.txt"
    _req(context).logs.append(LogFile(name=name, content=text))
    await update.message.reply_text(
        t(context, "log_added", name=name), parse_mode=ParseMode.MARKDOWN
    )
    return LOGS


async def got_log_text(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    if not update.message.text.strip():
        await update.message.reply_text(t(context, "log_empty"))
        return LOGS
    name = f"pasted-log-{len(req.logs) + 1}.txt"
    req.logs.append(LogFile(name=name, content=update.message.text))
    await update.message.reply_text(t(context, "pasted_log_added"))
    return LOGS


async def logs_done(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    req = _req(context)
    if not req.logs:
        await update.message.reply_text(t(context, "log_required"))
        return LOGS
    await update.message.reply_text(
        t(context, "ask_env"), parse_mode=ParseMode.MARKDOWN
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
    return await _ask_media(update, context)


async def skip_env(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _ask_media(update, context)


async def _ask_media(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    await update.message.reply_text(
        t(context, "ask_media"), parse_mode=ParseMode.MARKDOWN
    )
    return MEDIA


async def got_media(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    # Remember the message so it can be forwarded verbatim to the maintainer.
    media_ids: list[int] = context.user_data.setdefault("media_message_ids", [])
    media_ids.append(update.message.message_id)
    count = len(media_ids)
    _req(context).media_note = f"{count} attachment(s) forwarded to the maintainer."
    await update.message.reply_text(t(context, "media_saved", n=count))
    return MEDIA


async def media_done(update: Update, context: ContextTypes.DEFAULT_TYPE) -> int:
    return await _show_confirmation(update, context)


async def _show_confirmation(
    update: Update, context: ContextTypes.DEFAULT_TYPE
) -> int:
    req = _req(context)
    missing = req.missing()
    if missing:
        items = ", ".join(t(context, f"missing_{key}") for key in missing)
        await update.message.reply_text(t(context, "missing", items=items))
        return MEDIA

    app_label = req.app_name + (f" {req.app_version}" if req.app_version else "")
    summary = t(
        context,
        "review",
        app=escape_markdown(app_label, version=1),
        links=len(req.ipa_links) + len(req.ipa_files),
        logs=len(req.logs),
        bug=escape_markdown(req.bug_description[:300], version=1),
    )
    keyboard = InlineKeyboardMarkup(
        [
            [
                InlineKeyboardButton(t(context, "btn_submit"), callback_data="submit"),
                InlineKeyboardButton(t(context, "btn_cancel"), callback_data="abort"),
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
        lang = context.user_data.get("lang")
        cancelled_text = t(context, "cancelled_inline")
        context.user_data.clear()
        if lang:
            context.user_data["lang"] = lang
        await query.edit_message_text(cancelled_text)
        return ConversationHandler.END

    req: FixRequest = _req(context)
    cfg: Config = context.bot_data["config"]
    gh: GitHubClient = context.bot_data["github"]

    await query.edit_message_text(t(context, "submitting"))

    # Read the build identity out of the user's log header and compare it
    # with the latest commit on the branch the log says it was built from.
    info = LogBuildInfo()
    for log_file in req.logs:
        candidate = extract_build_info(log_file.content)
        if candidate.version:
            info = candidate
            break
    req.hyperhle_version = info.commit or info.version
    req.hyperhle_version_url = info.run_url
    latest = await gh.latest_commit(info.branch or "trunk")
    if latest is not None:
        req.latest_commit_sha = latest.sha
        if info.commit:
            req.up_to_date = latest.sha.lower().startswith(info.commit)

    # 1) Open a GitHub issue (or fall back to a prefilled link).
    issue = await gh.create_issue(req.issue_title(), req.issue_body(), cfg.issue_labels)
    if issue is not None:
        issue_line = t(context, "issue_line_ok", url=issue.url)
    elif not gh.can_open_issues:
        issue_line = t(
            context,
            "issue_line_no_token",
            url=gh.new_issue_link(req.issue_title(), req.issue_body()),
        )
    else:
        issue_line = t(context, "issue_line_failed")

    # 2) Forward to the maintainer.
    forwarded = await _forward_to_maintainer(update, context, req, issue)

    if req.up_to_date is True:
        build_line = t(context, "build_ok", v=req.hyperhle_version)
    elif req.up_to_date is False:
        build_line = t(
            context,
            "build_outdated",
            v=req.hyperhle_version,
            latest=req.latest_commit_sha[:7],
            actions=cfg.actions_url,
        )
    else:
        build_line = t(context, "build_unknown")
    forward_status = t(
        context,
        "forward_ok" if forwarded else "forward_failed",
        user=cfg.forward_username,
    )
    await context.bot.send_message(
        chat_id=update.effective_chat.id,
        text=t(context, "submitted") + build_line + issue_line + forward_status,
        parse_mode=ParseMode.MARKDOWN,
        disable_web_page_preview=True,
    )
    lang = context.user_data.get("lang")
    context.user_data.clear()
    if lang:
        context.user_data["lang"] = lang
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
        # Forward attached IPA files and screenshots/videos verbatim.
        attachment_ids = context.user_data.get("ipa_message_ids", []) + context.user_data.get(
            "media_message_ids", []
        )
        for msg_id in attachment_ids:
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
    lang = context.user_data.get("lang")
    cancelled_text = t(context, "cancelled")
    context.user_data.clear()
    if lang:
        context.user_data["lang"] = lang
    await update.message.reply_text(
        cancelled_text, reply_markup=ReplyKeyboardRemove()
    )
    return ConversationHandler.END


async def language_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Standalone /language: re-show the picker, also usable mid-conversation.

    Uses the "setlang:" callback prefix so it never collides with the
    conversation's own LANGUAGE/CONFIRM callback handlers.
    """
    await update.message.reply_text(
        t(context, "choose_language"),
        reply_markup=_language_keyboard(prefix="setlang"),
    )


async def on_setlang(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    query = update.callback_query
    await query.answer()
    context.user_data["lang"] = query.data.removeprefix("setlang:")
    await query.edit_message_text(t(context, "language_set"))


async def help_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    cfg: Config = context.bot_data["config"]
    await update.message.reply_text(
        t(context, "help", actions=cfg.actions_url, user=cfg.forward_username),
        parse_mode=ParseMode.MARKDOWN,
        disable_web_page_preview=True,
    )


def build_conversation() -> ConversationHandler:
    return ConversationHandler(
        entry_points=[CommandHandler("start", start)],
        states={
            LANGUAGE: [
                CallbackQueryHandler(on_language, pattern=r"^lang:(en|ru|ar)$"),
                MessageHandler(filters.TEXT & ~filters.COMMAND, language_reprompt),
            ],
            APP_NAME: [MessageHandler(filters.TEXT & ~filters.COMMAND, got_app_name)],
            APP_VERSION: [
                CommandHandler("skip", skip_app_version),
                MessageHandler(filters.TEXT & ~filters.COMMAND, got_app_version),
            ],
            IPA_LINKS: [
                CommandHandler("done", ipa_done),
                MessageHandler(filters.Document.ALL, got_ipa_file),
                MessageHandler(filters.TEXT & ~filters.COMMAND, got_ipa_links),
            ],
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
            CONFIRM: [CallbackQueryHandler(on_confirm, pattern=r"^(submit|abort)$")],
        },
        fallbacks=[CommandHandler("cancel", cancel)],
        allow_reentry=True,
    )


def register_handlers(application: Application) -> None:
    application.add_handler(build_conversation())
    application.add_handler(CommandHandler("help", help_command))
    application.add_handler(CommandHandler("language", language_command))
    application.add_handler(
        CallbackQueryHandler(on_setlang, pattern=r"^setlang:(en|ru|ar)$")
    )
