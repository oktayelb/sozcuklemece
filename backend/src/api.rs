use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::clock::Clock;
use crate::config::Config;
use crate::daily_word::{DailyWordStore, StoreError, game_day, moment_in_game_timezone};
use crate::encoder::{EncoderError, TeiEncoder};
use crate::game::{
    FinishResponse, GAME_SECONDS, GUESS_INTERVAL_SECONDS, Game, GuessCheck, GuessResponse,
    HINT_KINDS, HINT_PENALTY, HintKind, HintResponse, JudgedGuess, MAX_GUESS_LENGTH, RevealedHint,
    game_result, remove_stale_games,
};
use crate::players::{PlayerDay, TodaysPlayers, is_valid_player_id};
use crate::puzzle::Puzzle;
use crate::rate_limit::{
    ADMIN_LIMIT, ADMIN_WINDOW_SECONDS, GAME_START_LIMIT, GAME_START_WINDOW_SECONDS, GUESS_LIMIT,
    GUESS_WINDOW_SECONDS, HINT_LIMIT, HINT_WINDOW_SECONDS, RESULT_LIMIT, RESULT_WINDOW_SECONDS,
    RateLimiter, UNKNOWN_CLIENT, rate_limit_key,
};
use crate::similarity::{MeaningJudge, PreparedWord};
use crate::turnstile::{HumanCheck, START_GAME_ACTION, TurnstileVerifier};

pub const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

pub struct AppState {
    pub config: Config,
    pub clock: Clock,
    pub judge: Arc<MeaningJudge>,
    pub word_store: DailyWordStore,
    pub games: Mutex<HashMap<String, Game>>,
    pub players: Mutex<TodaysPlayers>,
    turnstile: TurnstileVerifier,
    game_start_limiter: Mutex<RateLimiter>,
    guess_limiter: Mutex<RateLimiter>,
    hint_limiter: Mutex<RateLimiter>,
    result_limiter: Mutex<RateLimiter>,
    admin_limiter: Mutex<RateLimiter>,
}

impl AppState {
    pub fn new(config: Config, clock: Clock) -> AppState {
        let judge = Arc::new(MeaningJudge::new(TeiEncoder::new(&config.tei_url)));
        let word_store = DailyWordStore::new(judge.clone(), config.word_file.clone());
        let turnstile = TurnstileVerifier::new(
            config.turnstile_secret_key.clone(),
            config.turnstile_verify_url.clone(),
            config.turnstile_hostname.clone(),
        );
        AppState {
            config,
            clock,
            judge,
            word_store,
            games: Mutex::new(HashMap::new()),
            players: Mutex::new(TodaysPlayers::new()),
            turnstile,
            game_start_limiter: Mutex::new(RateLimiter::new(
                GAME_START_LIMIT,
                GAME_START_WINDOW_SECONDS,
            )),
            guess_limiter: Mutex::new(RateLimiter::new(GUESS_LIMIT, GUESS_WINDOW_SECONDS)),
            hint_limiter: Mutex::new(RateLimiter::new(HINT_LIMIT, HINT_WINDOW_SECONDS)),
            result_limiter: Mutex::new(RateLimiter::new(RESULT_LIMIT, RESULT_WINDOW_SECONDS)),
            admin_limiter: Mutex::new(RateLimiter::new(ADMIN_LIMIT, ADMIN_WINDOW_SECONDS)),
        }
    }

    pub fn remove_expired_state(&self) {
        self.word_store.drop_expired_word(self.clock.unix_seconds());
        let mut games = self.games.lock().unwrap();
        let removed_games = remove_stale_games(&mut games, self.clock.monotonic_seconds());
        let mut players = self.players.lock().unwrap();
        for game in &removed_games {
            players.record_finish(game);
        }
    }
}

pub enum ApiError {
    GameNotFound,
    WordNotReady,
    AlreadyPlayed,
    TooManyRequests { seconds_to_wait: f64 },
    AdminDisabled,
    WrongPassword,
    HumanCheckFailed,
    HumanCheckUnavailable,
    InvalidInput(String),
    ModelBusy,
    ServerFailed,
}

