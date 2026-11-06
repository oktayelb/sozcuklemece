import pytest

from backend.puzzles import load_puzzles
from backend.similarity import WIN_SIMILARITY, BertEncoder, MeaningJudge


@pytest.fixture(scope="module")
def judge():
    return MeaningJudge(BertEncoder(), load_puzzles())


@pytest.fixture(scope="module")
def puzzles_by_word():
    by_word = {}
    for puzzle in load_puzzles():
        by_word[puzzle["word"]] = puzzle
    return by_word


def test_every_definition_wins_its_own_puzzle(judge):
    for puzzle in load_puzzles():
        assert judge.similarity(puzzle, puzzle["definition"]) >= WIN_SIMILARITY


def test_every_wrong_meaning_loses_its_own_puzzle(judge):
    for puzzle in load_puzzles():
        for wrong_meaning in puzzle["wrong_meanings"]:
            assert judge.similarity(puzzle, wrong_meaning) < WIN_SIMILARITY, wrong_meaning


def test_casing_and_punctuation_do_not_change_the_result(judge, puzzles_by_word):
    puzzle = puzzles_by_word["sıyırgaç"]
    plain = judge.similarity(puzzle, "sosu silen ekmek parçası")
    shouted = judge.similarity(puzzle, "SOSU SİLEN EKMEK PARÇASI!!!")
    assert shouted == pytest.approx(plain)


def test_correct_paraphrase_wins(judge, puzzles_by_word):
    puzzle = puzzles_by_word["önözlem"]
    assert judge.similarity(puzzle, "güzel bir an bitmeden onu özlemek") >= WIN_SIMILARITY


@pytest.mark.parametrize(
    ("word", "opposite_guess"),
    [
        ("önözlem", "eski güzel günlere özlem duymak"),
        ("sorusavar", "her soruya dürüstçe cevap veren siyasetçi"),
    ],
)
def test_opposite_meaning_scores_high_on_definition_but_does_not_win(
    judge, puzzles_by_word, word, opposite_guess
):
    assert judge.similarity(puzzles_by_word[word], opposite_guess) < WIN_SIMILARITY


def test_unrelated_guess_is_far(judge, puzzles_by_word):
    assert judge.similarity(puzzles_by_word["kökağı"], "siyasi parti lideri") < 0.3
