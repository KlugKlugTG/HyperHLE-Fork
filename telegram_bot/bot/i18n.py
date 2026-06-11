"""User-facing strings in English and Russian.

The GitHub issue body and the maintainer forward stay in English (they are
read by maintainers); only the Telegram conversation is localized.
"""
from __future__ import annotations

from telegram.ext import ContextTypes

DEFAULT_LANG = "en"

STRINGS: dict[str, dict[str, str]] = {
    "en": {
        "choose_language": "🌐 Choose your language / Выберите язык:",
        "intro": (
            "🛠️ *HyperHLE app-fix request*\n\n"
            "I'll collect everything needed to get an app fixed:\n"
            "1️⃣ the *IPA link(s)*\n"
            "2️⃣ a *log file*\n"
            "3️⃣ a description of the *bug/crash*\n\n"
            "Your request will be filed against the *latest HyperHLE build "
            "from Actions* and forwarded to a maintainer.\n\n"
            "Send /cancel any time to stop.\n\n"
            "First — what's the *app / game name*?"
        ),
        "ask_version": (
            "Got it. What's the *app version*? (e.g. `1.0`)\n"
            "Send /skip if you don't know."
        ),
        "ask_ipa": (
            "🔗 Now send the *IPA link(s)* — a direct download URL to the "
            "`.ipa` (or zipped `.app`). You can paste several, one per line."
        ),
        "no_link": (
            "I couldn't find a valid `http(s)://…` link there. Please paste a "
            "direct download URL to the IPA."
        ),
        "links_saved": (
            "✅ Saved {n} link(s).\n\n"
            "🐞 Now *describe the bug or crash*. What happens, and where does "
            "it fail (boot, menu, level, gameplay)?"
        ),
        "ask_logs": (
            "📄 Now send the *log file(s)*. Attach the log as a document, or "
            "paste the log text directly. Send /done when you've added at "
            "least one."
        ),
        "file_too_big": (
            "That file is larger than 20 MB, which is the most I can "
            "download. Please trim the log or paste the relevant part as text."
        ),
        "download_failed": "Sorry, I couldn't download that file. Try again or paste the text.",
        "log_not_txt": (
            "Logs must be plain-text `.txt` or `.log` files. Please attach "
            "the log as a `.txt` file, or paste its text as a message."
        ),
        "log_empty": "That log is empty. Please send a non-empty log.",
        "log_added": "✅ Added log `{name}`. Send another, or /done to continue.",
        "pasted_log_added": "✅ Added pasted log. Send another log, or /done to continue.",
        "log_required": (
            "A log is required so the crash can be investigated. Please "
            "attach or paste at least one log before /done."
        ),
        "ask_env": (
            "🖥️ Optional: what *OS* and *GPU* did you test on? (e.g. "
            "`Windows 11 / NVIDIA GTX 1660`)\nSend /skip to leave it out."
        ),
        "ask_media": (
            "📷 Optional: send any *screenshots or a short video* that show "
            "the problem. They'll be forwarded to the maintainer.\nSend /done "
            "when you're finished (or to skip)."
        ),
        "media_saved": "✅ Saved attachment {n}. Send more, or /done to review.",
        "missing": "Still missing: {items}. Use /cancel to start over.",
        "missing_app_name": "app name",
        "missing_ipa": "IPA link(s)",
        "missing_logs": "log file(s)",
        "missing_bug": "bug/crash description",
        "review": (
            "*Please review your request:*\n\n"
            "*App:* {app}\n"
            "*IPA link(s):* {links}\n"
            "*Logs:* {logs}\n"
            "*Bug:* {bug}"
        ),
        "btn_submit": "✅ Submit",
        "btn_cancel": "❌ Cancel",
        "cancelled_inline": "Cancelled. Send /start to begin again.",
        "submitting": "⏳ Submitting your request…",
        "submitted": "✅ *Request submitted!* Thanks for the details.",
        "build_ok": "\n🧪 Build from your log: {v} — ✅ up to date with the latest commit.",
        "build_outdated": (
            "\n⚠️ Build {v} from your log is *OUTDATED* — the latest commit is "
            "{latest}. Please download the newest build from {actions} and "
            "re-test; the bug may already be fixed. (The request was still "
            "submitted.)"
        ),
        "build_unknown": (
            "\n🧪 I couldn't find a HyperHLE build hash in your log, so I "
            "couldn't verify it's the latest build."
        ),
        "issue_line_ok": "\n📌 GitHub issue: {url}",
        "issue_line_no_token": (
            "\n📌 No GitHub token configured — prefilled issue link:\n{url}"
        ),
        "issue_line_failed": (
            "\n⚠️ Couldn't open the GitHub issue automatically (it was still forwarded)."
        ),
        "forward_ok": "\n📨 Forwarded to {user}.",
        "forward_failed": (
            "\n⚠️ Couldn't forward to {user} (is FORWARD_CHAT_ID set and has "
            "{user} started the bot?)."
        ),
        "cancelled": "Cancelled. Send /start whenever you're ready.",
        "language_set": "✅ Language set to English.",
        "help": (
            "I file *app fix requests* for HyperHLE.\n\n"
            "/start — start a new request (IPA link, log file, bug description)\n"
            "/language — change the language (English / Русский)\n"
            "/cancel — abort the current request\n"
            "/help — this message\n\n"
            "Requests are pinned to the latest build from {actions} and "
            "forwarded to {user}."
        ),
    },
    "ru": {
        "choose_language": "🌐 Choose your language / Выберите язык:",
        "intro": (
            "🛠️ *Запрос на исправление приложения в HyperHLE*\n\n"
            "Я соберу всё, что нужно, чтобы приложение исправили:\n"
            "1️⃣ *ссылку (ссылки) на IPA*\n"
            "2️⃣ *файл лога*\n"
            "3️⃣ описание *бага/вылета*\n\n"
            "Запрос будет привязан к *последней сборке HyperHLE из Actions* "
            "и переслан мейнтейнеру.\n\n"
            "Отправьте /cancel в любой момент, чтобы прервать.\n\n"
            "Для начала — как называется *приложение / игра*?"
        ),
        "ask_version": (
            "Принято. Какая *версия приложения*? (например, `1.0`)\n"
            "Отправьте /skip, если не знаете."
        ),
        "ask_ipa": (
            "🔗 Теперь отправьте *ссылку (ссылки) на IPA* — прямой URL для "
            "скачивания `.ipa` (или `.app` в zip-архиве). Можно несколько, "
            "по одной на строку."
        ),
        "no_link": (
            "Я не нашёл корректной ссылки `http(s)://…`. Пожалуйста, "
            "пришлите прямую ссылку на скачивание IPA."
        ),
        "links_saved": (
            "✅ Сохранено ссылок: {n}.\n\n"
            "🐞 Теперь *опишите баг или вылет*. Что происходит и на каком "
            "этапе (запуск, меню, уровень, геймплей)?"
        ),
        "ask_logs": (
            "📄 Теперь отправьте *файл(ы) лога*. Прикрепите лог как документ "
            "или вставьте текст лога сообщением. Отправьте /done, когда "
            "добавите хотя бы один."
        ),
        "file_too_big": (
            "Файл больше 20 МБ — это максимум, который я могу скачать. "
            "Обрежьте лог или вставьте нужную часть текстом."
        ),
        "download_failed": "Не удалось скачать файл. Попробуйте ещё раз или вставьте текст.",
        "log_not_txt": (
            "Лог должен быть текстовым файлом `.txt` или `.log`. Прикрепите "
            "лог как `.txt` или вставьте его текст сообщением."
        ),
        "log_empty": "Этот лог пустой. Пришлите непустой лог.",
        "log_added": "✅ Лог `{name}` добавлен. Пришлите ещё один или /done, чтобы продолжить.",
        "pasted_log_added": (
            "✅ Текст лога добавлен. Пришлите ещё один лог или /done, чтобы продолжить."
        ),
        "log_required": (
            "Лог обязателен — без него вылет не получится исследовать. "
            "Прикрепите или вставьте хотя бы один лог перед /done."
        ),
        "ask_env": (
            "🖥️ Необязательно: на какой *ОС* и каком *GPU* вы тестировали? "
            "(например, `Windows 11 / NVIDIA GTX 1660`)\n"
            "Отправьте /skip, чтобы пропустить."
        ),
        "ask_media": (
            "📷 Необязательно: пришлите *скриншоты или короткое видео* с "
            "проблемой. Они будут пересланы мейнтейнеру.\nОтправьте /done, "
            "когда закончите (или чтобы пропустить)."
        ),
        "media_saved": "✅ Вложение {n} сохранено. Пришлите ещё или /done для проверки.",
        "missing": "Ещё не хватает: {items}. Используйте /cancel, чтобы начать заново.",
        "missing_app_name": "название приложения",
        "missing_ipa": "ссылка на IPA",
        "missing_logs": "файл лога",
        "missing_bug": "описание бага/вылета",
        "review": (
            "*Проверьте ваш запрос:*\n\n"
            "*Приложение:* {app}\n"
            "*Ссылок на IPA:* {links}\n"
            "*Логов:* {logs}\n"
            "*Баг:* {bug}"
        ),
        "btn_submit": "✅ Отправить",
        "btn_cancel": "❌ Отмена",
        "cancelled_inline": "Отменено. Отправьте /start, чтобы начать заново.",
        "submitting": "⏳ Отправляю ваш запрос…",
        "submitted": "✅ *Запрос отправлен!* Спасибо за подробности.",
        "build_ok": "\n🧪 Сборка из вашего лога: {v} — ✅ совпадает с последним коммитом.",
        "build_outdated": (
            "\n⚠️ Сборка {v} из вашего лога *УСТАРЕЛА* — последний коммит: "
            "{latest}. Скачайте свежую сборку из {actions} и проверьте ещё "
            "раз; баг, возможно, уже исправлен. (Запрос всё равно отправлен.)"
        ),
        "build_unknown": (
            "\n🧪 Я не нашёл хеш сборки HyperHLE в вашем логе, поэтому не "
            "смог проверить, последняя ли это версия."
        ),
        "issue_line_ok": "\n📌 Issue на GitHub: {url}",
        "issue_line_no_token": (
            "\n📌 GitHub-токен не настроен — ссылка с предзаполненным issue:\n{url}"
        ),
        "issue_line_failed": (
            "\n⚠️ Не удалось автоматически создать issue на GitHub "
            "(запрос всё равно переслан)."
        ),
        "forward_ok": "\n📨 Переслано {user}.",
        "forward_failed": (
            "\n⚠️ Не удалось переслать {user} (задан ли FORWARD_CHAT_ID и "
            "писал(а) ли {user} боту хоть раз?)."
        ),
        "cancelled": "Отменено. Отправьте /start, когда будете готовы.",
        "language_set": "✅ Язык переключён на русский.",
        "help": (
            "Я создаю *запросы на исправление приложений* для HyperHLE.\n\n"
            "/start — новый запрос (ссылка на IPA, файл лога, описание бага)\n"
            "/language — сменить язык (English / Русский)\n"
            "/cancel — прервать текущий запрос\n"
            "/help — это сообщение\n\n"
            "Запросы привязываются к последней сборке из {actions} и "
            "пересылаются {user}."
        ),
    },
}


def get_lang(context: ContextTypes.DEFAULT_TYPE) -> str:
    return context.user_data.get("lang", DEFAULT_LANG)


def t(context: ContextTypes.DEFAULT_TYPE, key: str, **kwargs: object) -> str:
    table = STRINGS.get(get_lang(context), STRINGS[DEFAULT_LANG])
    text = table.get(key) or STRINGS[DEFAULT_LANG][key]
    return text.format(**kwargs) if kwargs else text
