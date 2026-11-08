mod support;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use sozcuklemece_backend::clock::Clock;
use sozcuklemece_backend::daily_word::WORD_TTL_SECONDS;
use sozcuklemece_backend::game::{GAME_RESULT_GRACE_SECONDS, GAME_SECONDS, GUESS_INTERVAL_SECONDS};
use sozcuklemece_backend::rate_limit::{
    ADMIN_LIMIT, GUESS_LIMIT, GUESS_WINDOW_SECONDS, HINT_LIMIT, HINT_WINDOW_SECONDS, RESULT_LIMIT,
};
use support::{
    ADMIN_TOKEN, FakeTeiMode, FakeTurnstileMode, OTHER_ACTION_TURNSTILE_TOKEN,
    OTHER_SITE_TURNSTILE_TOKEN, PLAYER_IP, TESTING_KEY_TURNSTILE_TOKEN, TURNSTILE_SITE_KEY,
    TestServer, VALID_TURNSTILE_TOKEN, first_puzzle, game_day_noon, new_player_id, second_puzzle,
};

const UNKNOWN_GUESS: &str = "bambaşka bir açıklama";

async fn use_up_guess_limit(server: &TestServer) {
    for number in 0..GUESS_LIMIT {
        let game_id = server.start_game_id().await;
        let text = format!("{UNKNOWN_GUESS} {number}");
        assert_eq!(server.guess(&game_id, &text).await.status, StatusCode::OK);
    }
}

fn puzzle_json_with(changes: Value) -> Value {
    let mut puzzle = serde_json::to_value(second_puzzle()).unwrap();
    for (field, value) in changes.as_object().unwrap() {
        puzzle[field] = value.clone();
    }
    puzzle
}

#[tokio::test]
async fn today_reports_date_and_a_ready_word() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.get("/api/today").await.body,
        json!({
            "date": "2026-11-06",
            "word_available": true,
            "turnstile_site_key": TURNSTILE_SITE_KEY,
            "turnstile_action": "start_game",
        })
    );
}

#[tokio::test]
async fn today_reports_when_no_word_is_ready() {
    let server = TestServer::empty().await;
    assert_eq!(server.get("/api/today").await.body["word_available"], false);
}

#[tokio::test]
async fn start_gives_word_and_hint_names_without_hint_contents() {
    let server = TestServer::with_word().await;
    let body = server.start_game().await;
    assert_eq!(body["word"], first_puzzle().word.as_str());
    let kinds: Vec<&str> = body["hints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hint| hint["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["parts", "sentence", "category"]);
    assert_eq!(body["duration_seconds"], GAME_SECONDS as i64);
    assert_eq!(body["score"], 1000);
    assert_eq!(body["max_time_penalty"], 500);
}

#[tokio::test]
async fn start_without_a_word_is_not_found() {
    let server = TestServer::empty().await;
    assert_eq!(
        server
            .start_game_with_token(VALID_TURNSTILE_TOKEN)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn answer_does_not_leak_before_the_game_ends() {
    let server = TestServer::with_word().await;
    let started = server.start_game().await;
    let game_id = started["game_id"].as_str().unwrap();
    let wrong_guess = server.guess(game_id, UNKNOWN_GUESS).await.body;

    let seen_by_player = json!([started, wrong_guess]).to_string();

    let puzzle = first_puzzle();
    assert!(!seen_by_player.contains(&puzzle.definition));
    assert!(!seen_by_player.contains(&puzzle.sentence));
    assert!(!seen_by_player.contains(&puzzle.parts));
    for accepted in &puzzle.also_accepted {
        assert!(!seen_by_player.contains(accepted));
    }
}

#[tokio::test]
async fn hint_is_delivered_on_request_and_lowers_score() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let body = server
        .post(
            &format!("/api/games/{game_id}/hints"),
            json!({ "kind": "sentence" }),
        )
        .await
        .body;
    assert_eq!(body["text"], first_puzzle().sentence.as_str());
    assert!(body["score"].as_i64().unwrap() <= 850);
}

#[tokio::test]
async fn unknown_hint_kind_is_refused() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let response = server
        .post(
            &format!("/api/games/{game_id}/hints"),
            json!({ "kind": "definition" }),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn correct_guess_wins_and_finish_reveals_result() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let won = server
        .guess(&game_id, &format!("{}!", first_puzzle().definition))
        .await
        .body;
    assert_eq!(won["won"], true);
    assert_eq!(won["finished"], true);

    let result = server.finish(&game_id).await.body;
    assert_eq!(result["won"], true);
    assert_eq!(result["definition"], first_puzzle().definition.as_str());
    assert!(result["score"].as_i64().unwrap() > 900);
    assert_eq!(result["guess_count"], 1);
}

#[tokio::test]
async fn wrong_guess_keeps_game_going() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let body = server.guess(&game_id, UNKNOWN_GUESS).await.body;
    assert_eq!(body["won"], false);
    assert_eq!(body["finished"], false);
    assert_eq!(body["closeness"], 0);
}

