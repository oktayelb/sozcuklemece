use std::env;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use sozcuklemece_backend::api::{AppState, router};
use sozcuklemece_backend::clock::Clock;
use sozcuklemece_backend::config::Config;

const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

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

async fn clean_up_periodically(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
    loop {
        interval.tick().await;
        state.remove_expired_state();
    }
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
    let config = Config::from_environment();
    let bind_address = config.bind_address.clone();
    if env::args().nth(1).as_deref() == Some("healthcheck") {
        check_health(&bind_address);
    }
    let state = Arc::new(AppState::new(config, Clock::system()));

    if let Err(error) = state
        .word_store
        .load_saved_word(state.clock.unix_seconds())
        .await
    {
        eprintln!("Kayıtlı sözcük yüklenemedi: {error}");
        process::exit(1);
    }
    tokio::spawn(clean_up_periodically(state.clone()));

    let listener = match TcpListener::bind(&bind_address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{bind_address} adresi dinlenemedi: {error}");
            process::exit(1);
        }
    };
    eprintln!("Sözcüklemece arka ucu {bind_address} adresinde dinliyor.");

    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(error) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        eprintln!("Sunucu durdu: {error}");
        process::exit(1);
    }
}
