use std::fmt;
use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

pub const MAX_TEXTS_PER_REQUEST: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum EncoderError {
    Overloaded,
    TooLong,
    Failed(String),
}

impl fmt::Display for EncoderError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EncoderError::Overloaded => write!(formatter, "yapay zekâ sunucusu çok yoğun"),
            EncoderError::TooLong => write!(formatter, "metin modelin okuyabileceğinden uzun"),
            EncoderError::Failed(reason) => {
                write!(formatter, "yapay zekâ sunucusu yanıt vermedi: {reason}")
            }
        }
    }
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
    normalize: bool,
    truncate: bool,
}

#[derive(Deserialize)]
struct TeiErrorBody {
    error: String,
    error_type: String,
}

fn says_input_is_too_long(body: &TeiErrorBody) -> bool {
    body.error_type == "Validation" && body.error.contains("tokens")
}

pub struct TeiEncoder {
    http: reqwest::Client,
    embed_url: String,
}

impl TeiEncoder {
    pub fn new(base_url: &str) -> TeiEncoder {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("HTTP istemcisi kurulamadı");
        TeiEncoder {
            http,
            embed_url: format!("{}/embed", base_url.trim_end_matches('/')),
        }
    }

    pub async fn encode(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let mut vectors = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_TEXTS_PER_REQUEST) {
            vectors.extend(self.encode_chunk(chunk).await?);
        }
        Ok(vectors)
    }

    async fn encode_chunk(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let request = EmbedRequest {
            inputs: texts,
            normalize: true,
            truncate: false,
        };
        let response = match self.http.post(&self.embed_url).json(&request).send().await {
            Ok(response) => response,
            Err(error) if error.is_timeout() => return Err(EncoderError::Overloaded),
            Err(error) => return Err(EncoderError::Failed(error.to_string())),
        };

        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(EncoderError::Overloaded);
        }
        if status == StatusCode::UNPROCESSABLE_ENTITY {
            return match response.json::<TeiErrorBody>().await {
                Ok(body) if says_input_is_too_long(&body) => Err(EncoderError::TooLong),
                Ok(body) => Err(EncoderError::Failed(body.error)),
                Err(error) => Err(EncoderError::Failed(error.to_string())),
            };
        }
        if !status.is_success() {
            return Err(EncoderError::Failed(format!("HTTP {status}")));
        }

        let vectors: Vec<Vec<f32>> = match response.json().await {
            Ok(vectors) => vectors,
            Err(error) => return Err(EncoderError::Failed(error.to_string())),
        };
        if vectors.len() != texts.len() {
            return Err(EncoderError::Failed(format!(
                "{} metin için {} vektör geldi",
                texts.len(),
                vectors.len()
            )));
        }
        Ok(vectors)
    }
}
