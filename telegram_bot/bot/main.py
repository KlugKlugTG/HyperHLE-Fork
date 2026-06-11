"""Entry point: build the Telegram application and start polling."""
from __future__ import annotations

import logging

from telegram.ext import Application

from .config import load_config
from .conversation import register_handlers
from .github_client import GitHubClient


def build_application() -> Application:
    cfg = load_config()
    application = Application.builder().token(cfg.telegram_token).build()

    application.bot_data["config"] = cfg
    application.bot_data["github"] = GitHubClient(
        owner=cfg.github_owner,
        repo=cfg.github_repo,
        token=cfg.github_token,
        build_workflow=cfg.build_workflow,
    )

    register_handlers(application)
    return application


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    application = build_application()
    logging.getLogger(__name__).info("HyperHLE fix bot starting (polling)…")
    application.run_polling(allowed_updates=None)


if __name__ == "__main__":
    main()
