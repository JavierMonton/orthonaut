use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{api::{extract_session_id, AppState}, db};

const AUTHORIZE_URL: &str = "https://es.wikipedia.org/w/rest.php/oauth2/authorize";
const TOKEN_URL: &str = "https://es.wikipedia.org/w/rest.php/oauth2/access_token";

fn generate_random_token() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn set_cookie_header(name: &str, value: &str, max_age: Option<i64>) -> HeaderValue {
    let max_age_part = match max_age {
        Some(s) => format!("; Max-Age={s}"),
        None => String::new(),
    };
    HeaderValue::from_str(&format!(
        "{name}={value}; HttpOnly; Path=/; SameSite=Lax{max_age_part}"
    ))
    .expect("cookie value contains no invalid characters")
}

fn extract_presession_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                if k.trim() == "orthonaut_presession" {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
        })
}

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub logged_in: bool,
    pub expires_at: Option<String>,
    pub oauth_configured: bool,
}

pub async fn auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(ref oauth_config) = state.oauth_config else {
        return (StatusCode::BAD_REQUEST, "OAuth is not configured in orthonaut.toml").into_response();
    };

    if oauth_config.token.is_some() {
        return Redirect::to("http://localhost:5173/?auth=success").into_response();
    }

    // If the user is already logged in (has a valid session), just redirect to success.
    if let Some(session_id) = extract_session_id(&headers) {
        if db::get_oauth_token(state.db_path.as_str(), &session_id)
            .ok()
            .flatten()
            .is_some()
        {
            return Redirect::to("http://localhost:5173/?auth=success").into_response();
        }
    }

    let session_id = generate_random_token();
    let oauth_state = generate_random_token();

    {
        let mut pending = state.oauth_pending_state.lock().await;
        pending.insert(session_id.clone(), oauth_state.clone());
    }

    let mut auth_url =
        reqwest::Url::parse(AUTHORIZE_URL).expect("valid OAuth authorize URL");
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oauth_config.client_id)
        .append_pair("redirect_uri", &oauth_config.redirect_uri)
        .append_pair("scope", "basic editpage")
        .append_pair("state", &oauth_state);

    let mut response = Redirect::to(auth_url.as_str()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie_header("orthonaut_presession", &session_id, None),
    );
    response
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Response {
    if params.error.is_some() {
        return Redirect::to("http://localhost:5173/?auth=error").into_response();
    }

    let Some(code) = params.code else {
        return Redirect::to("http://localhost:5173/?auth=error").into_response();
    };
    let Some(param_state) = params.state else {
        return Redirect::to("http://localhost:5173/?auth=error").into_response();
    };

    let Some(session_id) = extract_presession_id(&headers) else {
        return Redirect::to("http://localhost:5173/?auth=error").into_response();
    };

    {
        let mut pending = state.oauth_pending_state.lock().await;
        match pending.remove(&session_id) {
            Some(ref expected) if *expected == param_state => {}
            _ => return Redirect::to("http://localhost:5173/?auth=error").into_response(),
        }
    }

    let Some(ref oauth_config) = state.oauth_config else {
        return Redirect::to("http://localhost:5173/?auth=error").into_response();
    };

    match exchange_code_for_token(
        &state.http_client,
        &code,
        &oauth_config.client_id,
        &oauth_config.client_secret,
        &oauth_config.redirect_uri,
    )
    .await
    {
        Ok(token_data) => {
            let expires_at = (chrono::Utc::now()
                + chrono::Duration::seconds(token_data.expires_in as i64))
            .to_rfc3339();

            if let Err(e) = db::store_oauth_token(
                state.db_path.as_str(),
                &session_id,
                &token_data.access_token,
                token_data.refresh_token.as_deref(),
                &expires_at,
            ) {
                tracing::error!("Failed to store OAuth token: {e}");
                return Redirect::to("http://localhost:5173/?auth=error").into_response();
            }

            let mut response = Redirect::to("http://localhost:5173/?auth=success").into_response();
            let hdrs = response.headers_mut();
            // Set the permanent session cookie.
            hdrs.insert(
                header::SET_COOKIE,
                set_cookie_header("orthonaut_session", &session_id, None),
            );
            // Clear the pre-session cookie.
            hdrs.append(
                header::SET_COOKIE,
                set_cookie_header("orthonaut_presession", "", Some(0)),
            );
            response
        }
        Err(e) => {
            tracing::error!("Token exchange failed: {e}");
            Redirect::to("http://localhost:5173/?auth=error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

pub async fn exchange_code_for_token(
    client: &reqwest::Client,
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Token endpoint returned {status}: {body}"));
    }

    response.json().await.map_err(|e| e.to_string())
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Refresh token endpoint returned {status}: {body}"));
    }

    response.json().await.map_err(|e| e.to_string())
}

pub async fn auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<AuthStatusResponse> {
    let oauth_configured = state.oauth_config.is_some();

    if state.oauth_config.as_ref().and_then(|c| c.token.as_ref()).is_some() {
        return Json(AuthStatusResponse {
            logged_in: true,
            expires_at: None,
            oauth_configured,
        });
    }

    let Some(session_id) = extract_session_id(&headers) else {
        return Json(AuthStatusResponse {
            logged_in: false,
            expires_at: None,
            oauth_configured,
        });
    };

    match db::get_oauth_token(state.db_path.as_str(), &session_id) {
        Ok(Some(token)) => Json(AuthStatusResponse {
            logged_in: true,
            expires_at: Some(token.expires_at),
            oauth_configured,
        }),
        _ => Json(AuthStatusResponse {
            logged_in: false,
            expires_at: None,
            oauth_configured,
        }),
    }
}

pub async fn auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(session_id) = extract_session_id(&headers) {
        let _ = db::delete_oauth_token(state.db_path.as_str(), &session_id);
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie_header("orthonaut_session", "", Some(0)),
    );
    response
}