#[tokio::test]
async fn matching_a_wrong_meaning_does_not_win() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let body = server
        .guess(&game_id, &first_puzzle().wrong_meanings[0])
        .await
        .body;
    assert_eq!(body["won"], false);
}

#[tokio::test]
async fn finish_after_timeout_gives_zero_and_reveals_definition() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.advance_clock(GAME_SECONDS + 1.0);

    let timed_out = server
        .guess(&game_id, &first_puzzle().definition)
        .await
        .body;
    assert_eq!(timed_out["finished"], true);
    assert_eq!(timed_out["won"], false);

    let result = server.finish(&game_id).await.body;
    assert_eq!(result["won"], false);
    assert_eq!(result["score"], 0);
    assert_eq!(result["definition"], first_puzzle().definition.as_str());
}

#[tokio::test]
async fn running_game_keeps_its_word_when_a_new_one_is_published() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;

    server.publish(&second_puzzle()).await;

    assert_eq!(
        server
            .guess(&game_id, &first_puzzle().definition)
            .await
            .body["won"],
        true
    );
}

#[tokio::test]
async fn play_after_an_upload_gets_the_new_word() {
    let server = TestServer::with_word().await;
    server.publish(&second_puzzle()).await;
    assert_eq!(
        server.start_game().await["word"],
        second_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn word_is_gone_after_a_day() {
    let server = TestServer::with_word().await;
    server.advance_clock(WORD_TTL_SECONDS);
    assert_eq!(server.get("/api/today").await.body["word_available"], false);
    assert_eq!(
        server
            .start_game_with_token(VALID_TURNSTILE_TOKEN)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn running_game_can_finish_after_its_word_expires() {
    let server = TestServer::with_word().await;
    server.advance_clock(WORD_TTL_SECONDS - 1.0);
    let game_id = server.start_game_id().await;

    server.advance_clock(2.0);

    assert_eq!(server.get("/api/today").await.body["word_available"], false);
    assert_eq!(
        server
            .guess(&game_id, &first_puzzle().definition)
            .await
            .body["won"],
        true
    );
}

#[tokio::test]
async fn cleanup_removes_an_expired_word_and_its_file() {
    let server = TestServer::with_word().await;
    server.advance_clock(WORD_TTL_SECONDS);

    server.state.remove_expired_state();

    assert!(
        server
            .state
            .word_store
            .current_word(server.state.clock.unix_seconds())
            .is_none()
    );
    assert!(!server.word_file().exists());
}

#[tokio::test]
async fn cleanup_removes_old_games_but_not_running_ones() {
    let server = TestServer::with_word().await;
    let old_game_id = server.start_game_id().await;
    server.advance_clock(GAME_SECONDS + GAME_RESULT_GRACE_SECONDS - 1.0);
    let running_game_id = server.start_game_id().await;
    server.advance_clock(1.0);

    server.state.remove_expired_state();

    let remaining: Vec<String> = server.state.games.lock().unwrap().keys().cloned().collect();
    assert_eq!(remaining, vec![running_game_id]);
    assert_eq!(
        server.guess(&old_game_id, UNKNOWN_GUESS).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn word_survives_a_restart() {
    let first_run = TestServer::with_word().await;

    let second_run = first_run
        .restarted(Clock::manual(game_day_noon() + 60.0))
        .await;

    let game_id = second_run.start_game_id().await;
    assert_eq!(
        second_run
            .guess(&game_id, &first_puzzle().definition)
            .await
            .body["won"],
        true
    );
}

#[tokio::test]
async fn restart_after_a_day_finds_no_word() {
    let first_run = TestServer::with_word().await;

    let second_run = first_run
        .restarted(Clock::manual(game_day_noon() + WORD_TTL_SECONDS))
        .await;

    assert_eq!(
        second_run.get("/api/today").await.body["word_available"],
        false
    );
    assert!(!second_run.word_file().exists());
}

#[tokio::test]
async fn finish_can_be_called_twice() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    let first = server.finish(&game_id).await.body;
    let second = server.finish(&game_id).await.body;
    assert_eq!(first, second);
}

#[tokio::test]
async fn unknown_game_is_not_found() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.guess("yok", UNKNOWN_GUESS).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn empty_and_oversized_guesses_are_refused() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    assert_eq!(
        server.guess(&game_id, "").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        server.guess(&game_id, &"a".repeat(201)).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn guess_length_is_counted_in_letters_not_bytes() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    assert_eq!(
        server.guess(&game_id, &"ş".repeat(200)).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn busy_model_asks_the_player_to_wait() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.tei.set_mode(FakeTeiMode::Overloaded);

    assert_eq!(
        server.guess(&game_id, UNKNOWN_GUESS).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );

    server.tei.set_mode(FakeTeiMode::Working);
    let result = server.finish(&game_id).await.body;
    assert_eq!(result["guess_count"], 0);
}

#[tokio::test]
async fn broken_model_is_a_server_error() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.tei.set_mode(FakeTeiMode::Broken);
    assert_eq!(
        server.guess(&game_id, UNKNOWN_GUESS).await.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn status_shows_the_live_word_and_when_it_expires() {
    let server = TestServer::with_word().await;
    let body = server
        .call(
            Method::GET,
            "/api/admin/word",
            None,
            &[("x-admin-token", ADMIN_TOKEN)],
        )
        .await
        .body;
    assert_eq!(body["word"], first_puzzle().word.as_str());
    assert!(body["expires_at"].as_str().unwrap().ends_with("+03:00"));
}

#[tokio::test]
async fn status_without_a_word() {
    let server = TestServer::empty().await;
    let body = server
        .call(
            Method::GET,
            "/api/admin/word",
            None,
            &[("x-admin-token", ADMIN_TOKEN)],
        )
        .await
        .body;
    assert_eq!(body, json!({ "word": null, "expires_at": null }));
}

#[tokio::test]
async fn publish_reports_the_new_word() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.publish(&second_puzzle()).await.body["word"],
        second_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn upload_needs_the_right_password() {
    let server = TestServer::with_word().await;
    let puzzle = serde_json::to_value(second_puzzle()).unwrap();
    for token in [None, Some("yanlis"), Some("")] {
        assert_eq!(
            server.publish_with_token(&puzzle, token).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        server.start_game().await["word"],
        first_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn status_needs_the_right_password() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.get("/api/admin/word").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn wrong_password_is_refused_before_the_body_is_judged() {
    let server = TestServer::with_word().await;
    let response = server
        .publish_with_token(&json!({ "word": "" }), Some("yanlis"))
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_stays_closed_when_no_password_is_configured() {
    let server = TestServer::empty_with(|config| {
        config.admin_token = None;
    })
    .await;
    let puzzle = serde_json::to_value(second_puzzle()).unwrap();
    assert_eq!(
        server.publish_with_token(&puzzle, Some("")).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn bad_upload_is_refused_and_keeps_the_live_word() {
    let server = TestServer::with_word().await;
    let bad_fields = [
        json!({ "definition": "" }),
        json!({ "definition": "   " }),
        json!({ "word": "a".repeat(61) }),
        json!({ "also_accepted": ["  "] }),
    ];
    for bad_field in bad_fields {
        let response = server
            .publish_with_token(&puzzle_json_with(bad_field.clone()), Some(ADMIN_TOKEN))
            .await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad_field}"
        );
    }
    assert_eq!(
        server.start_game().await["word"],
        first_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn upload_needs_every_hint() {
    let server = TestServer::with_word().await;
    let mut incomplete = serde_json::to_value(second_puzzle()).unwrap();
    incomplete.as_object_mut().unwrap().remove("parts");
    let response = server
        .publish_with_token(&incomplete, Some(ADMIN_TOKEN))
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn uploaded_text_is_trimmed() {
    let server = TestServer::with_word().await;
    let padded_word = format!("  {}  ", second_puzzle().word);
    server
        .publish_with_token(
            &puzzle_json_with(json!({ "word": padded_word })),
            Some(ADMIN_TOKEN),
        )
        .await;
    assert_eq!(
        server.start_game().await["word"],
        second_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn upload_without_accuracy_lists_can_be_played() {
    let server = TestServer::with_word().await;
    let mut puzzle = serde_json::to_value(second_puzzle()).unwrap();
    puzzle.as_object_mut().unwrap().remove("also_accepted");
    puzzle.as_object_mut().unwrap().remove("wrong_meanings");
    assert_eq!(
        server
            .publish_with_token(&puzzle, Some(ADMIN_TOKEN))
            .await
            .status,
        StatusCode::OK
    );

    let game_id = server.start_game_id().await;

    assert_eq!(
        server
            .guess(&game_id, &second_puzzle().definition)
            .await
            .body["won"],
        true
    );
}

#[tokio::test]
async fn failed_upload_keeps_the_live_word() {
    let server = TestServer::with_word().await;
    server.tei.set_mode(FakeTeiMode::Broken);

    assert_eq!(
        server.publish(&second_puzzle()).await.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );

    server.tei.set_mode(FakeTeiMode::Working);
    assert_eq!(
        server.start_game().await["word"],
        first_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn admin_attempts_are_rate_limited() {
    let server = TestServer::empty().await;
    let puzzle = serde_json::to_value(first_puzzle()).unwrap();
    for _ in 0..ADMIN_LIMIT {
        assert_eq!(
            server
                .publish_with_token(&puzzle, Some("yanlis"))
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        server.publish(&first_puzzle()).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn guess_over_the_limit_is_refused_with_a_wait_time() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    use_up_guess_limit(&server).await;

    let refused = server.guess(&game_id, UNKNOWN_GUESS).await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    let retry_after: f64 = refused.headers["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1.0..=GUESS_WINDOW_SECONDS).contains(&retry_after));
}

#[tokio::test]
async fn refused_guess_does_not_reach_the_game() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    use_up_guess_limit(&server).await;

    server.guess(&game_id, UNKNOWN_GUESS).await;

    assert_eq!(server.finish(&game_id).await.body["guess_count"], 0);
}

#[tokio::test]
async fn guess_is_accepted_again_after_the_window() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    use_up_guess_limit(&server).await;

    server.advance_clock(GUESS_WINDOW_SECONDS);

    assert_eq!(
        server.guess(&game_id, UNKNOWN_GUESS).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn guess_limit_is_counted_per_client() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    use_up_guess_limit(&server).await;

    let other_player = server
        .guess_from(&game_id, UNKNOWN_GUESS, "203.0.113.7")
        .await;

    assert_eq!(other_player.status, StatusCode::OK);
}

#[tokio::test]
async fn hints_and_finish_still_work_after_the_guess_limit() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    use_up_guess_limit(&server).await;

    let hint = server
        .post(
            &format!("/api/games/{game_id}/hints"),
            json!({ "kind": "parts" }),
        )
        .await;
    let finish = server.finish(&game_id).await;

    assert_eq!(hint.status, StatusCode::OK);
    assert_eq!(finish.status, StatusCode::OK);
}

async fn ask_for_hint(server: &TestServer, game_id: &str) -> StatusCode {
    server
        .post(
            &format!("/api/games/{game_id}/hints"),
            json!({ "kind": "parts" }),
        )
        .await
        .status
}

#[tokio::test]
async fn hints_are_rate_limited() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    for _ in 0..HINT_LIMIT {
        assert_eq!(ask_for_hint(&server, &game_id).await, StatusCode::OK);
    }

    assert_eq!(
        ask_for_hint(&server, &game_id).await,
        StatusCode::TOO_MANY_REQUESTS
    );

    server.advance_clock(HINT_WINDOW_SECONDS);
    assert_eq!(ask_for_hint(&server, &game_id).await, StatusCode::OK);
}

#[tokio::test]
async fn finish_is_rate_limited() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    for _ in 0..RESULT_LIMIT {
        assert_eq!(server.finish(&game_id).await.status, StatusCode::OK);
    }

    assert_eq!(
        server.finish(&game_id).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn start_tells_the_player_how_long_to_wait_between_guesses() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.start_game().await["guess_interval_seconds"],
        GUESS_INTERVAL_SECONDS
    );
}

#[tokio::test]
async fn guess_sent_too_soon_is_refused_with_a_wait_time() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.guess(&game_id, UNKNOWN_GUESS).await;

    let refused = server.guess(&game_id, "bir başka tahmin").await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    let retry_after: f64 = refused.headers["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(retry_after, GUESS_INTERVAL_SECONDS.ceil());
    assert_eq!(server.finish(&game_id).await.body["guess_count"], 1);
}

#[tokio::test]
async fn guess_after_the_interval_is_accepted() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.guess(&game_id, UNKNOWN_GUESS).await;

    server.advance_clock(GUESS_INTERVAL_SECONDS);

    assert_eq!(
        server.guess(&game_id, "bir başka tahmin").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn each_game_waits_on_its_own() {
    let server = TestServer::with_word().await;
    let first_game_id = server.start_game_id().await;
    let second_game_id = server.start_game_id().await;
    server.guess(&first_game_id, UNKNOWN_GUESS).await;

    assert_eq!(
        server.guess(&second_game_id, UNKNOWN_GUESS).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn today_gives_the_turnstile_site_key() {
    let server = TestServer::with_word().await;
    assert_eq!(
        server.get("/api/today").await.body["turnstile_site_key"],
        TURNSTILE_SITE_KEY
    );
}

#[tokio::test]
async fn start_needs_a_turnstile_token() {
    let server = TestServer::with_word().await;
    let response = server
        .post("/api/games", json!({ "player_id": new_player_id() }))
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(server.state.games.lock().unwrap().is_empty());
}

#[tokio::test]
async fn start_with_a_failed_human_check_is_forbidden() {
    let server = TestServer::with_word().await;
    for token in ["", "sahte-jeton"] {
        let response = server.start_game_with_token(token).await;
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{token}");
    }
    assert!(server.state.games.lock().unwrap().is_empty());
}

#[tokio::test]
async fn token_from_another_site_is_forbidden() {
    let server = TestServer::with_word().await;
    let response = server
        .start_game_with_token(OTHER_SITE_TURNSTILE_TOKEN)
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert!(server.state.games.lock().unwrap().is_empty());
}

#[tokio::test]
async fn token_for_another_action_is_forbidden() {
    let server = TestServer::with_word().await;
    let response = server
        .start_game_with_token(OTHER_ACTION_TURNSTILE_TOKEN)
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn token_from_cloudflare_test_keys_passes() {
    let server = TestServer::with_word().await;
    let response = server
        .start_game_with_token(TESTING_KEY_TURNSTILE_TOKEN)
        .await;
    assert_eq!(response.status, StatusCode::OK);
}

#[tokio::test]
async fn empty_token_is_refused_without_asking_cloudflare() {
    let server = TestServer::with_word().await;
    server.start_game_with_token("").await;
    assert!(server.turnstile.requests().is_empty());
}

#[tokio::test]
async fn turnstile_is_told_the_player_ip() {
    let server = TestServer::with_word().await;
    server.start_game().await;
    let requests = server.turnstile.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].remoteip.as_deref(), Some(PLAYER_IP));
}

#[tokio::test]
async fn unreachable_turnstile_asks_the_player_to_wait() {
    let server = TestServer::with_word().await;
    server.turnstile.set_mode(FakeTurnstileMode::Down);
    let response = server.start_game_with_token(VALID_TURNSTILE_TOKEN).await;
    assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn turnstile_is_not_asked_when_there_is_no_word() {
    let server = TestServer::empty().await;
    server.start_game_with_token(VALID_TURNSTILE_TOKEN).await;
    assert!(server.turnstile.requests().is_empty());
}

async fn start_as(server: &TestServer, player_id: &str) -> Value {
    let response = server.start_game_as(player_id, VALID_TURNSTILE_TOKEN).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    response.body
}

#[tokio::test]
async fn player_who_finished_cannot_start_again() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    server.finish(&game_id).await;

    let second_try = server
        .start_game_as(&player_id, VALID_TURNSTILE_TOKEN)
        .await;

    assert_eq!(second_try.status, StatusCode::CONFLICT);
    assert_eq!(server.state.games.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn already_played_is_refused_without_asking_cloudflare() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    server.finish(&game_id).await;

    server
        .start_game_as(&player_id, VALID_TURNSTILE_TOKEN)
        .await;

    assert_eq!(server.turnstile.requests().len(), 1);
}

#[tokio::test]
async fn winning_counts_as_played_even_without_finish() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    server.guess(&game_id, &first_puzzle().definition).await;

    let second_try = server
        .start_game_as(&player_id, VALID_TURNSTILE_TOKEN)
        .await;

    assert_eq!(second_try.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn running_out_of_time_counts_as_played() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    start_as(&server, &player_id).await;
    server.advance_clock(GAME_SECONDS);

    let second_try = server
        .start_game_as(&player_id, VALID_TURNSTILE_TOKEN)
        .await;

    assert_eq!(second_try.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn starting_again_mid_game_continues_the_same_game() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let first_start = start_as(&server, &player_id).await;
    let game_id = first_start["game_id"].as_str().unwrap();
    server
        .post(
            &format!("/api/games/{game_id}/hints"),
            json!({ "kind": "sentence" }),
        )
        .await;
    server.guess(game_id, UNKNOWN_GUESS).await;
    server.advance_clock(30.0);

    let second_start = start_as(&server, &player_id).await;

    assert_eq!(second_start["game_id"], game_id);
    assert_eq!(second_start["remaining_seconds"], GAME_SECONDS - 30.0);
    assert_eq!(
        second_start["revealed_hints"],
        json!([{ "kind": "sentence", "text": first_puzzle().sentence }])
    );
    assert_eq!(second_start["guesses"][0]["text"], UNKNOWN_GUESS);
    assert_eq!(second_start["guesses"][0]["closeness"], 0);
    assert_eq!(server.state.games.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn new_game_starts_with_nothing_revealed() {
    let server = TestServer::with_word().await;
    let started = server.start_game().await;
    assert_eq!(started["revealed_hints"], json!([]));
    assert_eq!(started["guesses"], json!([]));
}

#[tokio::test]
async fn other_players_can_still_play() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.finish(&game_id).await;

    assert_eq!(
        server
            .start_game_with_token(VALID_TURNSTILE_TOKEN)
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn player_can_play_again_when_a_new_word_is_published() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    server.finish(&game_id).await;

    server.publish(&second_puzzle()).await;

    assert_eq!(
        start_as(&server, &player_id).await["word"],
        second_puzzle().word.as_str()
    );
}

#[tokio::test]
async fn invalid_player_id_is_refused() {
    let server = TestServer::with_word().await;
    for player_id in ["", "kisa", "bosluk iceren bir kimlik 123", &"a".repeat(65)] {
        let response = server.start_game_as(player_id, VALID_TURNSTILE_TOKEN).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{player_id}"
        );
    }
    assert!(server.turnstile.requests().is_empty());
}

#[tokio::test]
async fn player_status_follows_the_game() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    assert_eq!(server.player_today(&player_id).await.body["state"], "new");

    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    let playing = server.player_today(&player_id).await.body;
    assert_eq!(playing["state"], "playing");
    assert_eq!(playing["result"], Value::Null);

    server.guess(&game_id, &first_puzzle().definition).await;
    let finished = server.player_today(&player_id).await.body;
    assert_eq!(finished["state"], "finished");
    assert_eq!(finished["result"]["won"], true);
    assert_eq!(
        finished["result"]["definition"],
        first_puzzle().definition.as_str()
    );
}

#[tokio::test]
async fn result_is_kept_after_the_game_is_cleaned_up() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    start_as(&server, &player_id).await;
    server.advance_clock(GAME_SECONDS + GAME_RESULT_GRACE_SECONDS);

    server.state.remove_expired_state();

    assert!(server.state.games.lock().unwrap().is_empty());
    let status = server.player_today(&player_id).await.body;
    assert_eq!(status["state"], "finished");
    assert_eq!(status["result"]["won"], false);
    assert_eq!(status["result"]["seconds_used"], GAME_SECONDS as i64);
}

#[tokio::test]
async fn player_status_without_a_word_is_not_found() {
    let server = TestServer::empty().await;
    assert_eq!(
        server.player_today(&new_player_id()).await.status,
        StatusCode::NOT_FOUND
    );
}

async fn play_and_finish(server: &TestServer, guess: &str) -> Value {
    let game_id = server.start_game_id().await;
    server.guess(&game_id, guess).await;
    server.finish(&game_id).await.body
}

#[tokio::test]
async fn result_compares_the_player_with_everyone_today() {
    let server = TestServer::with_word().await;
    play_and_finish(&server, UNKNOWN_GUESS).await;
    play_and_finish(&server, UNKNOWN_GUESS).await;

    let winner = play_and_finish(&server, &first_puzzle().definition).await;

    let stats = &winner["stats"];
    assert_eq!(stats["players"], 3);
    assert_eq!(stats["win_percent"], 33);
    assert_eq!(stats["better_than_percent"], 100);
    assert_eq!(stats["average_winning_seconds"], 0);
    assert_eq!(stats["score_buckets"][0]["players"], 2);
    assert_eq!(winner["won"], true);
}

#[tokio::test]
async fn player_status_shows_stats_that_keep_growing() {
    let server = TestServer::with_word().await;
    let player_id = new_player_id();
    let game_id = start_as(&server, &player_id).await["game_id"]
        .as_str()
        .unwrap()
        .to_string();
    server.finish(&game_id).await;
    play_and_finish(&server, &first_puzzle().definition).await;

    let status = server.player_today(&player_id).await.body;

    assert_eq!(status["result"]["stats"]["players"], 2);
    assert_eq!(status["result"]["stats"]["better_than_percent"], 0);
}

#[tokio::test]
async fn new_word_starts_fresh_stats() {
    let server = TestServer::with_word().await;
    play_and_finish(&server, UNKNOWN_GUESS).await;

    server.publish(&second_puzzle()).await;

    let result = play_and_finish(&server, UNKNOWN_GUESS).await;
    assert_eq!(result["stats"]["players"], 1);
}

#[tokio::test]
async fn game_of_a_replaced_word_has_no_stats() {
    let server = TestServer::with_word().await;
    let game_id = server.start_game_id().await;
    server.publish(&second_puzzle()).await;
    server.start_game().await;

    let result = server.finish(&game_id).await.body;

    assert_eq!(result["stats"], Value::Null);
    assert_eq!(result["word"], first_puzzle().word.as_str());
}
