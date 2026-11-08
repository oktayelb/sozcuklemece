import json
from datetime import date

import pytest
from fastapi.testclient import TestClient

from backend import main
from backend.daily_word import WORD_TTL_SECONDS
from backend.game import GAME_RESULT_GRACE_SECONDS, GAME_SECONDS
from backend.rate_limit import (
    ADMIN_LIMIT,
    GAME_START_LIMIT,
    GAME_START_WINDOW_SECONDS,
    GUESS_LIMIT,
    GUESS_WINDOW_SECONDS,
)
from tests.fakes import ExactMatchEncoder
from tools.evaluate_guesses import load_sample_puzzles

GAME_DAY = date(2026, 11, 6)
UNKNOWN_GUESS = "bambaşka bir açıklama"
ADMIN_TOKEN = "deneme-parolasi"
ADMIN_HEADERS = {"X-Admin-Token": ADMIN_TOKEN}
SAMPLE_PUZZLES = load_sample_puzzles()
FIRST_PUZZLE = SAMPLE_PUZZLES[0]
SECOND_PUZZLE = SAMPLE_PUZZLES[1]


@pytest.fixture
def app_settings(monkeypatch, tmp_path):
    monkeypatch.setattr(main, "BertEncoder", ExactMatchEncoder)
    monkeypatch.setattr(main, "current_game_day", lambda: GAME_DAY)
    monkeypatch.setattr(main, "WORD_FILE", tmp_path / "current_word.json")
    monkeypatch.setenv("ADMIN_TOKEN", ADMIN_TOKEN)


@pytest.fixture
def empty_client(app_settings):
    with TestClient(main.app) as test_client:
        yield test_client


@pytest.fixture
def client(empty_client):
    assert publish(empty_client, FIRST_PUZZLE).status_code == 200
    return empty_client


def publish(client, puzzle, headers=ADMIN_HEADERS):
    return client.post("/api/admin/word", json=puzzle, headers=headers)


def start_game(client):
    response = client.post("/api/games")
    assert response.status_code == 200
    return response.json()


def guess(client, game_id, text):
    return client.post(f"/api/games/{game_id}/guesses", json={"text": text})


def use_up_guess_limit(client, game_id):
    for _ in range(GUESS_LIMIT):
        assert guess(client, game_id, UNKNOWN_GUESS).status_code == 200


def advance_clock(monkeypatch, seconds):
    real_time = main.time.time
    real_monotonic = main.time.monotonic
    monkeypatch.setattr(main.time, "time", lambda: real_time() + seconds)
    monkeypatch.setattr(main.time, "monotonic", lambda: real_monotonic() + seconds)


def test_today_reports_date_and_a_ready_word(client):
    assert client.get("/api/today").json() == {"date": "2026-11-06", "word_available": True}


def test_today_reports_when_no_word_is_ready(empty_client):
    assert empty_client.get("/api/today").json()["word_available"] is False


def test_frontend_is_served_with_one_play_button(client):
    response = client.get("/")
    assert response.status_code == 200
    assert "Sözcüklemece" in response.text
    assert response.text.count('id="play-button"') == 1


def test_admin_page_is_served(client):
    response = client.get("/admin/")
    assert response.status_code == 200
    assert "Günün sözcüğü" in response.text


@pytest.mark.parametrize("number", [1, 2, 3])
def test_pencil_sounds_are_served(client, number):
    response = client.get(f"/sounds/pencil-{number}.mp3")
    assert response.status_code == 200
    assert response.headers["content-type"] == "audio/mpeg"


def test_start_gives_word_and_hint_names_without_hint_contents(client):
    body = start_game(client)
    assert body["word"] == FIRST_PUZZLE["word"]
    assert [hint["kind"] for hint in body["hints"]] == ["parts", "sentence", "category"]
    assert body["duration_seconds"] == GAME_SECONDS
    assert body["score"] == 1000


def test_start_without_a_word_is_not_found(empty_client):
    assert empty_client.post("/api/games").status_code == 404


def test_answer_does_not_leak_before_the_game_ends(client):
    started = start_game(client)
    game_id = started["game_id"]
    wrong_guess = guess(client, game_id, UNKNOWN_GUESS).json()

    seen_by_player = json.dumps([started, wrong_guess], ensure_ascii=False)

    assert FIRST_PUZZLE["definition"] not in seen_by_player
    assert FIRST_PUZZLE["sentence"] not in seen_by_player
    assert FIRST_PUZZLE["parts"] not in seen_by_player
    for accepted in FIRST_PUZZLE["also_accepted"]:
        assert accepted not in seen_by_player


def test_hint_is_delivered_on_request_and_lowers_score(client):
    game_id = start_game(client)["game_id"]
    body = client.post(f"/api/games/{game_id}/hints", json={"kind": "sentence"}).json()
    assert body["text"] == FIRST_PUZZLE["sentence"]
    assert body["score"] <= 850


