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

use sozcuklemece_backend::api::{AppState, router};
use sozcuklemece_backend::clock::Clock;
use sozcuklemece_backend::config::Config;
use sozcuklemece_backend::puzzle::Puzzle;

pub const ADMIN_TOKEN: &str = "deneme-parolasi";
pub const CLIENT_IP_HEADER: &str = "x-real-ip";
pub const PLAYER_IP: &str = "203.0.113.5";
pub const VALID_TURNSTILE_TOKEN: &str = "gecerli-jeton";
pub const TURNSTILE_SITE_KEY: &str = "deneme-site-anahtari";
const TURNSTILE_SECRET_KEY: &str = "deneme-gizli-anahtar";
const VECTOR_SIZE: usize = 1024;

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
}

struct FakeTeiState {
    memory: Mutex<FakeTeiMemory>,
    mode: Mutex<FakeTeiMode>,
}

#[derive(Deserialize)]
struct EmbedRequest {
    inputs: Vec<String>,
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
    let mut memory = state.memory.lock().unwrap();
    let mut vectors = Vec::new();
    for text in request.inputs {
        let next_position = memory.position_of_text.len();
        let position = *memory
            .position_of_text
            .entry(text.clone())
            .or_insert(next_position);
        memory.encoded_texts.push(text);
        let mut vector = vec![0.0f32; VECTOR_SIZE];
        vector[position] = 1.0;
        vectors.push(vector);
    }
    Json(vectors).into_response()
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
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeTei { url, state }
    }

    pub fn set_mode(&self, mode: FakeTeiMode) {
        *self.state.mode.lock().unwrap() = mode;
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
    let success =
        request.secret == TURNSTILE_SECRET_KEY && request.response == VALID_TURNSTILE_TOKEN;
    state.requests.lock().unwrap().push(request);
    Json(serde_json::json!({ "success": success })).into_response()
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
        word_file,
        admin_token: Some(ADMIN_TOKEN.to_string()),
        client_ip_header: Some(axum::http::HeaderName::from_static(CLIENT_IP_HEADER)),
        turnstile_site_key: TURNSTILE_SITE_KEY.to_string(),
        turnstile_secret_key: TURNSTILE_SECRET_KEY.to_string(),
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

    pub async fn publish(&self, puzzle: &Puzzle) -> ApiResponse {
        self.publish_with_token(&serde_json::to_value(puzzle).unwrap(), Some(ADMIN_TOKEN))
            .await
    }

    pub async fn start_game_with_token(&self, token: &str) -> ApiResponse {
        let body = serde_json::json!({ "turnstile_token": token });
        self.post("/api/games", body).await
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