impl From<EncoderError> for ApiError {
    fn from(error: EncoderError) -> ApiError {
        match error {
            EncoderError::Overloaded => ApiError::ModelBusy,
            EncoderError::Failed(_) => {
                eprintln!("Tahmin değerlendirilemedi: {error}");
                ApiError::ServerFailed
            }
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> ApiError {
        match error {
            StoreError::Encoder(error) => ApiError::from(error),
            StoreError::File(error) => {
                eprintln!("Sözcük kaydedilemedi: {error}");
                ApiError::ServerFailed
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, detail) = match &self {
            ApiError::GameNotFound => (StatusCode::NOT_FOUND, "Oyun bulunamadı.".to_string()),
            ApiError::WordNotReady => (
                StatusCode::NOT_FOUND,
                "Bugünün sözcüğü henüz hazır değil.".to_string(),
            ),
            ApiError::AlreadyPlayed => (
                StatusCode::CONFLICT,
                "Bugünün sözcüğünü zaten oynadın.".to_string(),
            ),
            ApiError::TooManyRequests { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "Çok fazla istek gönderildi. Biraz bekleyip tekrar dene.".to_string(),
            ),
            ApiError::AdminDisabled => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Sunucuda ADMIN_TOKEN ayarlanmamış.".to_string(),
            ),
            ApiError::WrongPassword => (StatusCode::UNAUTHORIZED, "Parola yanlış.".to_string()),
            ApiError::HumanCheckFailed => (
                StatusCode::FORBIDDEN,
                "İnsan doğrulaması geçilemedi.".to_string(),
            ),
            ApiError::HumanCheckUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "İnsan doğrulaması şu an yapılamıyor. Biraz bekleyip tekrar dene.".to_string(),
            ),
            ApiError::InvalidInput(detail) => (StatusCode::UNPROCESSABLE_ENTITY, detail.clone()),
            ApiError::ModelBusy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Sunucu şu an çok yoğun. Biraz bekleyip tekrar dene.".to_string(),
            ),
            ApiError::ServerFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Sunucuda bir hata oluştu.".to_string(),
            ),
        };

        let mut response = (status, Json(json!({ "detail": detail }))).into_response();
        if let ApiError::TooManyRequests { seconds_to_wait } = self {
            let retry_after = HeaderValue::from(seconds_to_wait.ceil() as u64);
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, retry_after);
        }
        response
    }
}

fn host_from_proxy_header(parts: &Parts, config: &Config) -> Option<String> {
    let header_name = config.client_ip_header.as_ref()?;
    let value = parts.headers.get(header_name)?;
    let text = value.to_str().ok()?;
    Some(text.trim().to_string())
}

fn client_host(parts: &Parts, config: &Config) -> String {
    if let Some(host) = host_from_proxy_header(parts, config) {
        return host;
    }
    if let Some(ConnectInfo(address)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
        return address.ip().to_string();
    }
    UNKNOWN_CLIENT.to_string()
}

fn enforce_rate_limit(
    limiter: &Mutex<RateLimiter>,
    client: &str,
    now: f64,
) -> Result<(), ApiError> {
    let seconds_to_wait = limiter.lock().unwrap().register_request(client, now);
    if seconds_to_wait > 0.0 {
        return Err(ApiError::TooManyRequests { seconds_to_wait });
    }
    Ok(())
}

pub struct Client {
    host: String,
    rate_limit_key: String,
}

impl Client {
    fn known_ip(&self) -> Option<&str> {
        if self.host == UNKNOWN_CLIENT {
            return None;
        }
        Some(&self.host)
    }
}

impl FromRequestParts<Arc<AppState>> for Client {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Client, ApiError> {
        let host = client_host(parts, &state.config);
        let rate_limit_key = rate_limit_key(&host);
        Ok(Client {
            host,
            rate_limit_key,
        })
    }
}

