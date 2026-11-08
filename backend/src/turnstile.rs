use std::time::Duration;

use serde::Deserialize;

pub const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
pub const START_GAME_ACTION: &str = "start_game";
const MAX_TOKEN_LENGTH: usize = 2048;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
pub enum HumanCheck {
    Passed,
    Failed,
    Unavailable,
}

#[derive(Deserialize, Default)]
struct SiteverifyMetadata {
    #[serde(default)]
    result_with_testing_key: bool,
}

#[derive(Deserialize)]
struct SiteverifyResponse {
    success: bool,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    metadata: SiteverifyMetadata,
}

pub struct TurnstileVerifier {
    http: reqwest::Client,
    secret_key: String,
    verify_url: String,
    expected_hostname: String,
}

impl TurnstileVerifier {
    pub fn new(
        secret_key: String,
        verify_url: String,
        expected_hostname: String,
    ) -> TurnstileVerifier {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("HTTP istemcisi kurulamadı");
        TurnstileVerifier {
            http,
            secret_key,
            verify_url,
            expected_hostname,
        }
    }

    fn judge_answer(&self, answer: &SiteverifyResponse) -> HumanCheck {
        if !answer.success {
            return HumanCheck::Failed;
        }
        if answer.metadata.result_with_testing_key {
            return HumanCheck::Passed;
        }
        if answer.hostname != self.expected_hostname {
            eprintln!(
                "Turnstile jetonu başka bir alan adından geldi: {}",
                answer.hostname
            );
            return HumanCheck::Failed;
        }
        if answer.action != START_GAME_ACTION {
            eprintln!(
                "Turnstile jetonu başka bir eylem için alınmış: {}",
                answer.action
            );
            return HumanCheck::Failed;
        }
        HumanCheck::Passed
    }

    pub async fn verify(&self, token: &str, client_ip: Option<&str>) -> HumanCheck {
        if token.is_empty() || token.len() > MAX_TOKEN_LENGTH {
            return HumanCheck::Failed;
        }

        let mut form = vec![("secret", self.secret_key.as_str()), ("response", token)];
        if let Some(client_ip) = client_ip {
            form.push(("remoteip", client_ip));
        }

        let response = match self.http.post(&self.verify_url).form(&form).send().await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("Turnstile'a ulaşılamadı: {error}");
                return HumanCheck::Unavailable;
            }
        };
        if !response.status().is_success() {
            eprintln!("Turnstile HTTP {} döndü", response.status());
            return HumanCheck::Unavailable;
        }

        match response.json::<SiteverifyResponse>().await {
            Ok(answer) => self.judge_answer(&answer),
            Err(error) => {
                eprintln!("Turnstile yanıtı okunamadı: {error}");
                HumanCheck::Unavailable
            }
        }
    }
}
