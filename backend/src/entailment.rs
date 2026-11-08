use std::time::Duration;

use serde::{Deserialize, Serialize};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const ENTAILMENT_LABEL: &str = "entailment";

#[derive(Serialize)]
struct PredictRequest<'a> {
    inputs: Vec<[&'a str; 2]>,
    truncate: bool,
}

#[derive(Deserialize)]
struct LabelScore {
    label: String,
    score: f64,
}

pub struct EntailmentChecker {
    http: reqwest::Client,
    predict_url: String,
}

impl EntailmentChecker {
    pub fn new(base_url: &str) -> EntailmentChecker {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("HTTP istemcisi kurulamadı");
        EntailmentChecker {
            http,
            predict_url: format!("{}/predict", base_url.trim_end_matches('/')),
        }
    }

    pub async fn highest_entailment(
        &self,
        guess: &str,
        meanings: &[String],
    ) -> Result<f64, String> {
        let mut pairs = Vec::new();
        for meaning in meanings {
            pairs.push([guess, meaning.as_str()]);
        }
        let request = PredictRequest {
            inputs: pairs,
            truncate: true,
        };
        let response = match self
            .http
            .post(&self.predict_url)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return Err(error.to_string()),
        };
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let predictions: Vec<Vec<LabelScore>> = match response.json().await {
            Ok(predictions) => predictions,
            Err(error) => return Err(error.to_string()),
        };

        let mut highest = 0.0;
        for prediction in predictions {
            for label_score in prediction {
                if label_score.label == ENTAILMENT_LABEL {
                    highest = f64::max(highest, label_score.score);
                }
            }
        }
        Ok(highest)
    }
}