pub struct AdminAccess;

impl FromRequestParts<Arc<AppState>> for AdminAccess {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<AdminAccess, ApiError> {
        let client = rate_limit_key(&client_host(parts, &state.config));
        enforce_rate_limit(
            &state.admin_limiter,
            &client,
            state.clock.monotonic_seconds(),
        )?;

        let Some(expected_token) = &state.config.admin_token else {
            return Err(ApiError::AdminDisabled);
        };
        let given_token = match parts.headers.get(ADMIN_TOKEN_HEADER) {
            Some(value) => value.as_bytes(),
            None => b"",
        };
        if given_token.ct_eq(expected_token.as_bytes()).into() {
            Ok(AdminAccess)
        } else {
            Err(ApiError::WrongPassword)
        }
    }
}

#[derive(Serialize)]
struct TodayResponse {
    date: String,
    word_available: bool,
    turnstile_site_key: String,
    turnstile_action: &'static str,
}

#[derive(Serialize)]
struct WordStatusResponse {
    word: Option<String>,
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct HintOption {
    kind: HintKind,
    label: &'static str,
}

#[derive(Serialize)]
struct StartResponse {
    game_id: String,
    word: String,
    hints: Vec<HintOption>,
    duration_seconds: i64,
    hint_penalty: i64,
    guess_interval_seconds: f64,
    remaining_seconds: f64,
    score: i64,
    revealed_hints: Vec<RevealedHint>,
    guesses: Vec<JudgedGuess>,
}

#[derive(Deserialize)]
struct StartRequest {
    turnstile_token: String,
    player_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum PlayerState {
    New,
    Playing,
    Finished,
}

#[derive(Serialize)]
struct PlayerTodayResponse {
    state: PlayerState,
    result: Option<FinishResponse>,
}

#[derive(Deserialize)]
struct HintRequest {
    kind: HintKind,
}

#[derive(Deserialize)]
struct GuessRequest {
    text: String,
}

fn word_status(state: &AppState) -> WordStatusResponse {
    match state.word_store.current_word(state.clock.unix_seconds()) {
        Some(live) => WordStatusResponse {
            word: Some(live.word.puzzle.word.clone()),
            expires_at: Some(moment_in_game_timezone(live.expires_at)),
        },
        None => WordStatusResponse {
            word: None,
            expires_at: None,
        },
    }
}

async fn get_today(State(state): State<Arc<AppState>>) -> Json<TodayResponse> {
    let now = state.clock.unix_seconds();
    Json(TodayResponse {
        date: game_day(now).to_string(),
        word_available: state.word_store.current_word(now).is_some(),
        turnstile_site_key: state.config.turnstile_site_key.clone(),
        turnstile_action: START_GAME_ACTION,
    })
}

async fn get_word_status(
    _: AdminAccess,
    State(state): State<Arc<AppState>>,
) -> Json<WordStatusResponse> {
    Json(word_status(&state))
}

async fn publish_word(
    _: AdminAccess,
    State(state): State<Arc<AppState>>,
    Json(upload): Json<Puzzle>,
) -> Result<Json<WordStatusResponse>, ApiError> {
    let puzzle = match upload.cleaned() {
        Ok(puzzle) => puzzle,
        Err(invalid) => {
            return Err(ApiError::InvalidInput(format!(
                "'{}' alanı boş ya da çok uzun.",
                invalid.0
            )));
        }
    };
    state
        .word_store
        .publish(puzzle, state.clock.unix_seconds())
        .await?;
    Ok(Json(word_status(&state)))
}

fn start_response(game: &Game, now: f64) -> StartResponse {
    let mut hints = Vec::new();
    for kind in HINT_KINDS {
        hints.push(HintOption {
            kind,
            label: kind.label(),
        });
    }
    StartResponse {
        game_id: game.id.clone(),
        word: game.puzzle().word.clone(),
        hints,
        duration_seconds: GAME_SECONDS as i64,
        hint_penalty: HINT_PENALTY,
        guess_interval_seconds: GUESS_INTERVAL_SECONDS,
        remaining_seconds: game.remaining_seconds(now),
        score: game.current_score(now),
        revealed_hints: game.revealed_hints(),
        guesses: game.judged_guesses(),
    }
}

fn current_day_of_player(
    games: &mut HashMap<String, Game>,
    players: &mut TodaysPlayers,
    word: &Arc<PreparedWord>,
    player_id: &str,
    now: f64,
) -> Option<PlayerDay> {
    players.follow_word(word);
    let day = players.day_of(player_id)?;
    let PlayerDay::Playing { game_id } = &day else {
        return Some(day);
    };
    let game = games.get_mut(game_id)?;
    game.finish_if_time_is_up(now);
    players.record_finish(game);
    players.day_of(player_id)
}

fn game_to_continue(
    games: &mut HashMap<String, Game>,
    players: &mut TodaysPlayers,
    word: &Arc<PreparedWord>,
    player_id: &str,
    now: f64,
) -> Result<Option<StartResponse>, ApiError> {
    match current_day_of_player(games, players, word, player_id, now) {
        None => Ok(None),
        Some(PlayerDay::Finished { .. }) => Err(ApiError::AlreadyPlayed),
        Some(PlayerDay::Playing { game_id }) => Ok(Some(start_response(&games[&game_id], now))),
    }
}

fn check_player_id(player_id: &str) -> Result<(), ApiError> {
    if is_valid_player_id(player_id) {
        return Ok(());
    }
    Err(ApiError::InvalidInput(
        "Oyuncu kimliği geçersiz.".to_string(),
    ))
}

async fn start_game(
    State(state): State<Arc<AppState>>,
    client: Client,
    Json(body): Json<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    check_player_id(&body.player_id)?;
    enforce_rate_limit(
        &state.game_start_limiter,
        &client.rate_limit_key,
        state.clock.monotonic_seconds(),
    )?;
    let Some(live) = state.word_store.current_word(state.clock.unix_seconds()) else {
        return Err(ApiError::WordNotReady);
    };
    {
        let now = state.clock.monotonic_seconds();
        let mut games = state.games.lock().unwrap();
        let mut players = state.players.lock().unwrap();
        let existing_game =
            game_to_continue(&mut games, &mut players, &live.word, &body.player_id, now)?;
        if let Some(response) = existing_game {
            return Ok(Json(response));
        }
    }

    match state
        .turnstile
        .verify(&body.turnstile_token, client.known_ip())
        .await
    {
        HumanCheck::Passed => {}
        HumanCheck::Failed => return Err(ApiError::HumanCheckFailed),
        HumanCheck::Unavailable => return Err(ApiError::HumanCheckUnavailable),
    }

    let now = state.clock.monotonic_seconds();
    let mut games = state.games.lock().unwrap();
    let mut players = state.players.lock().unwrap();
    let existing_game =
        game_to_continue(&mut games, &mut players, &live.word, &body.player_id, now)?;
    if let Some(response) = existing_game {
        return Ok(Json(response));
    }
    let game = Game::new(live.word, body.player_id, now);
    let response = start_response(&game, now);
    players.record_start(&game);
    games.insert(game.id.clone(), game);
    Ok(Json(response))
}

async fn get_player_today(
    State(state): State<Arc<AppState>>,
    client: Client,
    Path(player_id): Path<String>,
) -> Result<Json<PlayerTodayResponse>, ApiError> {
    check_player_id(&player_id)?;
    enforce_rate_limit(
        &state.result_limiter,
        &client.rate_limit_key,
        state.clock.monotonic_seconds(),
    )?;
    let Some(live) = state.word_store.current_word(state.clock.unix_seconds()) else {
        return Err(ApiError::WordNotReady);
    };
    let now = state.clock.monotonic_seconds();
    let mut games = state.games.lock().unwrap();
    let mut players = state.players.lock().unwrap();
    let response =
        match current_day_of_player(&mut games, &mut players, &live.word, &player_id, now) {
            None => PlayerTodayResponse {
                state: PlayerState::New,
                result: None,
            },
            Some(PlayerDay::Playing { .. }) => PlayerTodayResponse {
                state: PlayerState::Playing,
                result: None,
            },
            Some(PlayerDay::Finished { outcome }) => PlayerTodayResponse {
                state: PlayerState::Finished,
                result: Some(game_result(&live.word.puzzle, &outcome)),
            },
        };
    Ok(Json(response))
}

async fn reveal_hint(
    State(state): State<Arc<AppState>>,
    client: Client,
    Path(game_id): Path<String>,
    Json(body): Json<HintRequest>,
) -> Result<Json<HintResponse>, ApiError> {
    enforce_rate_limit(
        &state.hint_limiter,
        &client.rate_limit_key,
        state.clock.monotonic_seconds(),
    )?;
    let now = state.clock.monotonic_seconds();
    let mut games = state.games.lock().unwrap();
    let Some(game) = games.get_mut(&game_id) else {
        return Err(ApiError::GameNotFound);
    };
    let response = game.reveal_hint(body.kind, now);
    state.players.lock().unwrap().record_finish(game);
    Ok(Json(response))
}

async fn submit_guess(
    State(state): State<Arc<AppState>>,
    client: Client,
    Path(game_id): Path<String>,
    Json(body): Json<GuessRequest>,
) -> Result<Json<GuessResponse>, ApiError> {
    let guess_length = body.text.chars().count();
    if guess_length == 0 || guess_length > MAX_GUESS_LENGTH {
        return Err(ApiError::InvalidInput(
            "Tahmin boş ya da çok uzun.".to_string(),
        ));
    }
    enforce_rate_limit(
        &state.guess_limiter,
        &client.rate_limit_key,
        state.clock.monotonic_seconds(),
    )?;

    let now = state.clock.monotonic_seconds();
    let word_to_judge = {
        let mut games = state.games.lock().unwrap();
        let Some(game) = games.get_mut(&game_id) else {
            return Err(ApiError::GameNotFound);
        };
        match game.check_guess(&body.text, now) {
            GuessCheck::TooSoon { seconds_to_wait } => {
                return Err(ApiError::TooManyRequests { seconds_to_wait });
            }
            GuessCheck::Answered(response) => {
                state.players.lock().unwrap().record_finish(game);
                return Ok(Json(response));
            }
            GuessCheck::NeedsJudging(word) => word,
        }
    };

    let similarity = state.judge.similarity(&word_to_judge, &body.text).await?;

    let mut games = state.games.lock().unwrap();
    let Some(game) = games.get_mut(&game_id) else {
        return Err(ApiError::GameNotFound);
    };
    let response = game.record_judged_guess(&body.text, similarity, now);
    state.players.lock().unwrap().record_finish(game);
    Ok(Json(response))
}

async fn finish_game(
    State(state): State<Arc<AppState>>,
    client: Client,
    Path(game_id): Path<String>,
) -> Result<Json<FinishResponse>, ApiError> {
    enforce_rate_limit(
        &state.result_limiter,
        &client.rate_limit_key,
        state.clock.monotonic_seconds(),
    )?;
    let now = state.clock.monotonic_seconds();
    let mut games = state.games.lock().unwrap();
    let Some(game) = games.get_mut(&game_id) else {
        return Err(ApiError::GameNotFound);
    };
    game.finish(now, false);
    state.players.lock().unwrap().record_finish(game);
    Ok(Json(game.result()))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/today", get(get_today))
        .route("/api/admin/word", get(get_word_status).post(publish_word))
        .route("/api/players/{player_id}/today", get(get_player_today))
        .route("/api/games", post(start_game))
        .route("/api/games/{game_id}/hints", post(reveal_hint))
        .route("/api/games/{game_id}/guesses", post(submit_guess))
        .route("/api/games/{game_id}/finish", post(finish_game))
        .with_state(state)
}
