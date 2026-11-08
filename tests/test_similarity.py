import pytest

from backend.similarity import WIN_SIMILARITY, BertEncoder, MeaningJudge
from tools.evaluate_guesses import load_sample_puzzles


@pytest.fixture(scope="module")
def judge():
    return MeaningJudge(BertEncoder())


@pytest.fixture(scope="module")
def prepared_words(judge):
    prepared = []
    for puzzle in load_sample_puzzles():
        prepared.append(judge.prepare(puzzle))
    return prepared


@pytest.fixture(scope="module")
def words_by_name(prepared_words):
    by_name = {}
    for word in prepared_words:
        by_name[word.puzzle["word"]] = word
    return by_name


def test_every_definition_wins_its_own_puzzle(judge, prepared_words):
    for word in prepared_words:
        assert judge.similarity(word, word.puzzle["definition"]) >= WIN_SIMILARITY


def test_every_wrong_meaning_loses_its_own_puzzle(judge, prepared_words):
    for word in prepared_words:
        for wrong_meaning in word.puzzle["wrong_meanings"]:
            assert judge.similarity(word, wrong_meaning) < WIN_SIMILARITY, wrong_meaning


def test_casing_and_punctuation_do_not_change_the_result(judge, words_by_name):
    word = words_by_name["sıyırgaç"]
    plain = judge.similarity(word, "sosu silen ekmek parçası")
    shouted = judge.similarity(word, "SOSU SİLEN EKMEK PARÇASI!!!")
    assert shouted == pytest.approx(plain)


def test_correct_paraphrase_wins(judge, words_by_name):
    word = words_by_name["önözlem"]
    assert judge.similarity(word, "güzel bir an bitmeden onu özlemek") >= WIN_SIMILARITY


@pytest.mark.parametrize(
    ("name", "opposite_guess"),
    [
        ("önözlem", "eski güzel günlere özlem duymak"),
        ("sorusavar", "her soruya dürüstçe cevap veren siyasetçi"),
    ],
)
def test_opposite_meaning_scores_high_on_definition_but_does_not_win(judge, words_by_name, name, opposite_guess):
    assert judge.similarity(words_by_name[name], opposite_guess) < WIN_SIMILARITY


def test_unrelated_guess_is_far(judge, words_by_name):
    assert judge.similarity(words_by_name["kökağı"], "siyasi parti lideri") < 0.3
