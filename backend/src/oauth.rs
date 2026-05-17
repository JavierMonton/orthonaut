use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Json,
};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{api::AppState, db};

const AUTHORIZE_URL: &str = "https://es.wikipedia.org/w/rest.php/oauth2/authorize";
const TOKEN_URL: &str = "https://es.wikipedia.org/w/rest.php/oauth2/access_token";

fn generate_state() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub logged_in: bool,
    pub expires_at: Option<String>,
    pub oauth_configured: bool,
}

pub async fn auth_login(State(state): State<AppState>) -> impl IntoResponse {
    let Some(ref oauth_config) = state.oauth_config else {
        return (StatusCode::BAD_REQUEST, "OAuth is not configured in wordfixer.toml").into_response();
    };

    if oauth_config.token.is_some() {
        return Redirect::to("http://localhost:5173/?auth=success").into_response();
    }

    let random_state = generate_state();
    {
        let mut pending = state.oauth_pending_state.lock().await;
        *pending = Some(random_state.clone());
    }

    let mut auth_url =
        reqwest::Url::parse(AUTHORIZE_URL).expect("valid OAuth authorize URL");
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oauth_config.client_id)
        .append_pair("redirect_uri", &oauth_config.redirect_uri)
        .append_pair("scope", "basic editpage")
        .append_pair("state", &random_state);

    Redirect::to(auth_url.as_str()).into_response()
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn auth_callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    if params.error.is_some() {
        return Redirect::to("http://localhost:5173/?auth=error");
    }

    let Some(code) = params.code else {
        return Redirect::to("http://localhost:5173/?auth=error");
    };
    let Some(param_state) = params.state else {
        return Redirect::to("http://localhost:5173/?auth=error");
    };

    {
        let mut pending = state.oauth_pending_state.lock().await;
        match pending.take() {
            Some(ref expected) if *expected == param_state => {}
            _ => return Redirect::to("http://localhost:5173/?auth=error"),
        }
    }

    let Some(ref oauth_config) = state.oauth_config else {
        return Redirect::to("http://localhost:5173/?auth=error");
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
                &token_data.access_token,
                token_data.refresh_token.as_deref(),
                &expires_at,
            ) {
                tracing::error!("Failed to store OAuth token: {e}");
                return Redirect::to("http://localhost:5173/?auth=error");
            }

            Redirect::to("http://localhost:5173/?auth=success")
        }
        Err(e) => {
            tracing::error!("Token exchange failed: {e}");
            Redirect::to("http://localhost:5173/?auth=error")
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

pub async fn auth_status(State(state): State<AppState>) -> Json<AuthStatusResponse> {
    let oauth_configured = state.oauth_config.is_some();

    if state.oauth_config.as_ref().and_then(|c| c.token.as_ref()).is_some() {
        return Json(AuthStatusResponse {
            logged_in: true,
            expires_at: None,
            oauth_configured,
        });
    }

    match db::get_oauth_token(state.db_path.as_str()) {
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

pub async fn auth_logout(State(state): State<AppState>) -> StatusCode {
    let _ = db::delete_oauth_token(state.db_path.as_str());
    StatusCode::NO_CONTENT
}