def test_unknown_hint_kind_is_refused(client):
    game_id = start_game(client)["game_id"]
    response = client.post(f"/api/games/{game_id}/hints", json={"kind": "definition"})
    assert response.status_code == 422


def test_correct_guess_wins_and_finish_reveals_result(client):
    game_id = start_game(client)["game_id"]
    won = guess(client, game_id, FIRST_PUZZLE["definition"] + "!").json()
    assert won["won"] is True
    assert won["finished"] is True

    result = client.post(f"/api/games/{game_id}/finish").json()
    assert result["won"] is True
    assert result["definition"] == FIRST_PUZZLE["definition"]
    assert result["score"] > 900
    assert result["guess_count"] == 1


def test_wrong_guess_keeps_game_going(client):
    game_id = start_game(client)["game_id"]
    body = guess(client, game_id, UNKNOWN_GUESS).json()
    assert body["won"] is False
    assert body["finished"] is False
    assert body["closeness"] == 0


def test_matching_a_wrong_meaning_does_not_win(client):
    game_id = start_game(client)["game_id"]
    body = guess(client, game_id, FIRST_PUZZLE["wrong_meanings"][0]).json()
    assert body["won"] is False


def test_finish_after_timeout_gives_zero_and_reveals_definition(client, monkeypatch):
    game_id = start_game(client)["game_id"]
    advance_clock(monkeypatch, GAME_SECONDS + 1)

    timed_out = guess(client, game_id, FIRST_PUZZLE["definition"]).json()
    assert timed_out["finished"] is True
    assert timed_out["won"] is False

    result = client.post(f"/api/games/{game_id}/finish").json()
    assert result["won"] is False
    assert result["score"] == 0
    assert result["definition"] == FIRST_PUZZLE["definition"]


def test_running_game_keeps_its_word_when_a_new_one_is_published(client):
    game_id = start_game(client)["game_id"]

    publish(client, SECOND_PUZZLE)

    assert guess(client, game_id, FIRST_PUZZLE["definition"]).json()["won"] is True


def test_play_after_an_upload_gets_the_new_word(client):
    publish(client, SECOND_PUZZLE)
    assert start_game(client)["word"] == SECOND_PUZZLE["word"]


def test_word_is_gone_after_a_day(client, monkeypatch):
    advance_clock(monkeypatch, WORD_TTL_SECONDS)
    assert client.get("/api/today").json()["word_available"] is False
    assert client.post("/api/games").status_code == 404


def test_running_game_can_finish_after_its_word_expires(client, monkeypatch):
    advance_clock(monkeypatch, WORD_TTL_SECONDS - 1)
    game_id = start_game(client)["game_id"]

    advance_clock(monkeypatch, 2)

    assert client.get("/api/today").json()["word_available"] is False
    assert guess(client, game_id, FIRST_PUZZLE["definition"]).json()["won"] is True


def test_cleanup_removes_an_expired_word_and_its_file(client, monkeypatch):
    advance_clock(monkeypatch, WORD_TTL_SECONDS)

    main.remove_expired_state(client.app)

    assert client.app.state.word_store.word is None
    assert not main.WORD_FILE.exists()


def test_cleanup_removes_old_games_but_not_running_ones(client, monkeypatch):
    old_game_id = start_game(client)["game_id"]
    advance_clock(monkeypatch, GAME_SECONDS + GAME_RESULT_GRACE_SECONDS - 1)
    running_game_id = start_game(client)["game_id"]
    advance_clock(monkeypatch, 1)

    main.remove_expired_state(client.app)

    assert list(client.app.state.games) == [running_game_id]
    assert guess(client, old_game_id, UNKNOWN_GUESS).status_code == 404


def test_word_survives_a_restart(app_settings):
    with TestClient(main.app) as first_run:
        publish(first_run, FIRST_PUZZLE)

    with TestClient(main.app) as second_run:
        game_id = start_game(second_run)["game_id"]
        assert guess(second_run, game_id, FIRST_PUZZLE["definition"]).json()["won"] is True


def test_restart_after_a_day_finds_no_word(app_settings, monkeypatch):
    with TestClient(main.app) as first_run:
        publish(first_run, FIRST_PUZZLE)

    advance_clock(monkeypatch, WORD_TTL_SECONDS)

    with TestClient(main.app) as second_run:
        assert second_run.get("/api/today").json()["word_available"] is False
    assert not main.WORD_FILE.exists()


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


def test_status_shows_the_live_word_and_when_it_expires(client):
    body = client.get("/api/admin/word", headers=ADMIN_HEADERS).json()
    assert body["word"] == FIRST_PUZZLE["word"]
    assert body["expires_at"].endswith("+03:00")


def test_status_without_a_word(empty_client):
    body = empty_client.get("/api/admin/word", headers=ADMIN_HEADERS).json()
    assert body == {"word": None, "expires_at": None}


def test_publish_reports_the_new_word(client):
    assert publish(client, SECOND_PUZZLE).json()["word"] == SECOND_PUZZLE["word"]


