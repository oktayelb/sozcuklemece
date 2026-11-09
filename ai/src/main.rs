mod api;
mod inference;
mod model_files;

use std::env;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::api::{Model, router};
use crate::inference::{InferenceQueue, Task};

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:80";
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);
const USAGE: &str = "Kullanım: sozcuklemece-ai embed|classify <model klasörü>\n          sozcuklemece-ai healthcheck";

fn exit_with(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}

fn check_health(bind_address: &str) -> ! {
    let port = match bind_address.parse::<SocketAddr>() {
        Ok(address) => address.port(),
        Err(_) => process::exit(1),
    };
    let local_address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    match TcpStream::connect_timeout(&local_address, HEALTH_CHECK_TIMEOUT) {
        Ok(_) => process::exit(0),
        Err(_) => process::exit(1),
    }
}

fn available_threads() -> usize {
    match thread::available_parallelism() {
        Ok(count) => count.get(),
        Err(_) => 1,
    }
}

fn load_model(task: Task, model_directory: &Path) -> Result<Model, String> {
    let max_tokens = model_files::read_max_tokens(model_directory)?;
    let mut labels = Vec::new();
    let mut truncate_at = None;
    if task == Task::Classify {
        labels = model_files::read_labels(model_directory)?;
        truncate_at = Some(max_tokens);
    }
    let tokenizer = model_files::load_tokenizer(model_directory, truncate_at)?;
    let threads = available_threads();
    let session = model_files::open_session(model_directory, threads)?;
    eprintln!(
        "{} yüklendi, {threads} iş parçacığı, en çok {max_tokens} parça.",
        model_directory.display()
    );
    Ok(Model {
        task,
        tokenizer,
        max_tokens,
        labels,
        queue: InferenceQueue::start(session, task),
    })
}

async fn shutdown_signal() {
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM dinlenemedi");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[tokio::main]
async fn main() {
    let bind_address = env::var("BIND_ADDRESS").unwrap_or(DEFAULT_BIND_ADDRESS.to_string());
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("healthcheck") {
        check_health(&bind_address);
    }
    if arguments.len() != 2 {
        exit_with(USAGE);
    }
    let task = match arguments[0].as_str() {
        "embed" => Task::Embed,
        "classify" => Task::Classify,
        _ => exit_with(USAGE),
    };
    let model_directory = PathBuf::from(&arguments[1]);

    let model = match load_model(task, &model_directory) {
        Ok(model) => model,
        Err(error) => exit_with(&format!("Model yüklenemedi: {error}")),
    };

    let listener = match TcpListener::bind(&bind_address).await {
        Ok(listener) => listener,
        Err(error) => exit_with(&format!("{bind_address} adresi dinlenemedi: {error}")),
    };
    eprintln!("Yapay zekâ sunucusu {bind_address} adresinde dinliyor.");

    if let Err(error) = axum::serve(listener, router(Arc::new(model)))
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        exit_with(&format!("Sunucu durdu: {error}"));
    }
}
