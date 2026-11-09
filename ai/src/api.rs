use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

use crate::inference::{InferenceError, InferenceQueue, Task, TokenizedText};

pub struct Model {
    pub task: Task,
    pub tokenizer: Tokenizer,
    pub max_tokens: usize,
    pub labels: Vec<String>,
    pub queue: InferenceQueue,
}

#[derive(Deserialize)]
struct EmbedRequest {
    inputs: Vec<String>,
    #[serde(default = "normalize_by_default")]
    normalize: bool,
}

fn normalize_by_default() -> bool {
    true
}

#[derive(Deserialize)]
struct PredictRequest {
    inputs: Vec<[String; 2]>,
}

#[derive(Serialize)]
struct LabelScore {
    label: String,
    score: f32,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    error_type: &'static str,
}

enum ApiError {
    TooLong { tokens: usize, max_tokens: usize },
    Tokenizer(String),
    Overloaded,
    Failed(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, error) = match self {
            ApiError::TooLong { tokens, max_tokens } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Validation",
                format!("`inputs` must have less than {max_tokens} tokens. Given: {tokens}"),
            ),
            ApiError::Tokenizer(reason) => (StatusCode::UNPROCESSABLE_ENTITY, "Tokenizer", reason),
            ApiError::Overloaded => (
                StatusCode::TOO_MANY_REQUESTS,
                "Overloaded",
                "Model is overloaded".to_string(),
            ),
            ApiError::Failed(reason) => (StatusCode::INTERNAL_SERVER_ERROR, "Backend", reason),
        };
        (status, Json(ErrorBody { error, error_type })).into_response()
    }
}

impl From<InferenceError> for ApiError {
    fn from(error: InferenceError) -> ApiError {
        match error {
            InferenceError::Overloaded => ApiError::Overloaded,
            InferenceError::Failed(reason) => ApiError::Failed(reason),
        }
    }
}

fn to_tokenized_text(encoding: &tokenizers::Encoding) -> TokenizedText {
    let mut ids = Vec::with_capacity(encoding.len());
    for id in encoding.get_ids() {
        ids.push(*id as i64);
    }
    let mut type_ids = Vec::with_capacity(encoding.len());
    for type_id in encoding.get_type_ids() {
        type_ids.push(*type_id as i64);
    }
    TokenizedText { ids, type_ids }
}

fn tokenize_text(model: &Model, text: &str) -> Result<TokenizedText, ApiError> {
    let encoding = match model.tokenizer.encode(text, true) {
        Ok(encoding) => encoding,
        Err(error) => return Err(ApiError::Tokenizer(error.to_string())),
    };
    if encoding.len() > model.max_tokens {
        return Err(ApiError::TooLong {
            tokens: encoding.len(),
            max_tokens: model.max_tokens,
        });
    }
    Ok(to_tokenized_text(&encoding))
}

fn tokenize_pair(model: &Model, first: &str, second: &str) -> Result<TokenizedText, ApiError> {
    match model.tokenizer.encode((first, second), true) {
        Ok(encoding) => Ok(to_tokenized_text(&encoding)),
        Err(error) => Err(ApiError::Tokenizer(error.to_string())),
    }
}

fn normalize(vector: &mut [f32]) {
    let mut squared_sum = 0.0;
    for value in vector.iter() {
        squared_sum += value * value;
    }
    let length = f32::sqrt(squared_sum);
    if length == 0.0 {
        return;
    }
    for value in vector.iter_mut() {
        *value /= length;
    }
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let mut largest = f32::NEG_INFINITY;
    for logit in logits {
        largest = largest.max(*logit);
    }
    let mut exponents = Vec::with_capacity(logits.len());
    let mut sum = 0.0;
    for logit in logits {
        let exponent = f32::exp(logit - largest);
        exponents.push(exponent);
        sum += exponent;
    }
    for exponent in exponents.iter_mut() {
        *exponent /= sum;
    }
    exponents
}

async fn embed(
    State(model): State<Arc<Model>>,
    Json(request): Json<EmbedRequest>,
) -> Result<Json<Vec<Vec<f32>>>, ApiError> {
    let mut texts = Vec::with_capacity(request.inputs.len());
    for input in &request.inputs {
        texts.push(tokenize_text(&model, input)?);
    }
    let mut vectors = model.queue.run(texts).await?;
    if request.normalize {
        for vector in vectors.iter_mut() {
            normalize(vector);
        }
    }
    Ok(Json(vectors))
}

async fn predict(
    State(model): State<Arc<Model>>,
    Json(request): Json<PredictRequest>,
) -> Result<Json<Vec<Vec<LabelScore>>>, ApiError> {
    let mut pairs = Vec::with_capacity(request.inputs.len());
    for [first, second] in &request.inputs {
        pairs.push(tokenize_pair(&model, first, second)?);
    }
    let all_logits = model.queue.run(pairs).await?;

    let mut predictions = Vec::with_capacity(all_logits.len());
    for logits in all_logits {
        let scores = softmax(&logits);
        let mut prediction = Vec::with_capacity(scores.len());
        for (label, score) in model.labels.iter().zip(scores) {
            prediction.push(LabelScore {
                label: label.clone(),
                score,
            });
        }
        prediction.sort_by(|first, second| second.score.total_cmp(&first.score));
        predictions.push(prediction);
    }
    Ok(Json(predictions))
}

async fn health() -> StatusCode {
    StatusCode::OK
}

pub fn router(model: Arc<Model>) -> Router {
    let mut router = Router::new().route("/health", get(health));
    match model.task {
        Task::Embed => router = router.route("/embed", post(embed)),
        Task::Classify => router = router.route("/predict", post(predict)),
    }
    router.with_state(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_vector_has_unit_length() {
        let mut vector = vec![3.0, 4.0];
        normalize(&mut vector);
        assert_eq!(vector, vec![0.6, 0.8]);
    }

    #[test]
    fn softmax_scores_add_up_to_one() {
        let scores = softmax(&[2.0, 1.0, 0.1]);
        let sum: f32 = scores.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        assert!(scores[0] > scores[1] && scores[1] > scores[2]);
    }

    #[test]
    fn softmax_survives_large_logits() {
        let scores = softmax(&[1000.0, 0.0]);
        assert!((scores[0] - 1.0).abs() < 1e-6);
    }
}
