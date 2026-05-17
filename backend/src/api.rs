use std::{
    collections::BTreeSet,
    fs,
    sync::Arc,
};

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
    pub http_client: reqwest::Client,
    pub checker: Arc<Mutex<SpellChecker>>,
    pub wikimedia_contact: Arc<String>,
    pub oauth_config: Option<Arc<OAuthConfig>>,
    pub oauth_pending_state: Arc<Mutex<Option<String>>>,
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
    Json(payload): Json<CheckRequest>,
) -> Result<Json<CheckResponse>, ApiError> {
    if payload.url.trim().is_empty() {
        return Err(ApiError::BadRequest("url is required".to_string()));
    }

    run_check_for_url(&state, payload.url.trim()).await.map(Json)
}

pub async fn check_random_page(
    State(state): State<AppState>,
    Json(payload): Json<CheckRandomRequest>,
) -> Result<Json<CheckResponse>, ApiError> {
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
    run_check_for_url(&state, &random_url).await.map(Json)
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

pub async fn export_ignored_words(
    State(state): State<AppState>,
) -> Result<Json<ExportIgnoredWordsResponse>, ApiError> {
    let db_words = db::list_ignored_words(state.db_path.as_str())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut merged: BTreeSet<String> = load_existing_suppression_words(state.suppressions_path.as_str())?;
    merged.extend(db_words);
    let exported_words: Vec<String> = merged.into_iter().collect();

    let mut file_content = String::from("# Words suppressed by Wordfixer.\n");
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

pub async fn get_word_contexts(
    State(state): State<AppState>,
    Path((id, word)): Path<(i64, String)>,
) -> Result<Json<WordContextsResponse>, ApiError> {
    if word.trim().is_empty() {
        return Err(ApiError::BadRequest("word is required".to_string()));
    }

    let article = db::get_article(state.db_path.as_str(), id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("article not found".to_string()))?;

    let (fetch_url, _) = normalize_input_url(&article.page_url)?;
    let contact = state.wikimedia_contact.as_str();
    let page = wikipedia::fetch_page(&state.http_client, &fetch_url, contact)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let paragraphs = extractor::extract_paragraphs_for_word(&page.html, &word);
    let total = paragraphs.len();
    Ok(Json(WordContextsResponse { paragraphs, total }))
}

pub async fn apply_edit(
    State(state): State<AppState>,
    Json(payload): Json<ApplyEditRequest>,
) -> Result<Json<ApplyEditResponse>, ApiError> {
    if payload.word.trim().is_empty() || payload.replacement.trim().is_empty() {
        return Err(ApiError::BadRequest("word and replacement are required".to_string()));
    }

    let oauth_config = state
        .oauth_config
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("OAuth is not configured".to_string()))?;

    let access_token = if let Some(ref static_token) = oauth_config.token {
        static_token.clone()
    } else {
        let token = db::get_oauth_token(state.db_path.as_str())
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
                &new_token.access_token,
                new_token.refresh_token.as_deref(),
                &expires_at,
            )
            .map_err(|e| ApiError::Internal(e.to_string()))?;
            new_token.access_token
        } else {
            token.access_token
        }
    };

    let article = db::get_article(state.db_path.as_str(), payload.article_id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("article not found".to_string()))?;

    let title = extract_title_from_wiki_url(&article.page_url)
        .ok_or_else(|| ApiError::BadRequest("cannot determine page title from URL".to_string()))?;

    let (wikitext, latest_id) =
        fetch_wikitext(&state.http_client, &title, &access_token, state.wikimedia_contact.as_str())
            .await
            .map_err(ApiError::Internal)?;

    let new_wikitext = replace_word_occurrences(&wikitext, &payload.word, &payload.replacement, payload.occurrence_index);
    if new_wikitext == wikitext {
        return Err(ApiError::BadRequest(format!(
            "word '{}' not found in page wikitext",
            payload.word
        )));
    }

    let new_revision = submit_wiki_edit(
        &state.http_client,
        &title,
        &new_wikitext,
        &format!(
            "Corrección ortográfica: «{}» → «{}»",
            payload.word, payload.replacement
        ),
        latest_id,
        &access_token,
        state.wikimedia_contact.as_str(),
    )
    .await
    .map_err(ApiError::Internal)?;

    Ok(Json(ApplyEditResponse { ok: true, new_revision }))
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
}

#[derive(serde::Deserialize)]
struct ActionTokens {
    csrftoken: String,
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
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<(String, u64), String> {
    let response = wikipedia::wikimedia_send(
        client
            .get("https://es.wikipedia.org/w/api.php")
            .query(&[
                ("action", "query"),
                ("prop", "revisions"),
                ("rvprop", "ids|content"),
                ("rvslots", "main"),
                ("titles", title),
                ("format", "json"),
            ])
            .header("Authorization", format!("Bearer {access_token}"))
            .header(reqwest::header::ACCEPT, "application/json"),
        wikimedia_contact,
    )
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

async fn fetch_csrf_token(
    client: &reqwest::Client,
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<String, String> {
    let response = wikipedia::wikimedia_send(
        client
            .get("https://es.wikipedia.org/w/api.php")
            .query(&[
                ("action", "query"),
                ("meta", "tokens"),
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
    Ok(payload.query.tokens.csrftoken)
}

async fn submit_wiki_edit(
    client: &reqwest::Client,
    title: &str,
    source: &str,
    comment: &str,
    latest_id: u64,
    access_token: &str,
    wikimedia_contact: &str,
) -> Result<u64, String> {
    let csrf_token = fetch_csrf_token(client, access_token, wikimedia_contact).await?;
    let latest_id_str = latest_id.to_string();
    let params = [
        ("action", "edit"),
        ("format", "json"),
        ("title", title),
        ("text", source),
        ("summary", comment),
        ("baserevid", latest_id_str.as_str()),
        ("token", csrf_token.as_str()),
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
    Ok(edit_resp.edit.newrevid)
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
                let prev_is_word = i > 0 && is_word_char(chars[i - 1]);
                let next_is_word =
                    i + word_len < chars.len() && is_word_char(chars[i + word_len]);
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

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '-'
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

async fn run_check_for_url(state: &AppState, input_url: &str) -> Result<CheckResponse, ApiError> {
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
            .join(format!("wordfixer-api-test-{nanos}.db"))
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
            http_client: reqwest::Client::new(),
            checker: Arc::new(Mutex::new(checker)),
            wikimedia_contact: Arc::new("wikipedia:es; User:Test".to_string()),
            oauth_config: None,
            oauth_pending_state: Arc::new(Mutex::new(None)),
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
