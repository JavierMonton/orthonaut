use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{
    checker::SpellChecker,
    db,
    extractor,
    reporter::{self, ArticleResult, CheckResponse},
    wikipedia,
};

#[derive(Clone)]
pub struct AppState {
    pub db_path: Arc<String>,
    pub http_client: reqwest::Client,
    pub checker: Arc<Mutex<SpellChecker>>,
}

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct SandboxCheckRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct AddIgnoredWordRequest {
    pub word: String,
}

#[derive(Debug, Serialize)]
pub struct IgnoredWordsResponse {
    pub words: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SandboxCheckResponse {
    pub status: String,
    pub wrong_words: Vec<String>,
    pub total_words: usize,
    pub misspelled_count: usize,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        let body = Json(serde_json::json!({ "error": message }));
        (status, body).into_response()
    }
}

pub async fn check_url(
    State(state): State<AppState>,
    Json(payload): Json<CheckRequest>,
) -> Result<Json<CheckResponse>, ApiError> {
    if payload.url.trim().is_empty() {
        return Err(ApiError::BadRequest("url is required".to_string()));
    }

    let (fetch_url, display_url) = normalize_input_url(payload.url.trim())?;

    let page = match wikipedia::fetch_page(&state.http_client, &fetch_url).await {
        Ok(page) => page,
        Err(wikipedia::WikipediaError::UpstreamStatus(code))
            if code == reqwest::StatusCode::FORBIDDEN && fetch_url != display_url =>
        {
            // Some upstream edge nodes may reject REST HTML for specific requests.
            // Retry with the canonical article URL so `/wiki/...` inputs keep working.
            wikipedia::fetch_page(&state.http_client, &display_url)
                .await
                .map_err(|e| ApiError::BadRequest(e.to_string()))?
        }
        Err(e) => return Err(ApiError::BadRequest(e.to_string())),
    };

    let tokens = extractor::extract_tokens(&page.html);
    let mut checker = state.checker.lock().await;
    let wrong_words = checker.find_wrong_words_from_tokens(&tokens);
    drop(checker);

    if wrong_words.is_empty() {
        return Ok(Json(reporter::ok_message(page.title)));
    }

    let id = db::insert_article(
        state.db_path.as_str(),
        &page.title,
        &display_url,
        &page.revision_id,
        &wrong_words,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    let result = ArticleResult {
        id,
        title: page.title,
        url: display_url,
        revision_id: page.revision_id,
        wrong_words,
        checked_at: chrono::Utc::now().to_rfc3339(),
    };

    Ok(Json(reporter::errors_found(result)))
}

pub async fn list_results(State(state): State<AppState>) -> Result<Json<Vec<ArticleResult>>, ApiError> {
    let records = db::list_articles(state.db_path.as_str()).map_err(|e| ApiError::Internal(e.to_string()))?;
    let mapped = records.into_iter().map(ArticleResult::from).collect();
    Ok(Json(mapped))
}

pub async fn list_ignored_words(
    State(state): State<AppState>,
) -> Result<Json<IgnoredWordsResponse>, ApiError> {
    let words = db::list_ignored_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(IgnoredWordsResponse { words }))
}

pub async fn add_ignored_word(
    State(state): State<AppState>,
    Json(payload): Json<AddIgnoredWordRequest>,
) -> Result<StatusCode, ApiError> {
    let normalized = crate::checker::normalize_ignored_word(&payload.word);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    db::insert_ignored_word(state.db_path.as_str(), &normalized)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut checker = state.checker.lock().await;
    checker.add_ignored_word(&normalized);
    drop(checker);
    Ok(StatusCode::CREATED)
}

