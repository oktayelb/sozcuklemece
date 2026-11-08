import pytest

from backend.game import (
    GAME_RESULT_GRACE_SECONDS,
    GAME_SECONDS,
    HINT_PENALTY,
    Game,
    closeness_label,
    remove_stale_games,
    repeats_the_word,
)
from backend.similarity import WIN_SIMILARITY, PreparedWord, closeness_percent, normalize_text

PUZZLE = {
    "word": "sıyırgaç",
    "definition": "Tabakta kalan sosu sıyırmak için kullanılan ekmek parçası",
    "parts": "sıyır- + -gaç",
    "sentence": "Son lokmayı sıyırgaçla topladı.",
    "category": "nesne",
    "field": "yemek",
}
START = 1000.0


class FixedJudge:
    def __init__(self, similarity):
        self.fixed_similarity = similarity

    def similarity(self, word, guess):
        return self.fixed_similarity


def new_game():
    return Game(PreparedWord(PUZZLE, None, None), started_at=START)


def test_full_score_at_first_moment():
    assert new_game().current_score(START) == 1000


def test_score_drops_with_time():
    game = new_game()
    assert game.current_score(START + 90) == 750
    assert game.current_score(START + GAME_SECONDS) == 500


def test_each_hint_costs_points_once():
    game = new_game()
    game.reveal_hint("parts", START)
    game.reveal_hint("parts", START)
    assert game.current_score(START) == 1000 - HINT_PENALTY
    game.reveal_hint("sentence", START)
    assert game.current_score(START) == 1000 - 2 * HINT_PENALTY


def test_worst_winning_score_stays_positive():
    game = new_game()
    for kind in ("parts", "sentence", "category"):
        game.reveal_hint(kind, START)
    assert game.current_score(START + GAME_SECONDS) == 50


def test_category_hint_contains_category_and_field():
    response = new_game().reveal_hint("category", START)
    assert "nesne" in response["text"]
    assert "yemek" in response["text"]


def test_winning_guess_finishes_game_with_score():
    game = new_game()
    response = game.submit_guess("sosu silen ekmek", FixedJudge(WIN_SIMILARITY), START + 36)
    assert response["won"] is True
    assert response["finished"] is True
    assert response["closeness"] == 100
    assert response["score"] == 900


def test_score_is_frozen_after_win():
    game = new_game()
    game.submit_guess("sosu silen ekmek", FixedJudge(WIN_SIMILARITY), START + 36)
    assert game.current_score(START + 170) == 900


def test_guess_below_threshold_keeps_game_running():
    game = new_game()
    response = game.submit_guess("sosu silen kaşık", FixedJudge(WIN_SIMILARITY - 0.01), START + 5)
    assert response["won"] is False
    assert response["finished"] is False
    assert response["closeness"] < 100


def test_guess_after_deadline_cannot_win():
    game = new_game()
    response = game.submit_guess("sosu silen ekmek", FixedJudge(0.99), START + GAME_SECONDS + 1)
    assert response["won"] is False
    assert response["finished"] is True
    assert response["closeness"] is None
    assert game.final_score == 0


def test_hint_after_deadline_reveals_nothing():
    game = new_game()
    response = game.reveal_hint("sentence", START + GAME_SECONDS + 1)
    assert response["text"] is None
    assert response["finished"] is True


def test_rejected_guess_is_not_counted():
    game = new_game()
    response = game.submit_guess("SIYIRGAÇ", FixedJudge(0.99), START)
    assert response["rejected"] is True
    assert response["finished"] is False
    assert game.guess_count == 0


@pytest.mark.parametrize("guess", ["sıyırgaç", "SIYIRGAÇ", "sıyır gaç", "sıyır-gaç", "bu bir sıyırgaçtır"])
def test_repeating_the_word_is_detected(guess):
    assert repeats_the_word(guess, PUZZLE)


def test_explaining_with_the_root_is_allowed():
    assert not repeats_the_word("tabağı sıyırmak", PUZZLE)


@pytest.mark.parametrize("guess", ["evimsemek", "evimsedim", "evimsemeye başladım", "EVİMSEDİM"])
def test_inflected_forms_of_the_word_are_detected(guess):
    verb_puzzle = {"word": "evimsemek", "inflected_stems": ["evimse"]}
    assert repeats_the_word(guess, verb_puzzle)


@pytest.mark.parametrize("guess", ["göz kulak", "gözkulak", "göz-kulak", "GÖZ KULAK olmak"])
def test_word_written_with_a_space_or_hyphen_is_detected(guess):
    assert repeats_the_word(guess, {"word": "göz-kulak"})


def test_stem_is_matched_whatever_its_case():
    assert repeats_the_word("evimsedim", {"word": "evimsemek", "inflected_stems": ["EVİMSE"]})


def test_stem_with_no_letters_does_not_reject_every_guess():
    assert not repeats_the_word("bambaşka bir tahmin", {"word": "evimsemek", "inflected_stems": ["-"]})


def test_game_stays_until_its_result_can_no_longer_be_requested():
    game = new_game()
    assert not game.is_stale(START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS - 1)
    assert game.is_stale(START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS)


def test_only_stale_games_are_removed():
    old_game = new_game()
    young_game = Game(PreparedWord(PUZZLE, None, None), started_at=START + 100)
    games = {old_game.id: old_game, young_game.id: young_game}

    remove_stale_games(games, START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS)

    assert list(games) == [young_game.id]


def test_too_short_guess_is_rejected():
    response = new_game().submit_guess("a!", FixedJudge(0.99), START)
    assert response["rejected"] is True


@pytest.mark.parametrize(
    ("typed", "expected"),
    [
        ("IŞIK", "ışık"),
        ("İÇİN", "için"),
        ("  Sosu   silen,   ekmek!  ", "sosu silen ekmek"),
    ],
)
def test_normalize_text_follows_turkish_rules(typed, expected):
    assert normalize_text(typed) == expected


def test_closeness_never_shows_a_win_below_threshold():
    assert closeness_percent(WIN_SIMILARITY - 0.0001) == 99


def test_closeness_never_goes_below_zero():
    assert closeness_percent(-0.5) == 0


def test_closeness_labels_cover_the_range():
    assert closeness_label(100) == "Buldun!"
    assert closeness_label(90) == "Çok yakın"
    assert closeness_label(70) == "Yakın"
    assert closeness_label(40) == "Isınıyor"
    assert closeness_label(5) == "Uzak"
