mod api;
mod config;
mod engine;
mod error;

use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    response::Html,
    routing::{get, post},
    Router,
};
use config::{AppConfig, VoicesConfig};
use engine::Engine;
use tokio::{net::TcpListener, signal};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub voices: Arc<VoicesConfig>,
    pub engine: Arc<Engine>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Arc::new(AppConfig::from_env().context("invalid environment configuration")?);
    tokio::fs::create_dir_all(&config.work_dir)
        .await
        .context("failed to create TTS work directory")?;
    let voices = Arc::new(
        VoicesConfig::load(&config.voices_path)
            .await
            .context("failed to load voices configuration")?,
    );
    let engine = Arc::new(Engine::new(config.clone()));
    let healthy = engine.smoke_test(&voices).await;
    engine.set_healthy(healthy);
    if !healthy {
        error!("startup synthesis smoke test failed; health is degraded");
    }

    let state = AppState {
        config,
        voices,
        engine,
    };
    let app = Router::new()
        .route("/", get(client))
        .route("/healthz", get(api::healthz))
        .route("/api/voices", get(api::voices))
        .route("/api/tts", post(api::tts))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api::request_context,
        ))
        .with_state(state);

    let address: SocketAddr = env::var("TTS_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse()
        .context("invalid TTS_LISTEN")?;
    let listener = TcpListener::bind(address).await?;
    info!(%address, "TTS API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn client() -> Html<&'static str> {
    Html(include_str!("client.html"))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            error!(%error, "failed to install Ctrl-C handler");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => error!(%error, "failed to install SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    tokio::time::sleep(Duration::from_millis(25)).await;
}
