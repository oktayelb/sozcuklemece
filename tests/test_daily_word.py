import gc
import json
import weakref
from datetime import date, datetime, timedelta, timezone

import pytest

from backend import daily_word
from backend.daily_word import WORD_TTL_SECONDS, DailyWordStore, current_game_day
from backend.game import GAME_RESULT_GRACE_SECONDS, GAME_SECONDS, Game, remove_stale_games
from backend.schemas import WordUpload
from backend.similarity import MeaningJudge, normalize_text
from tests.fakes import BrokenEncoder, ExactMatchEncoder, RecordingEncoder
from tools.evaluate_guesses import load_sample_puzzles

START = 1000.0
SAMPLE_PUZZLES = load_sample_puzzles()
FIRST_PUZZLE = SAMPLE_PUZZLES[0]
SECOND_PUZZLE = SAMPLE_PUZZLES[1]


@pytest.fixture
def word_file(tmp_path):
    return tmp_path / "data" / "current_word.json"


@pytest.fixture
def store(word_file):
    return DailyWordStore(MeaningJudge(ExactMatchEncoder()), word_file)


def saved_words(word_file):
    with open(word_file, encoding="utf-8") as saved_file:
        return json.load(saved_file)


def test_there_is_no_word_before_the_first_upload(store):
    assert store.current_word(START) is None


def test_published_word_is_served(store):
    store.publish(FIRST_PUZZLE, START)
    assert store.current_word(START).puzzle == FIRST_PUZZLE


def test_new_word_replaces_the_old_one_in_memory_and_on_disk(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    store.publish(SECOND_PUZZLE, START + 10)
    assert store.current_word(START + 10).puzzle == SECOND_PUZZLE
    assert saved_words(word_file)["puzzle"] == SECOND_PUZZLE


def test_publishing_forgets_cached_guesses(store):
    store.publish(FIRST_PUZZLE, START)
    store.judge.similarity(store.current_word(START), "eski sözcük için tahmin")
    assert store.judge.encode_guess.cache_info().currsize == 1

    store.publish(SECOND_PUZZLE, START)

    assert store.judge.encode_guess.cache_info().currsize == 0


def test_definition_is_processed_once_when_published(word_file):
    encoder = RecordingEncoder()
    store = DailyWordStore(MeaningJudge(encoder), word_file)
    store.publish(FIRST_PUZZLE, START)
    word = store.current_word(START)

    for number in range(5):
        store.judge.similarity(word, f"tahmin {number}")

    assert encoder.encoded_texts.count(normalize_text(FIRST_PUZZLE["definition"])) == 1


def test_guess_processes_only_the_guess(word_file):
    encoder = RecordingEncoder()
    store = DailyWordStore(MeaningJudge(encoder), word_file)
    store.publish(FIRST_PUZZLE, START)
    word = store.current_word(START)
    texts_after_publishing = len(encoder.encoded_texts)

    store.judge.similarity(word, "yalnızca bu tahmin")

    assert encoder.encoded_texts[texts_after_publishing:] == ["yalnızca bu tahmin"]


def test_word_lasts_exactly_one_day(store):
    store.publish(FIRST_PUZZLE, START)
    assert store.current_word(START + WORD_TTL_SECONDS - 1) is not None
    assert store.current_word(START + WORD_TTL_SECONDS) is None


def test_expiry_removes_every_remnant_of_the_word(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    store.judge.similarity(store.current_word(START), "bir tahmin")

    store.drop_expired_word(START + WORD_TTL_SECONDS)

    assert store.word is None
    assert not word_file.exists()
    assert store.judge.encode_guess.cache_info().currsize == 0


def test_publishing_again_restarts_the_day(store):
    store.publish(FIRST_PUZZLE, START)
    store.publish(SECOND_PUZZLE, START + WORD_TTL_SECONDS - 1)
    assert store.current_word(START + WORD_TTL_SECONDS + 100).puzzle == SECOND_PUZZLE


def test_saved_word_is_processed_again_after_a_restart(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    encoder = RecordingEncoder()
    restarted = DailyWordStore(MeaningJudge(encoder), word_file)

    restarted.load_saved_word(START + 100)

    assert restarted.current_word(START + 100).puzzle == FIRST_PUZZLE
    assert normalize_text(FIRST_PUZZLE["definition"]) in encoder.encoded_texts


def test_restart_does_not_extend_the_day(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    restarted = DailyWordStore(MeaningJudge(ExactMatchEncoder()), word_file)
    restarted.load_saved_word(START + 100)
    assert restarted.current_word(START + WORD_TTL_SECONDS) is None


def test_word_that_expired_while_stopped_is_deleted_without_being_processed(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    encoder = RecordingEncoder()
    restarted = DailyWordStore(MeaningJudge(encoder), word_file)

    restarted.load_saved_word(START + WORD_TTL_SECONDS)

    assert encoder.encoded_texts == []
    assert restarted.word is None
    assert not word_file.exists()


def test_start_without_a_saved_word_leaves_the_store_empty(store):
    store.load_saved_word(START)
    assert store.current_word(START) is None


def test_failed_upload_keeps_the_live_word(store, word_file):
    store.publish(FIRST_PUZZLE, START)
    store.judge.encoder = BrokenEncoder()

    with pytest.raises(RuntimeError):
        store.publish(SECOND_PUZZLE, START + 10)

    assert store.current_word(START + 10).puzzle == FIRST_PUZZLE
    assert saved_words(word_file)["puzzle"] == FIRST_PUZZLE


def test_word_without_wrong_meanings_can_be_judged(store):
    puzzle = dict(FIRST_PUZZLE, also_accepted=[], wrong_meanings=[])
    store.publish(puzzle, START)
    similarity = store.judge.similarity(store.current_word(START), puzzle["definition"])
    assert similarity == pytest.approx(1.0)


def test_old_word_stays_for_a_running_game_then_leaves_memory(store):
    store.publish(FIRST_PUZZLE, START)
    game = Game(store.current_word(START), started_at=START)
    games = {game.id: game}
    old_word = weakref.ref(store.current_word(START))
    del game

    store.publish(SECOND_PUZZLE, START + 10)
    gc.collect()
    assert old_word() is not None

    remove_stale_games(games, START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS)
    gc.collect()
    assert old_word() is None


@pytest.mark.parametrize("puzzle", SAMPLE_PUZZLES, ids=lambda puzzle: puzzle["word"])
def test_sample_puzzles_fit_the_upload_form(puzzle):
    assert WordUpload(**puzzle).word == puzzle["word"]


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

    monkeypatch.setattr(daily_word, "datetime", FixedDatetime)
    assert current_game_day() == expected_day


def test_turkey_offset_used_in_midnight_test_is_three_hours():
    moment = datetime(2026, 11, 6, 21, 0, tzinfo=timezone.utc)
    assert moment.astimezone(daily_word.GAME_TIMEZONE).utcoffset() == timedelta(hours=3)
