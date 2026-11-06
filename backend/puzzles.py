import json
from datetime import date, datetime
from pathlib import Path
from zoneinfo import ZoneInfo

PUZZLES_FILE = Path(__file__).with_name("puzzles.json")
GAME_TIMEZONE = ZoneInfo("Europe/Istanbul")
FIRST_GAME_DAY = date(2026, 1, 1)
WORDS_PER_DAY = 2


def load_puzzles():
    with open(PUZZLES_FILE, encoding="utf-8") as puzzles_file:
        return json.load(puzzles_file)


def current_game_day():
    return datetime.now(GAME_TIMEZONE).date()


def puzzles_for_day(puzzles, day):
    days_since_start = (day - FIRST_GAME_DAY).days
    first_index = days_since_start * WORDS_PER_DAY
    selected = []
    for offset in range(WORDS_PER_DAY):
        selected.append(puzzles[(first_index + offset) % len(puzzles)])
    return selected
