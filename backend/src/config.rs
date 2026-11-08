use std::env;
use std::path::PathBuf;

use axum::http::HeaderName;

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:8000";
const DEFAULT_TEI_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_WORD_FILE: &str = "data/current_word.json";

pub struct Config {
    pub bind_address: String,
    pub tei_url: String,
    pub word_file: PathBuf,
    pub admin_token: Option<String>,
    pub client_ip_header: Option<HeaderName>,
}

fn variable(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

impl Config {
    pub fn from_environment() -> Config {
        let client_ip_header = variable("CLIENT_IP_HEADER").map(|name| {
            HeaderName::try_from(name.trim().to_lowercase())
                .expect("CLIENT_IP_HEADER geçerli bir başlık adı olmalı")
        });
        Config {
            bind_address: variable("BIND_ADDRESS").unwrap_or(DEFAULT_BIND_ADDRESS.to_string()),
            tei_url: variable("TEI_URL").unwrap_or(DEFAULT_TEI_URL.to_string()),
            word_file: PathBuf::from(
                variable("WORD_FILE").unwrap_or(DEFAULT_WORD_FILE.to_string()),
            ),
            admin_token: variable("ADMIN_TOKEN"),
            client_ip_header,
        }
    }
}