pub async fn delete_result(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let deleted = db::delete_article(state.db_path.as_str(), id)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if deleted == 0 {
        return Err(ApiError::NotFound("result not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_ignored_word(
    State(state): State<AppState>,
    Path(word): Path<String>,
) -> Result<StatusCode, ApiError> {
    let normalized = crate::checker::normalize_ignored_word(&word);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    db::delete_ignored_word(state.db_path.as_str(), &normalized)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut checker = state.checker.lock().await;
    checker.remove_ignored_word(&normalized);
    drop(checker);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sandbox_check(
    State(state): State<AppState>,
    Json(payload): Json<SandboxCheckRequest>,
) -> Result<Json<SandboxCheckResponse>, ApiError> {
    if payload.content.trim().is_empty() {
        return Err(ApiError::BadRequest("content is required".to_string()));
    }

    let tokens = extractor::extract_tokens_from_input(&payload.content);
    let total_words = tokens.len();

    let mut checker = state.checker.lock().await;
    let wrong_words = checker.find_wrong_words_from_tokens(&tokens);
    drop(checker);

    Ok(Json(SandboxCheckResponse {
        status: "ok".to_string(),
        misspelled_count: wrong_words.len(),
        wrong_words,
        total_words,
    }))
}

fn normalize_input_url(input: &str) -> Result<(String, String), ApiError> {
    let parsed = reqwest::Url::parse(input)
        .map_err(|_| ApiError::BadRequest("invalid url format".to_string()))?;

    let Some(host) = parsed.host_str() else {
        return Ok((input.to_string(), input.to_string()));
    };

    if !host.ends_with("wikipedia.org") {
        return Ok((input.to_string(), input.to_string()));
    }

    let path = parsed.path();
    if let Some(title) = path.strip_prefix("/wiki/") {
        if title.is_empty() {
            return Err(ApiError::BadRequest("wikipedia page url is missing title".to_string()));
        }
        return Ok((
            build_url(&parsed, &format!("/api/rest_v1/page/html/{title}")),
            build_url(&parsed, &format!("/wiki/{title}")),
        ));
    }

    if let Some(title) = path.strip_prefix("/api/rest_v1/page/html/") {
        if title.is_empty() {
            return Err(ApiError::BadRequest("wikipedia rest url is missing title".to_string()));
        }
        return Ok((
            build_url(&parsed, &format!("/api/rest_v1/page/html/{title}")),
            build_url(&parsed, &format!("/wiki/{title}")),
        ));
    }

    Ok((input.to_string(), input.to_string()))
}

fn build_url(base: &reqwest::Url, path: &str) -> String {
    let mut url = base.clone();
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use axum::{extract::{Path as AxumPath, State}, Json};
    use tokio::sync::Mutex;

    use crate::{checker::SpellChecker, db, extractor::ExtractedToken};

    use super::{
        add_ignored_word, delete_ignored_word, list_ignored_words, normalize_input_url,
        AddIgnoredWordRequest, AppState,
    };

    #[test]
    fn converts_wiki_page_url_to_rest_html_url() {
        let (fetch, display) =
            normalize_input_url("https://es.wikipedia.org/wiki/Madrid").expect("valid url");
        assert_eq!(fetch, "https://es.wikipedia.org/api/rest_v1/page/html/Madrid");
        assert_eq!(display, "https://es.wikipedia.org/wiki/Madrid");
    }

    #[test]
    fn keeps_rest_url_and_derives_display_url() {
        let (fetch, display) = normalize_input_url(
            "https://es.wikipedia.org/api/rest_v1/page/html/Madrid?redirect=no",
        )
        .expect("valid url");
        assert_eq!(fetch, "https://es.wikipedia.org/api/rest_v1/page/html/Madrid");
        assert_eq!(display, "https://es.wikipedia.org/wiki/Madrid");
    }

    fn temp_db_path() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid clock")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("ortobot-api-test-{nanos}.db"))
            .to_string_lossy()
            .to_string()
    }

    async fn build_state() -> AppState {
        let db_path = temp_db_path();
        db::init_db(&db_path).expect("db init should work");
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let ignored = db::list_ignored_words(&db_path).expect("ignored words list should work");
        checker.add_ignored_words(ignored);

        AppState {
            db_path: Arc::new(db_path),
            http_client: reqwest::Client::new(),
            checker: Arc::new(Mutex::new(checker)),
        }
    }

    #[tokio::test]
    async fn ignored_words_api_roundtrip_updates_checker_cache() {
        let state = build_state().await;
        let created = add_ignored_word(
            State(state.clone()),
            Json(AddIgnoredWordRequest {
                word: "palabrafalsa".to_string(),
            }),
        )
        .await
        .expect("add ignored word should work");
        assert_eq!(created, axum::http::StatusCode::CREATED);

        let listed = list_ignored_words(State(state.clone()))
            .await
            .expect("list ignored words should work");
        assert!(listed.0.words.contains(&"palabrafalsa".to_string()));

        {
            let mut checker = state.checker.lock().await;
            let before_delete = checker.find_wrong_words_from_tokens(&[ExtractedToken {
                normalized: "palabrafalsa".to_string(),
                saw_uppercase: false,
            }]);
            assert!(!before_delete.contains(&"palabrafalsa".to_string()));
        }

        let deleted = delete_ignored_word(
            State(state.clone()),
            AxumPath("palabrafalsa".to_string()),
        )
        .await
        .expect("delete ignored word should work");
        assert_eq!(deleted, axum::http::StatusCode::NO_CONTENT);

        {
            let mut checker = state.checker.lock().await;
            let after_delete = checker.find_wrong_words_from_tokens(&[ExtractedToken {
                normalized: "palabrafalsa".to_string(),
                saw_uppercase: false,
            }]);
            assert!(after_delete.contains(&"palabrafalsa".to_string()));
        }
    }
}
