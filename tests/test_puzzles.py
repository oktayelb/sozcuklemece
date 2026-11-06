from datetime import date, datetime, timedelta, timezone

import pytest

from backend import puzzles as puzzles_module
from backend.puzzles import WORDS_PER_DAY, current_game_day, load_puzzles, puzzles_for_day

REQUIRED_FIELDS = (
    "word",
    "definition",
    "parts",
    "sentence",
    "category",
    "field",
    "also_accepted",
    "wrong_meanings",
)


def test_every_puzzle_is_complete():
    for puzzle in load_puzzles():
        for field in REQUIRED_FIELDS:
            assert puzzle[field], f"{puzzle['word']} sözcüğünde {field} boş"


def test_words_are_unique():
    words = [puzzle["word"] for puzzle in load_puzzles()]
    assert len(words) == len(set(words))


def test_each_day_gets_the_configured_word_count():
    selected = puzzles_for_day(load_puzzles(), date(2026, 11, 6))
    assert len(selected) == WORDS_PER_DAY


def test_consecutive_days_get_different_words():
    puzzles = load_puzzles()
    today = {puzzle["word"] for puzzle in puzzles_for_day(puzzles, date(2026, 11, 6))}
    tomorrow = {puzzle["word"] for puzzle in puzzles_for_day(puzzles, date(2026, 11, 7))}
    assert today.isdisjoint(tomorrow)


def test_selection_is_stable_within_a_day():
    puzzles = load_puzzles()
    assert puzzles_for_day(puzzles, date(2026, 11, 6)) == puzzles_for_day(puzzles, date(2026, 11, 6))


def test_rotation_works_far_in_the_future():
    selected = puzzles_for_day(load_puzzles(), date(2040, 2, 29))
    assert len(selected) == WORDS_PER_DAY


@pytest.mark.parametrize(
    ("utc_moment", "expected_day"),
    [
        (datetime(2026, 11, 6, 20, 59, tzinfo=timezone.utc), date(2026, 11, 6)),
        (datetime(2026, 11, 6, 21, 1, tzinfo=timezone.utc), date(2026, 11, 7)),
    ],
)
def test_new_day_starts_at_midnight_in_turkey(monkeypatch, utc_moment, expected_day):
    class FixedDatetime(datetime):
        @classmethod
        def now(cls, tz=None):
            return utc_moment.astimezone(tz)

    monkeypatch.setattr(puzzles_module, "datetime", FixedDatetime)
    assert current_game_day() == expected_day


def test_turkey_offset_used_in_midnight_test_is_three_hours():
    moment = datetime(2026, 11, 6, 21, 0, tzinfo=timezone.utc)
    assert moment.astimezone(puzzles_module.GAME_TIMEZONE).utcoffset() == timedelta(hours=3)
