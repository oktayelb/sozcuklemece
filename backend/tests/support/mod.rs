#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Form;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{TimeZone, Utc};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

use sozcuklemece_backend::api::{AppState, router};
use sozcuklemece_backend::clock::Clock;
use sozcuklemece_backend::config::Config;
use sozcuklemece_backend::puzzle::Puzzle;
use sozcuklemece_backend::text::normalize_text;

pub const ADMIN_TOKEN: &str = "deneme-parolasi";
pub const CLIENT_IP_HEADER: &str = "x-real-ip";
pub const PLAYER_IP: &str = "203.0.113.5";
pub const VALID_TURNSTILE_TOKEN: &str = "gecerli-jeton";
pub const OTHER_SITE_TURNSTILE_TOKEN: &str = "baska-sitenin-jetonu";
pub const OTHER_ACTION_TURNSTILE_TOKEN: &str = "baska-eylemin-jetonu";
pub const TESTING_KEY_TURNSTILE_TOKEN: &str = "XXXX.DUMMY.TOKEN.XXXX";
pub const TURNSTILE_SITE_KEY: &str = "deneme-site-anahtari";
pub const TURNSTILE_HOSTNAME: &str = "sozcuklemece.test";
const TURNSTILE_SECRET_KEY: &str = "deneme-gizli-anahtar";
const VECTOR_SIZE: usize = 1024;

pub fn new_player_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn load_sample_puzzles() -> Vec<Puzzle> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/sample_puzzles.json");
    let contents = fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

pub fn first_puzzle() -> Puzzle {
    load_sample_puzzles().remove(0)
}

pub fn second_puzzle() -> Puzzle {
    load_sample_puzzles().remove(1)
}

pub fn game_day_noon() -> f64 {
    Utc.with_ymd_and_hms(2026, 11, 6, 9, 0, 0)
        .unwrap()
        .timestamp() as f64
}

#[derive(Clone, Copy, PartialEq)]
pub enum FakeTeiMode {
    Working,
    Broken,
    Overloaded,
}

#[derive(Default)]
struct FakeTeiMemory {
    position_of_text: HashMap<String, usize>,
    encoded_texts: Vec<String>,
    predicted_pairs: Vec<(String, String)>,
}

struct FakeTeiState {
    memory: Mutex<FakeTeiMemory>,
    mode: Mutex<FakeTeiMode>,
}

pub const FAKE_TEI_MAX_WORDS: usize = 75;
pub const NEAR_GUESS_PREFIX: &str = "neredeyse ";
pub const ENTAILING_GUESS_PREFIX: &str = "yani ";
const FAKE_ENTAILMENT: f64 = 0.95;
const FAKE_NO_ENTAILMENT: f64 = 0.05;
const NEAR_GUESS_SIMILARITY: f32 = 0.55;

#[derive(Deserialize)]
struct EmbedRequest {
    inputs: Vec<String>,
    truncate: Option<bool>,
}