@pytest.mark.parametrize("headers", [{}, {"X-Admin-Token": "yanlis"}, {"X-Admin-Token": ""}])
def test_upload_needs_the_right_password(client, headers):
    assert publish(client, SECOND_PUZZLE, headers=headers).status_code == 401
    assert start_game(client)["word"] == FIRST_PUZZLE["word"]


def test_status_needs_the_right_password(client):
    assert client.get("/api/admin/word").status_code == 401


def test_wrong_password_is_refused_before_the_body_is_judged(client):
    response = client.post("/api/admin/word", json={"word": ""}, headers={"X-Admin-Token": "yanlis"})
    assert response.status_code == 401


@pytest.mark.parametrize("token", ["", None])
def test_admin_stays_closed_when_no_password_is_configured(client, monkeypatch, token):
    if token is None:
        monkeypatch.delenv("ADMIN_TOKEN")
    else:
        monkeypatch.setenv("ADMIN_TOKEN", token)

    assert publish(client, SECOND_PUZZLE, headers={"X-Admin-Token": ""}).status_code == 503


@pytest.mark.parametrize(
    "bad_field",
    [
        {"definition": ""},
        {"definition": "   "},
        {"word": "a" * 61},
        {"also_accepted": ["  "]},
    ],
)
def test_bad_upload_is_refused_and_keeps_the_live_word(client, bad_field):
    response = publish(client, dict(SECOND_PUZZLE, **bad_field))
    assert response.status_code == 422
    assert start_game(client)["word"] == FIRST_PUZZLE["word"]


def test_upload_needs_every_hint(client):
    incomplete = dict(SECOND_PUZZLE)
    del incomplete["parts"]
    assert publish(client, incomplete).status_code == 422


def test_uploaded_text_is_trimmed(client):
    publish(client, dict(SECOND_PUZZLE, word=f"  {SECOND_PUZZLE['word']}  "))
    assert start_game(client)["word"] == SECOND_PUZZLE["word"]


def test_upload_without_accuracy_lists_can_be_played(client):
    puzzle = dict(SECOND_PUZZLE)
    del puzzle["also_accepted"]
    del puzzle["wrong_meanings"]
    assert publish(client, puzzle).status_code == 200

    game_id = start_game(client)["game_id"]

    assert guess(client, game_id, SECOND_PUZZLE["definition"]).json()["won"] is True


def test_admin_attempts_are_rate_limited(empty_client):
    for _ in range(ADMIN_LIMIT):
        assert publish(empty_client, FIRST_PUZZLE, headers={"X-Admin-Token": "yanlis"}).status_code == 401

    assert publish(empty_client, FIRST_PUZZLE).status_code == 429


def test_guess_over_the_limit_is_refused_with_a_wait_time(client):
    game_id = start_game(client)["game_id"]
    use_up_guess_limit(client, game_id)

    refused = guess(client, game_id, UNKNOWN_GUESS)

    assert refused.status_code == 429
    assert 1 <= int(refused.headers["retry-after"]) <= GUESS_WINDOW_SECONDS


def test_refused_guess_does_not_reach_the_game(client):
    game_id = start_game(client)["game_id"]
    use_up_guess_limit(client, game_id)

    guess(client, game_id, UNKNOWN_GUESS)

    result = client.post(f"/api/games/{game_id}/finish").json()
    assert result["guess_count"] == GUESS_LIMIT


def test_guess_is_accepted_again_after_the_window(client, monkeypatch):
    game_id = start_game(client)["game_id"]
    use_up_guess_limit(client, game_id)

    advance_clock(monkeypatch, GUESS_WINDOW_SECONDS)

    assert guess(client, game_id, UNKNOWN_GUESS).status_code == 200


def test_guess_limit_is_counted_per_client(client):
    game_id = start_game(client)["game_id"]
    use_up_guess_limit(client, game_id)

    other_player = TestClient(main.app, client=("203.0.113.7", 50000))

    assert guess(other_player, game_id, UNKNOWN_GUESS).status_code == 200


def test_hints_and_finish_still_work_after_the_guess_limit(client):
    game_id = start_game(client)["game_id"]
    use_up_guess_limit(client, game_id)

    hint = client.post(f"/api/games/{game_id}/hints", json={"kind": "parts"})
    finish = client.post(f"/api/games/{game_id}/finish")

    assert hint.status_code == 200
    assert finish.status_code == 200


def test_starting_too_many_games_is_refused_with_a_wait_time(client):
    for _ in range(GAME_START_LIMIT):
        start_game(client)

    refused = client.post("/api/games")

    assert refused.status_code == 429
    assert 1 <= int(refused.headers["retry-after"]) <= GAME_START_WINDOW_SECONDS
    assert len(client.app.state.games) == GAME_START_LIMIT


def test_request_without_a_client_address_still_works(client):
    anonymous_player = TestClient(main.app, client=None)
    assert anonymous_player.post("/api/games").status_code == 200
