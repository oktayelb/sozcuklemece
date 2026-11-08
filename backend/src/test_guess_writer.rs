use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::puzzle::Puzzle;

pub const MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const RIGHT_GUESS_COUNT: usize = 15;
const WRONG_GUESS_COUNT: usize = 15;

#[derive(Debug)]
pub enum WriterError {
    Refused,
    Failed(String),
}

impl fmt::Display for WriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WriterError::Refused => write!(formatter, "model isteği geri çevirdi"),
            WriterError::Failed(reason) => write!(formatter, "{reason}"),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TestGuesses {
    pub right: Vec<String>,
    pub wrong: Vec<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
}

fn prompt_for(puzzle: &Puzzle) -> String {
    format!(
        "Sözcüklemece is a daily Turkish word game. Players see an invented Turkish word and type what they think it means. \
A model compares each guess with the definition and decides whether it wins.\n\n\
Write test guesses for the word below, the way real players type them: short (3 to 12 words), lowercase, casual Turkish, no quotation marks. \
Never use the word itself.\n\n\
Word: {word}\n\
Definition: {definition}\n\
Parts: {parts}\n\
Example sentence: {sentence}\n\
Category: {category}, {field}\n\n\
\"right\": {right_count} guesses that mean the same as the definition in other words. \
About half should describe a concrete situation or example instead of giving a definition.\n\
\"wrong\": {wrong_count} guesses a player might type that should not win: \
5 that say the opposite, 5 that share the topic but mean something else, 5 that are too general or miss the key part.",
        word = puzzle.word,
        definition = puzzle.definition,
        parts = puzzle.parts,
        sentence = puzzle.sentence,
        category = puzzle.category,
        field = puzzle.field,
        right_count = RIGHT_GUESS_COUNT,
        wrong_count = WRONG_GUESS_COUNT,
    )
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "right": { "type": "array", "items": { "type": "string" } },
            "wrong": { "type": "array", "items": { "type": "string" } },
        },
        "required": ["right", "wrong"],
        "additionalProperties": false,
    })
}

fn request_body(puzzle: &Puzzle) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "fallbacks": "default",
        "output_config": {
            "effort": "low",
            "format": { "type": "json_schema", "schema": output_schema() },
        },
        "messages": [{ "role": "user", "content": prompt_for(puzzle) }],
    })
}

fn test_guesses_from(response: MessageResponse) -> Result<TestGuesses, WriterError> {
    if response.stop_reason.as_deref() == Some("refusal") {
        return Err(WriterError::Refused);
    }
    for block in response.content {
        if block.kind != "text" {
            continue;
        }
        let Some(text) = block.text else {
            continue;
        };
        return serde_json::from_str(&text).map_err(|error| WriterError::Failed(error.to_string()));
    }
    Err(WriterError::Failed("yanıtta metin yok".to_string()))
}

pub struct TestGuessWriter {
    http: reqwest::Client,
    api_key: String,
    messages_url: String,
}

impl TestGuessWriter {
    pub fn new(api_key: String, messages_url: String) -> TestGuessWriter {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("HTTP istemcisi kurulamadı");
        TestGuessWriter {
            http,
            api_key,
            messages_url,
        }
    }

    pub async fn write(&self, puzzle: &Puzzle) -> Result<TestGuesses, WriterError> {
        let sent = self
            .http
            .post(&self.messages_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&request_body(puzzle))
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(error) => return Err(WriterError::Failed(error.to_string())),
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(WriterError::Failed(format!("HTTP {status}: {body}")));
        }
        match response.json::<MessageResponse>().await {
            Ok(message) => test_guesses_from(message),
            Err(error) => Err(WriterError::Failed(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_response(text: &str, stop_reason: &str) -> MessageResponse {
        MessageResponse {
            content: vec![
                ContentBlock {
                    kind: "thinking".to_string(),
                    text: None,
                },
                ContentBlock {
                    kind: "text".to_string(),
                    text: Some(text.to_string()),
                },
            ],
            stop_reason: Some(stop_reason.to_string()),
        }
    }

    #[test]
    fn json_text_becomes_test_guesses() {
        let response = text_response(r#"{"right":["a b c"],"wrong":["d e f"]}"#, "end_turn");
        assert_eq!(
            test_guesses_from(response).unwrap(),
            TestGuesses {
                right: vec!["a b c".to_string()],
                wrong: vec!["d e f".to_string()],
            }
        );
    }

    #[test]
    fn refusal_is_an_error() {
        let response = text_response("", "refusal");
        assert!(matches!(
            test_guesses_from(response),
            Err(WriterError::Refused)
        ));
    }
}
