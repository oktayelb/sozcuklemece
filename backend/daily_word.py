import json
import os
import threading
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

GAME_TIMEZONE = ZoneInfo("Europe/Istanbul")
WORD_FILE = Path(__file__).parent.parent / "data" / "current_word.json"
WORD_TTL_SECONDS = 24 * 60 * 60


def current_game_day():
    return datetime.now(GAME_TIMEZONE).date()


def read_saved_word(word_file):
    if not word_file.exists():
        return None
    with open(word_file, encoding="utf-8") as saved_file:
        return json.load(saved_file)


def save_word(word_file, puzzle, expires_at):
    word_file.parent.mkdir(parents=True, exist_ok=True)
    temporary_file = word_file.with_suffix(".tmp")
    with open(temporary_file, "w", encoding="utf-8") as saved_file:
        json.dump({"puzzle": puzzle, "expires_at": expires_at}, saved_file, ensure_ascii=False)
    os.replace(temporary_file, word_file)


class DailyWordStore:
    def __init__(self, judge, word_file):
        self.judge = judge
        self.word_file = word_file
        self.word = None
        self.expires_at = None
        self.lock = threading.Lock()

    def load_saved_word(self, now):
        saved = read_saved_word(self.word_file)
        if saved is None:
            return
        if now >= saved["expires_at"]:
            self.word_file.unlink()
            return
        self.word = self.judge.prepare(saved["puzzle"])
        self.expires_at = saved["expires_at"]

    def publish(self, puzzle, now):
        prepared_word = self.judge.prepare(puzzle)
        expires_at = now + WORD_TTL_SECONDS
        with self.lock:
            save_word(self.word_file, puzzle, expires_at)
            self.expires_at = expires_at
            self.word = prepared_word
            self.judge.forget_guesses()

    def drop_expired_word(self, now):
        with self.lock:
            if self.word is None or now < self.expires_at:
                return
            self.word = None
            self.word_file.unlink(missing_ok=True)
            self.judge.forget_guesses()

    def current_word(self, now):
        self.drop_expired_word(now)
        return self.word
