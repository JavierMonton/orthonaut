use std::{net::SocketAddr, path::{Path, PathBuf}, sync::Arc};

use axum::{routing::{delete, get, post}, Router};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod checker;
mod config;
mod db;
mod extractor;
mod reporter;
mod wikipedia;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "ortobot_backend=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_path = std::env::var("ORTOBOT_DB_PATH").unwrap_or_else(|_| "ortobot.db".to_string());
    db::init_db(&db_path)?;

    let dictionary_dir = std::env::var("ORTOBOT_DICT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("dictionaries"));
    let mut checker = checker::SpellChecker::new(&dictionary_dir)?;
    let ignored_words = db::list_ignored_words(&db_path)?;
    checker.add_ignored_words(ignored_words);
    let suppressions_path = checker::suppressions_path(&dictionary_dir)
        .to_string_lossy()
        .to_string();

    let config_path = std::env::var("ORTOBOT_CONFIG_PATH").unwrap_or_else(|_| "ortobot.toml".to_string());
    let config_path = Path::new(&config_path);
    let app_config = config::OrtobotConfig::load(config_path)?;
    tracing::info!(
        path = %app_config.path.display(),
        "loaded Ortobot config"
    );

    let wikimedia_ua = wikipedia::wikimedia_http_user_agent(&app_config.wikimedia_contact);
    tracing::info!(user_agent = %wikimedia_ua, "Wikimedia User-Agent");
    let http_client = reqwest::Client::builder()
        .user_agent(&wikimedia_ua)
        .build()?;

    let state = api::AppState {
        db_path: Arc::new(db_path),
        suppressions_path: Arc::new(suppressions_path),
        http_client,
        checker: Arc::new(Mutex::new(checker)),
        wikimedia_contact: Arc::new(app_config.wikimedia_contact),
    };

    let app = Router::new()
        .route("/api/check", post(api::check_url))
        .route("/api/check/random", post(api::check_random_page))
        .route("/api/sandbox/check", post(api::sandbox_check))
        .route("/api/ignored-words", get(api::list_ignored_words))
        .route("/api/ignored-words", post(api::add_ignored_word))
        .route("/api/ignored-words/export", post(api::export_ignored_words))
        .route("/api/ignored-words/:word", delete(api::delete_ignored_word))
        .route("/api/results", get(api::list_results))
        .route("/api/results/:id", delete(api::delete_result))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("backend listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