fn too_long_answer(request: &EmbedRequest) -> Option<Response> {
    if request.truncate != Some(false) {
        return None;
    }
    for text in &request.inputs {
        let word_count = text.split_whitespace().count();
        if word_count > FAKE_TEI_MAX_WORDS {
            let body = serde_json::json!({
                "error": format!("Input validation error: `inputs` must have less than {FAKE_TEI_MAX_WORDS} tokens. Given: {word_count}"),
                "error_type": "Validation",
            });
            return Some((StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response());
        }
    }
    None
}

async fn fake_embed(
    State(state): State<Arc<FakeTeiState>>,
    Json(request): Json<EmbedRequest>,
) -> Response {
    match *state.mode.lock().unwrap() {
        FakeTeiMode::Broken => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        FakeTeiMode::Overloaded => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        FakeTeiMode::Working => {}
    }
    if let Some(answer) = too_long_answer(&request) {
        return answer;
    }
    let mut memory = state.memory.lock().unwrap();
    let mut vectors = Vec::new();
    for text in request.inputs {
        let position = memory.position_of(&text);
        let mut vector = vec![0.0f32; VECTOR_SIZE];
        let mut near_meaning = text.strip_prefix(NEAR_GUESS_PREFIX);
        if near_meaning.is_none() {
            near_meaning = text.strip_prefix(ENTAILING_GUESS_PREFIX);
        }
        match near_meaning {
            Some(meaning) => {
                let meaning_position = memory.position_of(meaning);
                vector[meaning_position] = NEAR_GUESS_SIMILARITY;
                vector[position] = (1.0 - NEAR_GUESS_SIMILARITY * NEAR_GUESS_SIMILARITY).sqrt();
            }
            None => vector[position] = 1.0,
        }
        memory.encoded_texts.push(text);
        vectors.push(vector);
    }
    Json(vectors).into_response()
}

impl FakeTeiMemory {
    fn position_of(&mut self, text: &str) -> usize {
        let next_position = self.position_of_text.len();
        *self
            .position_of_text
            .entry(text.to_string())
            .or_insert(next_position)
    }
}

pub fn near_guess(meaning: &str) -> String {
    format!("{NEAR_GUESS_PREFIX}{}", normalize_text(meaning))
}

pub fn entailing_guess(meaning: &str) -> String {
    format!("{ENTAILING_GUESS_PREFIX}{}", normalize_text(meaning))
}

#[derive(Deserialize)]
struct PredictRequest {
    inputs: Vec<(String, String)>,
}

fn fake_entailment(guess: &str, meaning: &str) -> f64 {
    if guess.strip_prefix(ENTAILING_GUESS_PREFIX) == Some(meaning) {
        return FAKE_ENTAILMENT;
    }
    FAKE_NO_ENTAILMENT
}

async fn fake_predict(
    State(state): State<Arc<FakeTeiState>>,
    Json(request): Json<PredictRequest>,
) -> Response {
    if *state.mode.lock().unwrap() != FakeTeiMode::Working {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut memory = state.memory.lock().unwrap();
    let mut predictions = Vec::new();
    for (guess, meaning) in request.inputs {
        let entailment = fake_entailment(&guess, &meaning);
        predictions.push(serde_json::json!([
            { "label": "entailment", "score": entailment },
            { "label": "neutral", "score": 1.0 - entailment },
        ]));
        memory.predicted_pairs.push((guess, meaning));
    }
    Json(predictions).into_response()
}

pub struct FakeTei {
    pub url: String,
    state: Arc<FakeTeiState>,
}

impl FakeTei {
    pub async fn start() -> FakeTei {
        let state = Arc::new(FakeTeiState {
            memory: Mutex::new(FakeTeiMemory::default()),
            mode: Mutex::new(FakeTeiMode::Working),
        });
        let app = Router::new()
            .route("/embed", post(fake_embed))
            .route("/predict", post(fake_predict))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeTei { url, state }
    }

    pub fn set_mode(&self, mode: FakeTeiMode) {
        *self.state.mode.lock().unwrap() = mode;
    }

    pub fn predicted_pairs(&self) -> Vec<(String, String)> {
        self.state.memory.lock().unwrap().predicted_pairs.clone()
    }

    pub fn encoded_texts(&self) -> Vec<String> {
        self.state.memory.lock().unwrap().encoded_texts.clone()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum FakeTurnstileMode {
    Working,
    Down,
}

#[derive(Clone, Deserialize)]
pub struct SiteverifyRequest {
    pub secret: String,
    pub response: String,
    pub remoteip: Option<String>,
}

struct FakeTurnstileState {
    requests: Mutex<Vec<SiteverifyRequest>>,
    mode: Mutex<FakeTurnstileMode>,
}

async fn fake_siteverify(
    State(state): State<Arc<FakeTurnstileState>>,
    Form(request): Form<SiteverifyRequest>,
) -> Response {
    if *state.mode.lock().unwrap() == FakeTurnstileMode::Down {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let answer = siteverify_answer(&request);
    state.requests.lock().unwrap().push(request);
    Json(answer).into_response()
}

fn siteverify_answer(request: &SiteverifyRequest) -> Value {
    if request.secret != TURNSTILE_SECRET_KEY {
        return serde_json::json!({ "success": false });
    }
    match request.response.as_str() {
        VALID_TURNSTILE_TOKEN => serde_json::json!({
            "success": true,
            "hostname": TURNSTILE_HOSTNAME,
            "action": "start_game",
        }),
        OTHER_SITE_TURNSTILE_TOKEN => serde_json::json!({
            "success": true,
            "hostname": "baska-site.example",
            "action": "start_game",
        }),
        OTHER_ACTION_TURNSTILE_TOKEN => serde_json::json!({
            "success": true,
            "hostname": TURNSTILE_HOSTNAME,
            "action": "login",
        }),
        TESTING_KEY_TURNSTILE_TOKEN => serde_json::json!({
            "success": true,
            "hostname": "example.com",
            "metadata": { "result_with_testing_key": true },
        }),
        _ => serde_json::json!({ "success": false }),
    }
}

pub struct FakeTurnstile {
    pub verify_url: String,
    state: Arc<FakeTurnstileState>,
}

impl FakeTurnstile {
    pub async fn start() -> FakeTurnstile {
        let state = Arc::new(FakeTurnstileState {
            requests: Mutex::new(Vec::new()),
            mode: Mutex::new(FakeTurnstileMode::Working),
        });
        let app = Router::new()
            .route("/siteverify", post(fake_siteverify))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let verify_url = format!("http://{}/siteverify", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeTurnstile { verify_url, state }
    }

    pub fn set_mode(&self, mode: FakeTurnstileMode) {
        *self.state.mode.lock().unwrap() = mode;
    }

    pub fn requests(&self) -> Vec<SiteverifyRequest> {
        self.state.requests.lock().unwrap().clone()
    }
}

pub struct TestServer {
    pub state: Arc<AppState>,
    pub tei: Arc<FakeTei>,
    pub turnstile: Arc<FakeTurnstile>,
    pub data_directory: Arc<TempDir>,
    app: Router,
}

fn test_config(tei: &FakeTei, turnstile: &FakeTurnstile, word_file: PathBuf) -> Config {
    Config {
        bind_address: "127.0.0.1:0".to_string(),
        tei_url: tei.url.clone(),
        nli_url: None,
        word_file,
        admin_token: Some(ADMIN_TOKEN.to_string()),
        client_ip_header: Some(axum::http::HeaderName::from_static(CLIENT_IP_HEADER)),
        turnstile_site_key: TURNSTILE_SITE_KEY.to_string(),
        turnstile_secret_key: TURNSTILE_SECRET_KEY.to_string(),
        turnstile_hostname: TURNSTILE_HOSTNAME.to_string(),
        turnstile_verify_url: turnstile.verify_url.clone(),
    }
}

pub struct ApiResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl TestServer {
    pub async fn empty() -> TestServer {
        TestServer::empty_with(|_| {}).await
    }

    pub async fn empty_with(adjust: impl Fn(&mut Config)) -> TestServer {
        let tei = Arc::new(FakeTei::start().await);
        let turnstile = Arc::new(FakeTurnstile::start().await);
        let data_directory = Arc::new(TempDir::new().unwrap());
        let clock = Clock::manual(game_day_noon());
        TestServer::build(tei, turnstile, data_directory, clock, adjust)
    }

    pub async fn with_word() -> TestServer {
        let server = TestServer::empty().await;
        assert_eq!(server.publish(&first_puzzle()).await.status, StatusCode::OK);
        server
    }

    fn build(
        tei: Arc<FakeTei>,
        turnstile: Arc<FakeTurnstile>,
        data_directory: Arc<TempDir>,
        clock: Clock,
        adjust: impl Fn(&mut Config),
    ) -> TestServer {
        let word_file = data_directory.path().join("current_word.json");
        let mut config = test_config(&tei, &turnstile, word_file);
        adjust(&mut config);
        let state = Arc::new(AppState::new(config, clock));
        let app = router(state.clone());
        TestServer {
            state,
            tei,
            turnstile,
            data_directory,
            app,
        }
    }

    pub async fn restarted(&self, clock: Clock) -> TestServer {
        let server = TestServer::build(
            self.tei.clone(),
            self.turnstile.clone(),
            self.data_directory.clone(),
            clock,
            |_| {},
        );
        server
            .state
            .word_store
            .load_saved_word(server.state.clock.unix_seconds())
            .await
            .unwrap();
        server
    }

    pub fn word_file(&self) -> PathBuf {
        self.data_directory.path().join("current_word.json")
    }

    pub fn advance_clock(&self, seconds: f64) {
        self.state.clock.advance(seconds);
    }

    pub async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> ApiResponse {
        self.call_from(PLAYER_IP, method, path, body, headers).await
    }

    pub async fn call_from(
        &self,
        client_ip: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> ApiResponse {
        let mut builder = Request::builder().method(method).uri(path);
        builder = builder.header(CLIENT_IP_HEADER, client_ip);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        ApiResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, path: &str) -> ApiResponse {
        self.call(Method::GET, path, None, &[]).await
    }

    pub async fn post(&self, path: &str, body: Value) -> ApiResponse {
        self.call(Method::POST, path, Some(body), &[]).await
    }

    pub async fn publish_with_token(&self, puzzle: &Value, token: Option<&str>) -> ApiResponse {
        let mut headers = Vec::new();
        if let Some(token) = token {
            headers.push(("x-admin-token", token));
        }
        self.call(
            Method::POST,
            "/api/admin/word",
            Some(puzzle.clone()),
            &headers,
        )
        .await
    }

    pub async fn admin_post(&self, path: &str, puzzle: &Puzzle) -> ApiResponse {
        let body = serde_json::to_value(puzzle).unwrap();
        self.call(
            Method::POST,
            path,
            Some(body),
            &[("x-admin-token", ADMIN_TOKEN)],
        )
        .await
    }

    pub async fn publish(&self, puzzle: &Puzzle) -> ApiResponse {
        self.publish_with_token(&serde_json::to_value(puzzle).unwrap(), Some(ADMIN_TOKEN))
            .await
    }

    pub async fn start_game_as(&self, player_id: &str, token: &str) -> ApiResponse {
        let body = serde_json::json!({ "turnstile_token": token, "player_id": player_id });
        self.post("/api/games", body).await
    }

    pub async fn start_game_with_token(&self, token: &str) -> ApiResponse {
        self.start_game_as(&new_player_id(), token).await
    }

    pub async fn player_today(&self, player_id: &str) -> ApiResponse {
        self.get(&format!("/api/players/{player_id}/today")).await
    }

    pub async fn start_game(&self) -> Value {
        let response = self.start_game_with_token(VALID_TURNSTILE_TOKEN).await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        response.body
    }

    pub async fn start_game_id(&self) -> String {
        self.start_game().await["game_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub async fn guess(&self, game_id: &str, text: &str) -> ApiResponse {
        self.post(
            &format!("/api/games/{game_id}/guesses"),
            serde_json::json!({ "text": text }),
        )
        .await
    }

    pub async fn guess_from(&self, game_id: &str, text: &str, client_ip: &str) -> ApiResponse {
        let body = serde_json::json!({ "text": text });
        let path = format!("/api/games/{game_id}/guesses");
        self.call_from(client_ip, Method::POST, &path, Some(body), &[])
            .await
    }

    pub async fn finish(&self, game_id: &str) -> ApiResponse {
        self.post(
            &format!("/api/games/{game_id}/finish"),
            serde_json::json!({}),
        )
        .await
    }
}
