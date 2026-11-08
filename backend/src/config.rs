use std::env;
use std::path::PathBuf;
use std::process;

use axum::http::HeaderName;

use crate::turnstile::SITEVERIFY_URL;

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:8000";
const DEFAULT_TEI_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_WORD_FILE: &str = "data/current_word.json";

pub struct Config {
    pub bind_address: String,
    pub tei_url: String,
    pub nli_url: Option<String>,
    pub word_file: PathBuf,
    pub admin_token: Option<String>,
    pub client_ip_header: Option<HeaderName>,
    pub turnstile_site_key: String,
    pub turnstile_secret_key: String,
    pub turnstile_hostname: String,
    pub turnstile_verify_url: String,
}

fn variable(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn required_variable(name: &str) -> String {
    match variable(name) {
        Some(value) => value,
        None => {
            eprintln!("{name} ortam değişkeni ayarlanmalı.");
            process::exit(1);
        }
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
            nli_url: variable("NLI_URL"),
            word_file: PathBuf::from(
                variable("WORD_FILE").unwrap_or(DEFAULT_WORD_FILE.to_string()),
            ),
            admin_token: variable("ADMIN_TOKEN"),
            client_ip_header,
            turnstile_site_key: required_variable("TURNSTILE_SITE_KEY"),
            turnstile_secret_key: required_variable("TURNSTILE_SECRET_KEY"),
            turnstile_hostname: required_variable("TURNSTILE_HOSTNAME"),
            turnstile_verify_url: SITEVERIFY_URL.to_string(),
        }
    }
}
