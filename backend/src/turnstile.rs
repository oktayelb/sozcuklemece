use std::time::Duration;

use serde::Deserialize;

pub const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
const MAX_TOKEN_LENGTH: usize = 2048;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
pub enum HumanCheck {
    Passed,
    Failed,
    Unavailable,
}

#[derive(Deserialize)]
struct SiteverifyResponse {
    success: bool,
}

pub struct TurnstileVerifier {
    http: reqwest::Client,
    secret_key: String,
    verify_url: String,
}

impl TurnstileVerifier {
    pub fn new(secret_key: String, verify_url: String) -> TurnstileVerifier {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("HTTP istemcisi kurulamadı");
        TurnstileVerifier {
            http,
            secret_key,
            verify_url,
        }
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
            Ok(body) if body.success => HumanCheck::Passed,
            Ok(_) => HumanCheck::Failed,
            Err(error) => {
                eprintln!("Turnstile yanıtı okunamadı: {error}");
                HumanCheck::Unavailable
            }
        }
    }
}
