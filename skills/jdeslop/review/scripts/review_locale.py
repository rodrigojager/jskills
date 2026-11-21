"""Localization for the textual output of this skill."""

import json
import os
from pathlib import Path


MESSAGES = json.loads(Path(__file__).with_suffix(".json").read_text(encoding="utf-8"))


def preference(key, choices, default):
    value = os.environ.get("JDESLOP_" + key, os.environ.get("ANTI_SLOP_" + key))
    return value if value in choices else default


def tr(key, *values):
    language = preference("LANG", MESSAGES, "en")
    message = MESSAGES[language].get(key, key)
    return message.format(*values) if values else message
