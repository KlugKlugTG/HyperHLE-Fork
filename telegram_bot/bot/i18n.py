"""User-facing strings in English, Russian and Arabic.

The GitHub issue body and the maintainer forward stay in English (they are
read by maintainers); only the Telegram conversation is localized.
"""
from __future__ import annotations

from telegram.ext import ContextTypes

DEFAULT_LANG = "en"

STRINGS: dict[str, dict[str, str]] = {
    "en": {
        "choose_language": "🌐 Choose your language / Выберите язык / اختر لغتك:",
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
            "`.ipa` (or zipped `.app`) — or *attach the IPA file directly*. "
            "You can paste several links (one per line) or send several "
            "files; send /done when you've attached your file(s)."
        ),
        "no_link": (
            "I couldn't find a valid `http(s)://…` link there. Please paste a "
            "direct download URL to the IPA, or attach the `.ipa` file itself."
        ),
        "ipa_not_ipa": (
            "That file doesn't look like an IPA — please send a `.ipa` (or a "
            "zipped `.app`), or paste a direct download link."
        ),
        "ipa_file_saved": (
            "✅ Saved IPA file `{name}`. Send another file or a link, or "
            "/done to continue."
        ),
        "ipa_required": (
            "I still need at least one IPA link or IPA file before continuing."
        ),
        "ask_bug": (
            "🐞 Now *describe the bug or crash*. What happens, and where does "
            "it fail (boot, menu, level, gameplay)?"
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
            "/language — change the language (English / Русский / العربية)\n"
            "/cancel — abort the current request\n"
            "/help — this message\n\n"
            "Requests are pinned to the latest build from {actions} and "
            "forwarded to {user}."
        ),
    },
    "ru": {
        "choose_language": "🌐 Choose your language / Выберите язык / اختر لغتك:",
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
            "скачивания `.ipa` (или `.app` в zip-архиве) — или *прикрепите "
            "сам файл IPA*. Можно несколько ссылок (по одной на строку) или "
            "несколько файлов; после файлов отправьте /done."
        ),
        "no_link": (
            "Я не нашёл корректной ссылки `http(s)://…`. Пришлите прямую "
            "ссылку на скачивание IPA или прикрепите сам файл `.ipa`."
        ),
        "ipa_not_ipa": (
            "Этот файл не похож на IPA — пришлите `.ipa` (или `.app` в "
            "zip-архиве) либо вставьте прямую ссылку на скачивание."
        ),
        "ipa_file_saved": (
            "✅ Файл IPA `{name}` сохранён. Пришлите ещё файл или ссылку, "
            "либо /done, чтобы продолжить."
        ),
        "ipa_required": (
            "Мне всё ещё нужна хотя бы одна ссылка на IPA или файл IPA."
        ),
        "ask_bug": (
            "🐞 Теперь *опишите баг или вылет*. Что происходит и на каком "
            "этапе (запуск, меню, уровень, геймплей)?"
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
            "/language — сменить язык (English / Русский / العربية)\n"
            "/cancel — прервать текущий запрос\n"
            "/help — это сообщение\n\n"
            "Запросы привязываются к последней сборке из {actions} и "
            "пересылаются {user}."
        ),
    },
    "ar": {
        "choose_language": "🌐 Choose your language / Выберите язык / اختر لغتك:",
        "intro": (
            "🛠️ *طلب إصلاح تطبيق في HyperHLE*\n\n"
            "سأجمع كل ما يلزم لإصلاح التطبيق:\n"
            "1️⃣ *رابط (روابط) ملف IPA*\n"
            "2️⃣ *ملف السجل*\n"
            "3️⃣ وصف *الخطأ/الانهيار*\n\n"
            "سيُربط طلبك بأحدث نسخة من HyperHLE من Actions ويُحوَّل إلى "
            "المشرف.\n\n"
            "أرسل /cancel في أي وقت للإلغاء.\n\n"
            "أولاً — ما اسم *التطبيق / اللعبة*؟"
        ),
        "ask_version": (
            "حسناً. ما *إصدار التطبيق*؟ (مثل `1.0`)\n"
            "أرسل /skip إذا كنت لا تعرف."
        ),
        "ask_ipa": (
            "🔗 الآن أرسل *رابط (روابط) IPA* — رابط تنزيل مباشر لملف `.ipa` "
            "(أو `.app` مضغوط) — أو *أرفق ملف IPA مباشرة*. يمكنك لصق عدة "
            "روابط (كل رابط في سطر) أو إرسال عدة ملفات؛ أرسل /done بعد "
            "إرفاق ملفاتك."
        ),
        "no_link": (
            "لم أجد رابطاً صالحاً يبدأ بـ `http(s)://…`. الرجاء لصق رابط "
            "تنزيل مباشر لملف IPA، أو إرفاق ملف `.ipa` نفسه."
        ),
        "ipa_not_ipa": (
            "هذا الملف لا يبدو ملف IPA — أرسل ملف `.ipa` (أو `.app` مضغوطاً) "
            "أو الصق رابط تنزيل مباشراً."
        ),
        "ipa_file_saved": (
            "✅ تم حفظ ملف IPA‏ `{name}`. أرسل ملفاً أو رابطاً آخر، أو /done "
            "للمتابعة."
        ),
        "ipa_required": (
            "ما زلت بحاجة إلى رابط IPA واحد أو ملف IPA واحد على الأقل قبل "
            "المتابعة."
        ),
        "ask_bug": (
            "🐞 الآن *صِف الخطأ أو الانهيار*. ماذا يحدث، وأين يفشل التطبيق "
            "(الإقلاع، القائمة، المرحلة، اللعب)؟"
        ),
        "links_saved": (
            "✅ تم حفظ {n} رابط/روابط.\n\n"
            "🐞 الآن *صِف الخطأ أو الانهيار*. ماذا يحدث، وأين يفشل "
            "(الإقلاع، القائمة، المرحلة، اللعب)؟"
        ),
        "ask_logs": (
            "📄 الآن أرسل *ملف(ات) السجل*. أرفق السجل كمستند، أو الصق نص "
            "السجل مباشرة. أرسل /done بعد إضافة ملف واحد على الأقل."
        ),
        "file_too_big": (
            "هذا الملف أكبر من 20 ميغابايت، وهو أقصى ما أستطيع تنزيله. "
            "الرجاء اقتصاص السجل أو لصق الجزء المهم كنص."
        ),
        "download_failed": "عذراً، لم أستطع تنزيل الملف. حاول مرة أخرى أو الصق النص.",
        "log_not_txt": (
            "يجب أن يكون السجل ملفاً نصياً بامتداد `.txt` أو `.log`. أرفق "
            "السجل كملف `.txt` أو الصق نصه كرسالة."
        ),
        "log_empty": "هذا السجل فارغ. الرجاء إرسال سجل غير فارغ.",
        "log_added": "✅ تمت إضافة السجل `{name}`. أرسل سجلاً آخر، أو /done للمتابعة.",
        "pasted_log_added": "✅ تمت إضافة نص السجل. أرسل سجلاً آخر، أو /done للمتابعة.",
        "log_required": (
            "السجل إلزامي حتى يمكن التحقيق في الانهيار. الرجاء إرفاق أو لصق "
            "سجل واحد على الأقل قبل /done."
        ),
        "ask_env": (
            "🖥️ اختياري: ما *نظام التشغيل* و*بطاقة الرسوميات (GPU)* اللذان "
            "جرّبت عليهما؟ (مثل `Windows 11 / NVIDIA GTX 1660`)\n"
            "أرسل /skip للتخطي."
        ),
        "ask_media": (
            "📷 اختياري: أرسل *لقطات شاشة أو فيديو قصيراً* يوضح المشكلة. "
            "سيتم تحويلها إلى المشرف.\nأرسل /done عند الانتهاء (أو للتخطي)."
        ),
        "media_saved": "✅ تم حفظ المرفق {n}. أرسل المزيد، أو /done للمراجعة.",
        "missing": "ما زال ينقص: {items}. استخدم /cancel للبدء من جديد.",
        "missing_app_name": "اسم التطبيق",
        "missing_ipa": "رابط IPA",
        "missing_logs": "ملف السجل",
        "missing_bug": "وصف الخطأ/الانهيار",
        "review": (
            "*راجع طلبك من فضلك:*\n\n"
            "*التطبيق:* {app}\n"
            "*روابط IPA:* {links}\n"
            "*السجلات:* {logs}\n"
            "*الخطأ:* {bug}"
        ),
        "btn_submit": "✅ إرسال",
        "btn_cancel": "❌ إلغاء",
        "cancelled_inline": "تم الإلغاء. أرسل /start للبدء من جديد.",
        "submitting": "⏳ جارٍ إرسال طلبك…",
        "submitted": "✅ *تم إرسال الطلب!* شكراً على التفاصيل.",
        "build_ok": "\n🧪 النسخة في سجلك: {v} — ✅ مطابقة لأحدث كوميت.",
        "build_outdated": (
            "\n⚠️ النسخة {v} في سجلك *قديمة* — أحدث كوميت هو {latest}. "
            "الرجاء تنزيل أحدث نسخة من {actions} وإعادة الاختبار؛ فقد يكون "
            "الخطأ مُصلحاً بالفعل. (تم إرسال الطلب على أي حال.)"
        ),
        "build_unknown": (
            "\n🧪 لم أجد رمز النسخة في سجلك، لذا لم أستطع التحقق من أنها "
            "أحدث نسخة."
        ),
        "issue_line_ok": "\n📌 مشكلة (Issue) على GitHub: {url}",
        "issue_line_no_token": (
            "\n📌 لم يتم إعداد رمز GitHub — رابط مشكلة مُعبأ مسبقاً:\n{url}"
        ),
        "issue_line_failed": (
            "\n⚠️ تعذر فتح المشكلة على GitHub تلقائياً (تم تحويل الطلب رغم ذلك)."
        ),
        "forward_ok": "\n📨 تم التحويل إلى {user}.",
        "forward_failed": (
            "\n⚠️ تعذر التحويل إلى {user} (هل تم ضبط FORWARD_CHAT_ID وهل "
            "راسل {user} البوت من قبل؟)."
        ),
        "cancelled": "تم الإلغاء. أرسل /start عندما تكون جاهزاً.",
        "language_set": "✅ تم تغيير اللغة إلى العربية.",
        "help": (
            "أنا أُنشئ *طلبات إصلاح التطبيقات* لـ HyperHLE.\n\n"
            "/start — طلب جديد (رابط IPA، ملف السجل، وصف الخطأ)\n"
            "/language — تغيير اللغة (English / Русский / العربية)\n"
            "/cancel — إلغاء الطلب الحالي\n"
            "/help — هذه الرسالة\n\n"
            "تُربط الطلبات بأحدث نسخة من {actions} وتُحوَّل إلى {user}."
        ),
    },
}


def get_lang(context: ContextTypes.DEFAULT_TYPE) -> str:
    return context.user_data.get("lang", DEFAULT_LANG)


def t(context: ContextTypes.DEFAULT_TYPE, key: str, **kwargs: object) -> str:
    table = STRINGS.get(get_lang(context), STRINGS[DEFAULT_LANG])
    text = table.get(key) or STRINGS[DEFAULT_LANG][key]
    return text.format(**kwargs) if kwargs else text
