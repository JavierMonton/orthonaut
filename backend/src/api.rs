use std::{
    collections::{BTreeSet, HashMap},
    fs,
    sync::Arc,
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{
    checker::SpellChecker,
    config::OAuthConfig,
    db,
    extractor,
    oauth,
    reporter::{self, ArticleResult, CheckResponse},
    wikipedia,
};

#[derive(Clone)]
pub struct AppState {
    pub db_path: Arc<String>,
    pub suppressions_path: Arc<String>,
    pub always_wrong_path: Arc<String>,
    pub http_client: reqwest::Client,
    pub checker: Arc<Mutex<SpellChecker>>,
    pub wikimedia_contact: Arc<String>,
    pub oauth_config: Option<Arc<OAuthConfig>>,
    /// Maps pre-session ID → OAuth state string for in-flight login flows.
    pub oauth_pending_state: Arc<Mutex<HashMap<String, String>>>,
    /// When set, word lists live on this Wikipedia page (title) instead of local files.
    pub wordlist_page: Option<Arc<String>>,
    /// Serializes wiki word-list exports so concurrent exports don't collide.
    pub export_lock: Arc<Mutex<()>>,
}

pub fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                if k.trim() == "orthonaut_session" {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
        })
}

/// Returns the caller's session id, minting a new one (plus a `Set-Cookie` value to emit
/// on the response) when the visitor has none yet. The cookie's lifetime is aligned with
/// the analysis retention window so identity and data expire on the same schedule.
fn resolve_or_create_session(headers: &HeaderMap) -> (String, Option<axum::http::HeaderValue>) {
    match extract_session_id(headers) {
        Some(id) => (id, None),
        None => {
            let id = oauth::generate_random_token();
            let max_age = db::RETENTION_DAYS * 24 * 60 * 60;
            let cookie = oauth::set_cookie_header("orthonaut_session", &id, Some(max_age));
            (id, Some(cookie))
        }
    }
}

/// Builds a check response, attaching a `Set-Cookie` header when a session was just minted.
fn check_response(body: CheckResponse, set_cookie: Option<axum::http::HeaderValue>) -> Response {
    let mut response = Json(body).into_response();
    if let Some(cookie) = set_cookie {
        response
            .headers_mut()
            .insert(axum::http::header::SET_COOKIE, cookie);
    }
    response
}

