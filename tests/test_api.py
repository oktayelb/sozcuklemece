import json
from datetime import date

import pytest
from fastapi.testclient import TestClient

from backend import main
from backend.game import GAME_SECONDS
from backend.puzzles import load_puzzles, puzzles_for_day
from tests.fakes import ExactMatchEncoder

GAME_DAY = date(2026, 11, 6)
NEXT_DAY = date(2026, 11, 7)
UNKNOWN_GUESS = "bambaşka bir açıklama"


@pytest.fixture
def client(monkeypatch):
    monkeypatch.setattr(main, "BertEncoder", ExactMatchEncoder)
    monkeypatch.setattr(main, "current_game_day", lambda: GAME_DAY)
    with TestClient(main.app) as test_client:
        yield test_client


def expected_puzzle(slot, day=GAME_DAY):
    return puzzles_for_day(load_puzzles(), day)[slot]


def start_game(client, slot=0):
    response = client.post("/api/games", json={"slot": slot})
    assert response.status_code == 200
    return response.json()


def guess(client, game_id, text):
    return client.post(f"/api/games/{game_id}/guesses", json={"text": text})


def test_today_reports_date_and_word_count(client):
    body = client.get("/api/today").json()
    assert body == {"date": "2026-11-06", "word_count": 2}


def test_frontend_is_served(client):
    response = client.get("/")
    assert response.status_code == 200
    assert "Yeni Sözcükle" in response.text


def test_start_gives_word_and_hint_names_without_hint_contents(client):
    puzzle = expected_puzzle(0)
    body = start_game(client)
    assert body["word"] == puzzle["word"]
    assert [hint["kind"] for hint in body["hints"]] == ["parts", "sentence", "category"]
    assert body["duration_seconds"] == GAME_SECONDS
    assert body["score"] == 1000


def test_answer_does_not_leak_before_the_game_ends(client):
    puzzle = expected_puzzle(0)
    started = start_game(client)
    game_id = started["game_id"]
    wrong_guess = guess(client, game_id, UNKNOWN_GUESS).json()

    seen_by_player = json.dumps([started, wrong_guess], ensure_ascii=False)

    assert puzzle["definition"] not in seen_by_player
    assert puzzle["sentence"] not in seen_by_player
    assert puzzle["parts"] not in seen_by_player
    for accepted in puzzle["also_accepted"]:
        assert accepted not in seen_by_player


def test_second_slot_serves_the_second_word(client):
    assert start_game(client, slot=1)["word"] == expected_puzzle(1)["word"]


def test_slot_outside_today_is_not_found(client):
    assert client.post("/api/games", json={"slot": 5}).status_code == 404


def test_hint_is_delivered_on_request_and_lowers_score(client):
    game_id = start_game(client)["game_id"]
    body = client.post(f"/api/games/{game_id}/hints", json={"kind": "sentence"}).json()
    assert body["text"] == expected_puzzle(0)["sentence"]
    assert body["score"] <= 850


def test_unknown_hint_kind_is_refused(client):
    game_id = start_game(client)["game_id"]
    response = client.post(f"/api/games/{game_id}/hints", json={"kind": "definition"})
    assert response.status_code == 422


def test_correct_guess_wins_and_finish_reveals_result(client):
    puzzle = expected_puzzle(0)
    game_id = start_game(client)["game_id"]
    won = guess(client, game_id, puzzle["definition"] + "!").json()
    assert won["won"] is True
    assert won["finished"] is True

    result = client.post(f"/api/games/{game_id}/finish").json()
    assert result["won"] is True
    assert result["definition"] == puzzle["definition"]
    assert result["score"] > 900
    assert result["guess_count"] == 1


def test_wrong_guess_keeps_game_going(client):
    game_id = start_game(client)["game_id"]
    body = guess(client, game_id, UNKNOWN_GUESS).json()
    assert body["won"] is False
    assert body["finished"] is False
    assert body["closeness"] == 0


def test_matching_a_wrong_meaning_does_not_win(client):
    puzzle = expected_puzzle(0)
    game_id = start_game(client)["game_id"]
    body = guess(client, game_id, puzzle["wrong_meanings"][0]).json()
    assert body["won"] is False


def test_finish_after_timeout_gives_zero_and_reveals_definition(client, monkeypatch):
    game_id = start_game(client)["game_id"]
    real_monotonic = main.time.monotonic
    monkeypatch.setattr(main.time, "monotonic", lambda: real_monotonic() + GAME_SECONDS + 1)

    timed_out = guess(client, game_id, expected_puzzle(0)["definition"]).json()
    assert timed_out["finished"] is True
    assert timed_out["won"] is False

    result = client.post(f"/api/games/{game_id}/finish").json()
    assert result["won"] is False
    assert result["score"] == 0
    assert result["definition"] == expected_puzzle(0)["definition"]


def test_game_started_before_midnight_keeps_its_word(client, monkeypatch):
    puzzle_before_midnight = expected_puzzle(0)
    game_id = start_game(client)["game_id"]

    monkeypatch.setattr(main, "current_game_day", lambda: NEXT_DAY)

    won = guess(client, game_id, puzzle_before_midnight["definition"]).json()
    assert won["won"] is True


def test_play_pressed_after_midnight_gets_the_new_word(client, monkeypatch):
    monkeypatch.setattr(main, "current_game_day", lambda: NEXT_DAY)
    assert start_game(client)["word"] == expected_puzzle(0, NEXT_DAY)["word"]


def test_finish_can_be_called_twice(client):
    game_id = start_game(client)["game_id"]
    first = client.post(f"/api/games/{game_id}/finish").json()
    second = client.post(f"/api/games/{game_id}/finish").json()
    assert first == second


def test_unknown_game_is_not_found(client):
    assert guess(client, "yok", UNKNOWN_GUESS).status_code == 404


def test_empty_and_oversized_guesses_are_refused(client):
    game_id = start_game(client)["game_id"]
    assert guess(client, game_id, "").status_code == 422
    assert guess(client, game_id, "a" * 201).status_code == 422
