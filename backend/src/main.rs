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

// Release builds: embed the whole dictionaries/ folder into the binary.
// es_ES.{aff,dic} are read straight from here. suppressions.txt / always_wrong.txt
// are seeded onto disk on first boot (see seed_word_lists) so they stay user-editable
// and survive exports across restarts.
#[cfg(not(debug_assertions))]
mod dicts {
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "dictionaries"]
    struct Files;

    fn decode(bytes: &[u8]) -> String {
        match String::from_utf8(bytes.to_vec()) {
            Ok(s) => s,
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        }
    }

    pub fn aff() -> String {
        decode(&Files::get("es_ES.aff").expect("embedded aff").data)
    }

    pub fn dic() -> String {
        decode(&Files::get("es_ES.dic").expect("embedded dic").data)
    }

    pub fn raw(name: &str) -> Option<Vec<u8>> {
        Files::get(name).map(|f| f.data.into_owned())
    }
}

// Release builds: embed the compiled frontend into the binary for self-contained deployment.
#[cfg(not(debug_assertions))]
mod frontend {
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "../frontend/dist"]
    pub struct Assets;

    pub async fn handler(uri: axum::http::Uri) -> axum::response::Response {
        use axum::response::IntoResponse;
        let path = uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        match Assets::get(path) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                ([(axum::http::header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
            }
            // SPA fallback: unknown paths → index.html so React Router handles routing
            None => {
                let content = Assets::get("index.html").unwrap();
                ([(axum::http::header::CONTENT_TYPE, "text/html")], content.data).into_response()
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "orthonaut_backend=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

    let db_path = std::env::var("ORTHONAUT_DB_PATH")
        .unwrap_or_else(|_| format!("{}/orthonaut.db", home));
    db::init_db(&db_path)?;

    let dictionary_dir = std::env::var("ORTHONAUT_DICT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("{}/dictionaries", home)));

    // Release: Hunspell files are embedded in the binary — no files to upload.
    // Seed the curated word lists onto disk on first boot so they're loaded by the
    // checker below and remain editable / persisted across exports.
    #[cfg(not(debug_assertions))]
    {
        std::fs::create_dir_all(&dictionary_dir).ok();
        for name in ["suppressions.txt", "always_wrong.txt"] {
            let dest = dictionary_dir.join(name);
            if !dest.exists() {
                if let Some(bytes) = dicts::raw(name) {
                    if let Err(e) = std::fs::write(&dest, bytes) {
                        tracing::warn!(file = name, error = %e, "failed to seed word list");
                    } else {
                        tracing::info!(file = name, "seeded word list to disk");
                    }
                }
            }
        }
    }

    // Release: build dictionary from embedded data. Debug: load from disk (local dev with setup.sh).
    #[cfg(not(debug_assertions))]
    let mut checker = checker::SpellChecker::from_strs(&dicts::aff(), &dicts::dic(), &dictionary_dir)?;
    #[cfg(debug_assertions)]
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

    let config_path_str = std::env::var("ORTHONAUT_CONFIG_PATH")
        .unwrap_or_else(|_| format!("{}/orthonaut.toml", home));
    let config_path = Path::new(&config_path_str);
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

    // Release: serve embedded frontend; Debug: serve from disk (falls back to 404 if not built)
    #[cfg(not(debug_assertions))]
    let app = app.fallback(frontend::handler);

    #[cfg(debug_assertions)]
    let app = {
        use tower_http::services::{ServeDir, ServeFile};
        let dist = std::env::var("ORTHONAUT_STATIC_DIR")
            .unwrap_or_else(|_| "../frontend/dist".to_string());
        app.fallback_service(
            ServeDir::new(&dist)
                .not_found_service(ServeFile::new(format!("{}/index.html", dist))),
        )
    };

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("backend listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