const SEARCH_MAX_RESULTS: usize = 10;

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponseItem {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchContextsRequest {
    pub url: String,
    pub word: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplySearchEditRequest {
    pub url: String,
    pub word: String,
    pub replacement: String,
    pub occurrence_index: Option<usize>,
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
pub struct AlwaysWrongWordsResponse {
    pub words: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportAlwaysWrongWordsResponse {
    pub exported_count: usize,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct AddAlwaysWrongWordRequest {
    pub word: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckRandomRequest {
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IgnoredWordsResponse {
    pub words: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportIgnoredWordsResponse {
    pub exported_count: usize,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct ReloadWordlistsResponse {
    pub valid: usize,
    pub wrong: usize,
}

#[derive(Debug, Serialize)]
pub struct SandboxCheckResponse {
    pub status: String,
    pub wrong_words: Vec<String>,
    pub total_words: usize,
    pub misspelled_count: usize,
}

#[derive(Debug, Serialize)]
pub struct WordContextsResponse {
    pub paragraphs: Vec<String>,
    pub total: usize,
    pub wikitext_paragraphs: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApplyEditRequest {
    pub article_id: i64,
    pub word: String,
    pub replacement: String,
    pub occurrence_index: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ApplyEditResponse {
    pub ok: bool,
    pub new_revision: u64,
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
    headers: HeaderMap,
    Json(payload): Json<CheckRequest>,
) -> Result<Response, ApiError> {
    if payload.url.trim().is_empty() {
        return Err(ApiError::BadRequest("url is required".to_string()));
    }

    let (session_id, set_cookie) = resolve_or_create_session(&headers);
    let body = run_check_for_url(&state, &session_id, payload.url.trim()).await?;
    Ok(check_response(body, set_cookie))
}

pub async fn check_random_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CheckRandomRequest>,
) -> Result<Response, ApiError> {
    let language = payload.language.as_deref().unwrap_or("es");
    if language != "es" {
        return Err(ApiError::BadRequest("only 'es' language is currently supported".to_string()));
    }

    let random_url = fetch_random_wikipedia_url(
        &state.http_client,
        language,
        state.wikimedia_contact.as_str(),
    )
    .await?;
    let (session_id, set_cookie) = resolve_or_create_session(&headers);
    let body = run_check_for_url(&state, &session_id, &random_url).await?;
    Ok(check_response(body, set_cookie))
}

pub async fn list_results(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ArticleResult>>, ApiError> {
    // Lazy cleanup: drop analyses past the retention window on every list load. This is
    // the only cleanup trigger — no background scheduler.
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(db::RETENTION_DAYS)).to_rfc3339();
    db::delete_expired_articles(state.db_path.as_str(), &cutoff)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let Some(session_id) = extract_session_id(&headers) else {
        return Ok(Json(vec![]));
    };
    let records = db::list_articles_for_session(state.db_path.as_str(), &session_id)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
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

pub async fn export_ignored_words(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ExportIgnoredWordsResponse>, ApiError> {
    if let Some(page) = state.wordlist_page.clone() {
        return export_ignored_words_to_wiki(&state, page.as_str(), &headers).await;
    }

    let db_words = db::list_ignored_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut merged: BTreeSet<String> = load_existing_suppression_words(state.suppressions_path.as_str())?;
    merged.extend(db_words);
    let exported_words: Vec<String> = merged.into_iter().collect();

    let mut file_content = String::from("# Words suppressed by Orthonaut.\n");
    file_content.push_str("# Exported from DB + file merge.\n\n");
    if !exported_words.is_empty() {
        file_content.push_str(&exported_words.join("\n"));
        file_content.push('\n');
    }
    fs::write(state.suppressions_path.as_str(), file_content)
        .map_err(|e| ApiError::Internal(format!("failed to write suppressions file: {e}")))?;

    let mut checker = state.checker.lock().await;
    checker.replace_ignored_words(exported_words.clone());
    drop(checker);

    Ok(Json(ExportIgnoredWordsResponse {
        exported_count: exported_words.len(),
        path: state.suppressions_path.as_ref().to_string(),
    }))
}

/// Wikipedia-mode export: merge the in-app valid words into the configured wiki page's
/// VALIDAS block and publish the edit. Requires the operator to be logged in.
async fn export_ignored_words_to_wiki(
    state: &AppState,
    page: &str,
    headers: &HeaderMap,
) -> Result<Json<ExportIgnoredWordsResponse>, ApiError> {
    let session_id = extract_session_id(headers);
    let access_token = resolve_access_token(state, session_id.as_deref()).await?;

    // Serialize exports and fetch the latest revision *inside* the lock, so two concurrent
    // exports merge on top of each other instead of colliding on a stale base revision.
    let _guard = state.export_lock.lock().await;

    let (wikitext, latest_id) = fetch_wikitext(
        &state.http_client,
        page,
        Some(&access_token),
        state.wikimedia_contact.as_str(),
    )
    .await
    .map_err(ApiError::Internal)?;

    // Re-read merge: union the page's current valid words with the in-app (DB) words so any
    // word added manually on-wiki since startup is preserved.
    let db_words = db::list_ignored_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut merged: BTreeSet<String> = crate::wordlists::parse_block(
        &wikitext,
        crate::wordlists::VALIDAS_START,
        crate::wordlists::VALIDAS_END,
    )
    .into_iter()
    .collect();
    merged.extend(db_words);
    let exported_words: Vec<String> = merged.into_iter().collect();

    let new_wikitext = crate::wordlists::replace_block(
        &wikitext,
        crate::wordlists::VALIDAS_START,
        crate::wordlists::VALIDAS_END,
        &exported_words,
    );

    if new_wikitext != wikitext {
        submit_wiki_edit(
            &state.http_client,
            page,
            &new_wikitext,
            "Orthonaut: actualizando la lista de palabras válidas (hecho con [[Usuario:Jmlarraz/Orthonaut|Orthonaut]])",
            latest_id,
            &access_token,
            state.wikimedia_contact.as_str(),
        )
        .await
        .map_err(ApiError::Internal)?;
    }

    let mut checker = state.checker.lock().await;
    checker.replace_ignored_words(exported_words.clone());
    drop(checker);

    Ok(Json(ExportIgnoredWordsResponse {
        exported_count: exported_words.len(),
        path: page.to_string(),
    }))
}

pub async fn ignore_word_in_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, word)): Path<(i64, String)>,
) -> Result<StatusCode, ApiError> {
    if word.trim().is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }
    let session_id = extract_session_id(&headers)
        .ok_or_else(|| ApiError::NotFound("result not found".to_string()))?;
    db::remove_word_from_article(state.db_path.as_str(), &session_id, id, &word)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let session_id = extract_session_id(&headers)
        .ok_or_else(|| ApiError::NotFound("result not found".to_string()))?;
    let deleted = db::delete_article(state.db_path.as_str(), &session_id, id)
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

pub async fn list_always_wrong_words(
    State(state): State<AppState>,
) -> Result<Json<AlwaysWrongWordsResponse>, ApiError> {
    let words = db::list_always_wrong_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(AlwaysWrongWordsResponse { words }))
}

pub async fn add_always_wrong_word(
    State(state): State<AppState>,
    Json(payload): Json<AddAlwaysWrongWordRequest>,
) -> Result<StatusCode, ApiError> {
    let normalized = crate::checker::normalize_ignored_word(&payload.word);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    db::insert_always_wrong_word(state.db_path.as_str(), &normalized)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut checker = state.checker.lock().await;
    checker.add_always_wrong_word(&normalized);
    drop(checker);
    Ok(StatusCode::CREATED)
}

pub async fn delete_always_wrong_word(
    State(state): State<AppState>,
    Path(word): Path<String>,
) -> Result<StatusCode, ApiError> {
    let normalized = crate::checker::normalize_ignored_word(&word);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    db::delete_always_wrong_word(state.db_path.as_str(), &normalized)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut checker = state.checker.lock().await;
    checker.remove_always_wrong_word(&normalized);
    drop(checker);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn export_always_wrong_words(
    State(state): State<AppState>,
) -> Result<Json<ExportAlwaysWrongWordsResponse>, ApiError> {
    if state.wordlist_page.is_some() {
        return Err(ApiError::BadRequest(
            "always-wrong words are managed on the Wikipedia page in this deployment".to_string(),
        ));
    }

    let db_words = db::list_always_wrong_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut merged: BTreeSet<String> = load_existing_suppression_words(state.always_wrong_path.as_str())?;
    merged.extend(db_words);
    let exported_words: Vec<String> = merged.into_iter().collect();

    let mut file_content = String::from("# Words always flagged as errors by Orthonaut.\n");
    file_content.push_str("# Exported from DB + file merge.\n\n");
    if !exported_words.is_empty() {
        file_content.push_str(&exported_words.join("\n"));
        file_content.push('\n');
    }
    fs::write(state.always_wrong_path.as_str(), file_content)
        .map_err(|e| ApiError::Internal(format!("failed to write always wrong words file: {e}")))?;

    let mut checker = state.checker.lock().await;
    checker.replace_always_wrong_words(exported_words.clone());
    drop(checker);

    Ok(Json(ExportAlwaysWrongWordsResponse {
        exported_count: exported_words.len(),
        path: state.always_wrong_path.as_ref().to_string(),
    }))
}

/// Wikipedia-mode reload: re-read both word lists from the configured wiki page and refresh the
/// in-memory checker, so words added directly on-wiki take effect without restarting the process.
/// Mirrors the startup load in `main.rs` (anonymous read; valid words are unioned with the
/// not-yet-exported in-app DB words so they are not lost).
pub async fn reload_wordlists(
    State(state): State<AppState>,
) -> Result<Json<ReloadWordlistsResponse>, ApiError> {
    let Some(page) = state.wordlist_page.clone() else {
        return Err(ApiError::BadRequest(
            "word lists are only reloadable in Wikipedia mode".to_string(),
        ));
    };

    let (validas, incorrectas) = fetch_wordlists(
        &state.http_client,
        page.as_str(),
        state.wikimedia_contact.as_str(),
    )
    .await
    .map_err(ApiError::Internal)?;

    // Union the freshly-fetched valid words with the in-app DB words (runtime-added words not yet
    // exported to the wiki) so the reload preserves them, exactly like startup.
    let db_words = db::list_ignored_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut valid: BTreeSet<String> = validas.into_iter().collect();
    valid.extend(db_words);
    let valid_words: Vec<String> = valid.into_iter().collect();

    let mut checker = state.checker.lock().await;
    checker.replace_ignored_words(valid_words.clone());
    checker.replace_always_wrong_words(incorrectas.clone());
    drop(checker);

    tracing::info!(
        page = %page,
        valid = valid_words.len(),
        wrong = incorrectas.len(),
        "reloaded word lists from Wikipedia"
    );

    Ok(Json(ReloadWordlistsResponse {
        valid: valid_words.len(),
        wrong: incorrectas.len(),
    }))
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

pub async fn get_word_contexts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, word)): Path<(i64, String)>,
) -> Result<Json<WordContextsResponse>, ApiError> {
    if word.trim().is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    let session_id = extract_session_id(&headers)
        .ok_or_else(|| ApiError::NotFound("article not found".to_string()))?;
    let article = db::get_article(state.db_path.as_str(), &session_id, id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("article not found".to_string()))?;

    let (fetch_url, _) = normalize_input_url(&article.page_url)?;
    let contact = state.wikimedia_contact.as_str();
    let page = wikipedia::fetch_page(&state.http_client, &fetch_url, contact)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let paragraphs = extractor::extract_paragraphs_for_word(&page.html, &word);
    let total = paragraphs.len();

    let wikitext_paragraphs = if let Some(title) = extract_title_from_wiki_url(&article.page_url) {
        fetch_wikitext(&state.http_client, &title, None, contact)
            .await
            .map(|(wt, _)| extractor::extract_wikitext_paragraphs_for_word(&wt, &word))
            .unwrap_or_default()
    } else {
        vec![]
    };

    Ok(Json(WordContextsResponse { paragraphs, total, wikitext_paragraphs }))
}

pub async fn apply_edit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ApplyEditRequest>,
) -> Result<Json<ApplyEditResponse>, ApiError> {
    if payload.word.trim().is_empty() || payload.replacement.trim().is_empty() {
        return Err(ApiError::BadRequest("word and replacement are required".to_string()));
    }

    let session_id = extract_session_id(&headers);
    let article = db::get_article(
        state.db_path.as_str(),
        session_id.as_deref().unwrap_or(""),
        payload.article_id,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?
    .ok_or_else(|| ApiError::NotFound("article not found".to_string()))?;

    let title = extract_title_from_wiki_url(&article.page_url)
        .ok_or_else(|| ApiError::BadRequest("cannot determine page title from URL".to_string()))?;

    let access_token = resolve_access_token(&state, session_id.as_deref()).await?;
    let (new_revision, username) = perform_wiki_edit(
        &state.http_client,
        &title,
        &payload.word,
        &payload.replacement,
        payload.occurrence_index,
        &access_token,
        state.wikimedia_contact.as_str(),
    )
    .await?;

    record_edit(&state, username.as_deref());

    Ok(Json(ApplyEditResponse { ok: true, new_revision }))
}

// Wikipedia search API deserialization structs
#[derive(serde::Deserialize)]
struct WikiSearchResponse {
    query: WikiSearchQuery,
}

#[derive(serde::Deserialize)]
struct WikiSearchQuery {
    search: Vec<WikiSearchItem>,
}

#[derive(serde::Deserialize)]
struct WikiSearchItem {
    title: String,
}

pub async fn search_handler(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<Vec<SearchResponseItem>>, ApiError> {
    let query = payload.query.trim().to_string();
    if query.is_empty() {
        return Err(ApiError::BadRequest("query is required".to_string()));
    }

    let search_term = format!("\"{}\"", query);
    let limit = payload.limit.unwrap_or(SEARCH_MAX_RESULTS as u32).min(200);
    let limit_str = limit.to_string();
    let offset_str = payload.offset.unwrap_or(0).to_string();
    let response = wikipedia::wikimedia_send(
        state
            .http_client
            .get("https://es.wikipedia.org/w/api.php")
            .query(&[
                ("action", "query"),
                ("list", "search"),
                ("srsearch", search_term.as_str()),
                ("srlimit", limit_str.as_str()),
                ("sroffset", offset_str.as_str()),
                ("srnamespace", "0"),
                ("format", "json"),
            ])
            .header(reqwest::header::ACCEPT, "application/json"),
        state.wikimedia_contact.as_str(),
    )
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!("Wikipedia search returned {status}: {body}")));
    }

    let payload: WikiSearchResponse = response.json().await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let items: Vec<SearchResponseItem> = payload
        .query
        .search
        .into_iter()
        .map(|item| {
            let encoded = item.title.replace(' ', "_");
            SearchResponseItem {
                url: format!("https://es.wikipedia.org/wiki/{encoded}"),
                title: item.title,
            }
        })
        .collect();

    // Post-filter: keep only articles that actually contain the exact word (accent-sensitive)
    let filter_futures: Vec<_> = items.iter().map(|item| {
        let http_client = state.http_client.clone();
        let contact = state.wikimedia_contact.clone();
        let word = query.clone();
        let fetch_url = normalize_input_url(&item.url)
            .map(|(fetch, _)| fetch)
            .unwrap_or_else(|_| item.url.clone());
        async move {
            match wikipedia::fetch_page(&http_client, &fetch_url, &contact).await {
                Ok(page) => extractor::article_contains_word(&page.html, &word),
                Err(_) => true,
            }
        }
    }).collect();
    let keep_flags = futures::future::join_all(filter_futures).await;
    let items: Vec<_> = items
        .into_iter()
        .zip(keep_flags)
        .filter_map(|(item, keep)| if keep { Some(item) } else { None })
        .collect();

    Ok(Json(items))
}

pub async fn get_search_contexts(
    State(state): State<AppState>,
    Json(payload): Json<SearchContextsRequest>,
) -> Result<Json<WordContextsResponse>, ApiError> {
    if payload.url.trim().is_empty() || payload.word.trim().is_empty() {
        return Err(ApiError::BadRequest("url and word are required".to_string()));
    }

    let (fetch_url, display_url) = normalize_input_url(&payload.url)?;
    let contact = state.wikimedia_contact.as_str();

    let page = wikipedia::fetch_page(&state.http_client, &fetch_url, contact)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let paragraphs = extractor::extract_paragraphs_for_word(&page.html, &payload.word);
    let total = paragraphs.len();

    let wikitext_paragraphs = if let Some(title) = extract_title_from_wiki_url(&display_url) {
        fetch_wikitext(&state.http_client, &title, None, contact)
            .await
            .map(|(wt, _)| extractor::extract_wikitext_paragraphs_for_word(&wt, &payload.word))
            .unwrap_or_default()
    } else {
        vec![]
    };

    Ok(Json(WordContextsResponse { paragraphs, total, wikitext_paragraphs }))
}

pub async fn apply_search_edit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ApplySearchEditRequest>,
) -> Result<Json<ApplyEditResponse>, ApiError> {
    if payload.word.trim().is_empty() || payload.replacement.trim().is_empty() {
        return Err(ApiError::BadRequest("word and replacement are required".to_string()));
    }

    let (_, display_url) = normalize_input_url(&payload.url)?;
    let title = extract_title_from_wiki_url(&display_url)
        .ok_or_else(|| ApiError::BadRequest("cannot determine page title from URL".to_string()))?;

    let session_id = extract_session_id(&headers);
    let access_token = resolve_access_token(&state, session_id.as_deref()).await?;
    let (new_revision, username) = perform_wiki_edit(
        &state.http_client,
        &title,
        &payload.word,
        &payload.replacement,
        payload.occurrence_index,
        &access_token,
        state.wikimedia_contact.as_str(),
    )
    .await?;

    record_edit(&state, username.as_deref());

    Ok(Json(ApplyEditResponse { ok: true, new_revision }))
}

/// Bumps the per-user edit leaderboard after a successful edit. A failure here must
/// never fail the edit itself, so errors (and a missing username) are only logged.
fn record_edit(state: &AppState, username: Option<&str>) {
    let Some(username) = username else {
        tracing::warn!("edit succeeded but Wikipedia returned no username; not counted");
        return;
    };
    if let Err(e) = db::increment_edit_count(state.db_path.as_str(), username) {
        tracing::error!("failed to record edit count for {username}: {e}");
    }
}

/// Returns the edit leaderboard: every editor and their total, most edits first.
pub async fn list_stats(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::EditCount>>, ApiError> {
    db::list_edit_counts(state.db_path.as_str())
        .map(Json)
        .map_err(|e| ApiError::Internal(e.to_string()))
}

async fn resolve_access_token(state: &AppState, session_id: Option<&str>) -> Result<String, ApiError> {
    let oauth_config = state
        .oauth_config
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("OAuth is not configured".to_string()))?;

    if let Some(ref static_token) = oauth_config.token {
        return Ok(static_token.clone());
    }

    let session_id = session_id
        .ok_or_else(|| ApiError::BadRequest("not logged in to Wikipedia".to_string()))?;

    let token = db::get_oauth_token(state.db_path.as_str(), session_id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::BadRequest("not logged in to Wikipedia".to_string()))?;

    if is_token_expired(&token.expires_at) {
        let refresh_token = token
            .refresh_token
            .as_deref()
            .ok_or_else(|| ApiError::BadRequest("session expired, please log in again".to_string()))?;
        let new_token = oauth::refresh_access_token(
            &state.http_client,
            refresh_token,
            &oauth_config.client_id,
            &oauth_config.client_secret,
        )
        .await
        .map_err(|e| ApiError::Internal(e))?;
        let expires_at = (chrono::Utc::now()
            + chrono::Duration::seconds(new_token.expires_in as i64))
        .to_rfc3339();
        db::store_oauth_token(
            state.db_path.as_str(),
            session_id,
            &new_token.access_token,
            new_token.refresh_token.as_deref(),
            &expires_at,
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok(new_token.access_token)
    } else {
        Ok(token.access_token)
    }
}

async fn perform_wiki_edit(
    client: &reqwest::Client,
    title: &str,
    word: &str,
    replacement: &str,
    occurrence_index: Option<usize>,
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<(u64, Option<String>), ApiError> {
    let (wikitext, latest_id) =
        fetch_wikitext(client, title, Some(access_token), wikimedia_contact)
            .await
            .map_err(ApiError::Internal)?;

    let new_wikitext = replace_word_occurrences(&wikitext, word, replacement, occurrence_index);
    if new_wikitext == wikitext {
        return Err(ApiError::BadRequest(format!(
            "word '{}' not found in page wikitext",
            word
        )));
    }

    let trimmed = replacement.trim();
    let summary = if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
        format!(
            "Añadir enlace con [[Usuario:Jmlarraz/Orthonaut|Orthonaut]] a «{}»",
            word
        )
    } else {
        format!(
            "Corrección ortográfica con [[Usuario:Jmlarraz/Orthonaut|Orthonaut]]: «{}» → «{}»",
            word, replacement
        )
    };

    submit_wiki_edit(
        client,
        title,
        &new_wikitext,
        &summary,
        latest_id,
        access_token,
        wikimedia_contact,
    )
    .await
    .map_err(ApiError::Internal)
}

fn is_token_expired(expires_at: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .map(|exp| exp <= chrono::Utc::now())
        .unwrap_or(true)
}

fn extract_title_from_wiki_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let path = parsed.path();
    path.strip_prefix("/wiki/").map(percent_decode)
}

fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// Action API structs for fetching wikitext
#[derive(serde::Deserialize)]
struct ActionQueryResponse {
    query: ActionQuery,
}

#[derive(serde::Deserialize)]
struct ActionQuery {
    pages: std::collections::HashMap<String, ActionPage>,
}

#[derive(serde::Deserialize)]
struct ActionPage {
    revisions: Option<Vec<ActionRevision>>,
}

#[derive(serde::Deserialize)]
struct ActionRevision {
    revid: u64,
    slots: ActionSlots,
}

#[derive(serde::Deserialize)]
struct ActionSlots {
    main: ActionSlotMain,
}

#[derive(serde::Deserialize)]
struct ActionSlotMain {
    #[serde(rename = "*")]
    content: String,
}

// Action API structs for CSRF token
#[derive(serde::Deserialize)]
struct ActionTokenResponse {
    query: ActionTokenQuery,
}

#[derive(serde::Deserialize)]
struct ActionTokenQuery {
    tokens: ActionTokens,
    userinfo: Option<ActionUserInfo>,
}

#[derive(serde::Deserialize)]
struct ActionTokens {
    csrftoken: String,
}

#[derive(serde::Deserialize)]
struct ActionUserInfo {
    name: String,
}

// Action API structs for edit response
#[derive(serde::Deserialize)]
struct ActionEditResponse {
    edit: ActionEditResult,
}

#[derive(serde::Deserialize)]
struct ActionEditResult {
    newrevid: u64,
}

async fn fetch_wikitext(
    client: &reqwest::Client,
    title: &str,
    access_token: Option<&str>,
    wikimedia_contact: &str,
) -> Result<(String, u64), String> {
    let mut req = client
        .get("https://es.wikipedia.org/w/api.php")
        .query(&[
            ("action", "query"),
            ("prop", "revisions"),
            ("rvprop", "ids|content"),
            ("rvslots", "main"),
            ("titles", title),
            ("format", "json"),
        ])
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(token) = access_token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let response = wikipedia::wikimedia_send(req, wikimedia_contact)
    .await
    .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("wikitext fetch returned {status}: {body}"));
    }

    let payload: ActionQueryResponse = response.json().await.map_err(|e| e.to_string())?;
    let page = payload
        .query
        .pages
        .into_values()
        .next()
        .ok_or_else(|| "no page in wikitext response".to_string())?;
    let rev = page
        .revisions
        .and_then(|mut r| { r.reverse(); r.pop() })
        .ok_or_else(|| "no revisions in wikitext response".to_string())?;
    Ok((rev.slots.main.content, rev.revid))
}

/// Fetch the configured word-list page and parse both sentinel blocks.
/// Anonymous read (no token) — used at startup and before exporting.
pub async fn fetch_wordlists(
    client: &reqwest::Client,
    title: &str,
    wikimedia_contact: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let (wikitext, _) = fetch_wikitext(client, title, None, wikimedia_contact).await?;
    let validas = crate::wordlists::parse_block(
        &wikitext,
        crate::wordlists::VALIDAS_START,
        crate::wordlists::VALIDAS_END,
    );
    let incorrectas = crate::wordlists::parse_block(
        &wikitext,
        crate::wordlists::INCORRECTAS_START,
        crate::wordlists::INCORRECTAS_END,
    );
    Ok((validas, incorrectas))
}

/// Fetches the CSRF edit token and, in the same request, the logged-in user's name
/// (`meta=tokens|userinfo`) so we can attribute the edit without an extra round trip.
/// Returns `(csrf_token, username)`; username is `None` if the API omits it.
async fn fetch_csrf_token(
    client: &reqwest::Client,
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<(String, Option<String>), String> {
    let response = wikipedia::wikimedia_send(
        client
            .get("https://es.wikipedia.org/w/api.php")
            .query(&[
                ("action", "query"),
                ("meta", "tokens|userinfo"),
                ("type", "csrf"),
                ("format", "json"),
            ])
            .header("Authorization", format!("Bearer {access_token}")),
        wikimedia_contact,
    )
    .await
    .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CSRF token fetch returned {status}: {body}"));
    }

    let payload: ActionTokenResponse = response.json().await.map_err(|e| e.to_string())?;
    let username = payload.query.userinfo.map(|u| u.name);
    Ok((payload.query.tokens.csrftoken, username))
}

async fn submit_wiki_edit(
    client: &reqwest::Client,
    title: &str,
    source: &str,
    comment: &str,
    latest_id: u64,
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<(u64, Option<String>), String> {
    let (csrf_token, username) = fetch_csrf_token(client, access_token, wikimedia_contact).await?;
    let latest_id_str = latest_id.to_string();
    let params = [
        ("action", "edit"),
        ("format", "json"),
        ("title", title),
        ("text", source),
        ("summary", comment),
        ("baserevid", latest_id_str.as_str()),
        ("token", csrf_token.as_str()),
        ("minor", "true"),
    ];
    let response = wikipedia::wikimedia_send(
        client
            .post("https://es.wikipedia.org/w/api.php")
            .header("Authorization", format!("Bearer {access_token}"))
            .form(&params),
        wikimedia_contact,
    )
    .await
    .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("edit submit returned {status}: {body}"));
    }

    let edit_resp: ActionEditResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok((edit_resp.edit.newrevid, username))
}

fn replace_word_occurrences(text: &str, word: &str, replacement: &str, occurrence_index: Option<usize>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word_chars: Vec<char> = word.to_lowercase().chars().collect();
    let word_len = word_chars.len();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let mut match_count = 0usize;

    while i < chars.len() {
        if i + word_len <= chars.len() {
            let slice_lower: Vec<char> = chars[i..i + word_len]
                .iter()
                .flat_map(|c| c.to_lowercase())
                .collect();
            if slice_lower == word_chars {
                let prev_is_word = word_extends_left(&chars, i);
                let next_is_word = word_extends_right(&chars, i + word_len);
                if !prev_is_word && !next_is_word {
                    let should_replace = occurrence_index.map_or(true, |idx| idx == match_count);
                    match_count += 1;
                    if should_replace {
                        result.push_str(replacement);
                        i += word_len;
                        continue;
                    }
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Whether the character immediately before the match (at `start - 1`) makes the
/// matched run part of a larger word. Letters and hyphens always do. An
/// apostrophe only does when it is genuinely intra-word (preceded by a letter,
/// e.g. `l'paraiso`); a leading apostrophe run is wiki markup (`''`/`'''`) and
/// acts as a boundary.
fn word_extends_left(chars: &[char], start: usize) -> bool {
    if start == 0 {
        return false;
    }
    let c = chars[start - 1];
    if c == '\'' {
        return start >= 2 && chars[start - 2].is_alphabetic();
    }
    c.is_alphabetic() || c == '-'
}

/// Mirror of [`word_extends_left`] for the character immediately after the match
/// (at `end`). An apostrophe only extends the word when followed by a letter
/// (e.g. `paraiso'word`); a trailing `''`/`'''` markup run acts as a boundary.
fn word_extends_right(chars: &[char], end: usize) -> bool {
    if end >= chars.len() {
        return false;
    }
    let c = chars[end];
    if c == '\'' {
        return end + 1 < chars.len() && chars[end + 1].is_alphabetic();
    }
    c.is_alphabetic() || c == '-'
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
        let encoded = title.replace('/', "%2F");
        return Ok((
            build_url(&parsed, &format!("/api/rest_v1/page/html/{encoded}")),
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

async fn run_check_for_url(
    state: &AppState,
    session_id: &str,
    input_url: &str,
) -> Result<CheckResponse, ApiError> {
    let (fetch_url, display_url) = normalize_input_url(input_url)?;

    let contact = state.wikimedia_contact.as_str();
    let page = match wikipedia::fetch_page(&state.http_client, &fetch_url, contact).await {
        Ok(page) => page,
        Err(wikipedia::WikipediaError::UpstreamStatus(code))
            if code == reqwest::StatusCode::FORBIDDEN && fetch_url != display_url =>
        {
            // Some upstream edge nodes may reject REST HTML for specific requests.
            // Retry with the canonical article URL so `/wiki/...` inputs keep working.
            wikipedia::fetch_page(&state.http_client, &display_url, contact)
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
        return Ok(reporter::ok_message(page.title));
    }

    let id = db::insert_article(
        state.db_path.as_str(),
        session_id,
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

    Ok(reporter::errors_found(result))
}

#[derive(Debug, Deserialize)]
struct RandomApiResponse {
    query: RandomQuery,
}

#[derive(Debug, Deserialize)]
struct RandomQuery {
    random: Vec<RandomPage>,
}

#[derive(Debug, Deserialize)]
struct RandomPage {
    title: String,
}

async fn fetch_random_wikipedia_url(
    client: &reqwest::Client,
    language: &str,
    wikimedia_contact: &str,
) -> Result<String, ApiError> {
    let api_url = format!(
        "https://{language}.wikipedia.org/w/api.php?action=query&format=json&list=random&rnnamespace=0&rnlimit=1"
    );
    let response = wikipedia::wikimedia_send(
        client
            .get(api_url)
            .header(reqwest::header::ACCEPT, "application/json"),
        wikimedia_contact,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("failed to fetch random page: {e}")))?;

    if !response.status().is_success() {
        return Err(ApiError::BadRequest(format!(
            "random page upstream returned status {}",
            response.status()
        )));
    }

    let payload: RandomApiResponse = response
        .json()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid random page response: {e}")))?;
    let page = payload
        .query
        .random
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::BadRequest("random page response was empty".to_string()))?;
    let encoded = page.title.replace(' ', "_");
    Ok(format!("https://{language}.wikipedia.org/wiki/{encoded}"))
}

fn load_existing_suppression_words(path: &str) -> Result<BTreeSet<String>, ApiError> {
    if !std::path::Path::new(path).exists() {
        return Ok(BTreeSet::new());
    }
    let content = fs::read_to_string(path)
        .map_err(|e| ApiError::Internal(format!("failed to read suppressions file: {e}")))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(crate::checker::normalize_ignored_word)
        .collect())
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
        replace_word_occurrences, AddIgnoredWordRequest, AppState,
    };

    #[test]
    fn replaces_word_wrapped_in_italic_markup() {
        // The flagged word is clean (`paraiso`); the `''` italic markup must be
        // preserved, so the user only ever types the replacement word.
        let out = replace_word_occurrences("''El final del paraiso''", "paraiso", "paraíso", None);
        assert_eq!(out, "''El final del paraíso''");
    }

    #[test]
    fn replaces_word_wrapped_in_bold_markup() {
        let out = replace_word_occurrences("'''paraiso'''", "paraiso", "paraíso", None);
        assert_eq!(out, "'''paraíso'''");

        // Bold + italic (five apostrophes) is preserved too.
        let out = replace_word_occurrences("'''''paraiso'''''", "paraiso", "paraíso", None);
        assert_eq!(out, "'''''paraíso'''''");
    }

    #[test]
    fn replaces_plain_word_between_spaces() {
        let out = replace_word_occurrences("el paraiso es bonito", "paraiso", "paraíso", None);
        assert_eq!(out, "el paraíso es bonito");
    }

    #[test]
    fn does_not_replace_inside_word_with_genuine_apostrophe() {
        // A single apostrophe flanked by letters is intra-word: `paraiso` here is a
        // substring of a longer token and must not be touched.
        let out = replace_word_occurrences("l'paraiso", "paraiso", "paraíso", None);
        assert_eq!(out, "l'paraiso");
        let out = replace_word_occurrences("paraiso'word", "paraiso", "paraíso", None);
        assert_eq!(out, "paraiso'word");
    }

    #[test]
    fn occurrence_index_targets_the_right_match_with_markup() {
        let text = "paraiso y ''paraiso''";
        assert_eq!(
            replace_word_occurrences(text, "paraiso", "paraíso", Some(0)),
            "paraíso y ''paraiso''"
        );
        assert_eq!(
            replace_word_occurrences(text, "paraiso", "paraíso", Some(1)),
            "paraiso y ''paraíso''"
        );
    }

    #[test]
    fn converts_wiki_page_url_to_rest_html_url() {
        let (fetch, display) =
            normalize_input_url("https://es.wikipedia.org/wiki/Madrid").expect("valid url");
        assert_eq!(fetch, "https://es.wikipedia.org/api/rest_v1/page/html/Madrid");
        assert_eq!(display, "https://es.wikipedia.org/wiki/Madrid");
    }

    #[test]
    fn encodes_slash_in_title_for_rest_url() {
        let (fetch, display) =
            normalize_input_url("https://es.wikipedia.org/wiki/Wikipedia:Zona_de_pruebas/4")
                .expect("valid url");
        assert_eq!(
            fetch,
            "https://es.wikipedia.org/api/rest_v1/page/html/Wikipedia:Zona_de_pruebas%2F4"
        );
        assert_eq!(
            display,
            "https://es.wikipedia.org/wiki/Wikipedia:Zona_de_pruebas/4"
        );
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
            .join(format!("orthonaut-api-test-{nanos}.db"))
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
            suppressions_path: Arc::new("dictionaries/suppressions.txt".to_string()),
            always_wrong_path: Arc::new("dictionaries/always_wrong.txt".to_string()),
            http_client: reqwest::Client::new(),
            checker: Arc::new(Mutex::new(checker)),
            wikimedia_contact: Arc::new("wikipedia:es; User:Test".to_string()),
            oauth_config: None,
            oauth_pending_state: Arc::new(Mutex::new(std::collections::HashMap::new())),
            wordlist_page: None,
            export_lock: Arc::new(Mutex::new(())),
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
                is_link: false,
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
                is_link: false,
            }]);
            assert!(after_delete.contains(&"palabrafalsa".to_string()));
        }
    }
}
