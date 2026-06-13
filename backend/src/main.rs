use std::{collections::HashMap, net::SocketAddr, path::{Path, PathBuf}, sync::Arc};

use axum::{routing::{delete, get, post}, Router};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod checker;
mod config;
mod db;
mod extractor;
mod oauth;
mod reporter;
mod wikipedia;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "orthonaut_backend=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_path = std::env::var("ORTHONAUT_DB_PATH").unwrap_or_else(|_| "orthonaut.db".to_string());
    db::init_db(&db_path)?;

    let dictionary_dir = std::env::var("ORTHONAUT_DICT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("dictionaries"));
    let mut checker = checker::SpellChecker::new(&dictionary_dir)?;
    let ignored_words = db::list_ignored_words(&db_path)?;
    checker.add_ignored_words(ignored_words);
    let always_wrong = db::list_always_wrong_words(&db_path)?;
    checker.add_always_wrong_words(always_wrong);
    let suppressions_path = checker::suppressions_path(&dictionary_dir)
        .to_string_lossy()
        .to_string();
    let always_wrong_path = checker::always_wrong_path(&dictionary_dir)
        .to_string_lossy()
        .to_string();

    let config_path = std::env::var("ORTHONAUT_CONFIG_PATH").unwrap_or_else(|_| "../orthonaut.toml".to_string());
    let config_path = Path::new(&config_path);
    let app_config = config::OrthonautConfig::load(config_path)?;
    tracing::info!(
        path = %app_config.path.display(),
        "loaded Orthonaut config"
    );

    let wikimedia_ua = wikipedia::wikimedia_http_user_agent(&app_config.wikimedia_contact);
    tracing::info!(user_agent = %wikimedia_ua, "Wikimedia User-Agent");
    let http_client = reqwest::Client::builder()
        .user_agent(&wikimedia_ua)
        .build()?;

    if let Some(ref oauth) = app_config.oauth {
        tracing::info!(client_id = %oauth.client_id, "OAuth configured");
    } else {
        tracing::info!("OAuth not configured — Wikipedia editing disabled");
    }

    let oauth_config = app_config.oauth.map(|o| Arc::new(o));

    let state = api::AppState {
        db_path: Arc::new(db_path),
        suppressions_path: Arc::new(suppressions_path),
        always_wrong_path: Arc::new(always_wrong_path),
        http_client,
        checker: Arc::new(Mutex::new(checker)),
        wikimedia_contact: Arc::new(app_config.wikimedia_contact),
        oauth_config,
        oauth_pending_state: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/api/check", post(api::check_url))
        .route("/api/check/random", post(api::check_random_page))
        .route("/api/sandbox/check", post(api::sandbox_check))
        .route("/api/ignored-words", get(api::list_ignored_words))
        .route("/api/ignored-words", post(api::add_ignored_word))
        .route("/api/ignored-words/export", post(api::export_ignored_words))
        .route("/api/ignored-words/:word", delete(api::delete_ignored_word))
        .route("/api/always-wrong-words", get(api::list_always_wrong_words))
        .route("/api/always-wrong-words", post(api::add_always_wrong_word))
        .route("/api/always-wrong-words/export", post(api::export_always_wrong_words))
        .route("/api/always-wrong-words/:word", delete(api::delete_always_wrong_word))
        .route("/api/results", get(api::list_results))
        .route("/api/results/:id", delete(api::delete_result))
        .route("/api/results/:id/words/:word", delete(api::ignore_word_in_result))
        .route("/api/results/:id/contexts/:word", get(api::get_word_contexts))
        .route("/api/edit", post(api::apply_edit))
        .route("/api/search", post(api::search_handler))
        .route("/api/search/contexts", post(api::get_search_contexts))
        .route("/api/search/edit", post(api::apply_search_edit))
        .route("/api/auth/login", get(oauth::auth_login))
        .route("/api/auth/callback", get(oauth::auth_callback))
        .route("/api/auth/status", get(oauth::auth_status))
        .route("/api/auth/logout", post(oauth::auth_logout))
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
