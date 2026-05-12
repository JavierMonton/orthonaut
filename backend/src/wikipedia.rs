use reqwest::Client;
use scraper::{Html, Selector};
use std::time::Duration;

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_SECS: u64 = 5;

/// Identifies Ortobot per [WMF User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy).
/// `wikimedia_contact` comes from [`crate::config::OrtobotConfig`] (`wikimedia_contact` in `ortobot.toml`).
pub fn wikimedia_http_user_agent(wikimedia_contact: &str) -> String {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    format!("OrtobotBot/{VERSION} ({wikimedia_contact}; Wikipedia Spanish orthography checker) ortobot-backend/{VERSION}")
}

fn wikimedia_identified(
    builder: reqwest::RequestBuilder,
    wikimedia_contact: &str,
) -> reqwest::RequestBuilder {
    let ua = wikimedia_http_user_agent(wikimedia_contact);
    builder
        .header(reqwest::header::USER_AGENT, ua.as_str())
        .header(reqwest::header::HeaderName::from_static("api-user-agent"), ua)
}

/// Parses `Retry-After` as a delay in seconds, falling back to exponential backoff.
fn retry_after_secs(headers: &reqwest::header::HeaderMap, attempt: u32) -> u64 {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| BASE_BACKOFF_SECS * 2u64.pow(attempt))
}

/// Sends a Wikimedia-identified request, retrying on HTTP 429 with `Retry-After` / exponential backoff.
pub async fn wikimedia_send(
    builder: reqwest::RequestBuilder,
    wikimedia_contact: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut current = wikimedia_identified(builder, wikimedia_contact);
    let mut attempts = 0u32;

    loop {
        let next = current.try_clone();
        let response = current.send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempts < MAX_RETRIES {
            if let Some(next_builder) = next {
                let delay = retry_after_secs(response.headers(), attempts);
                tracing::warn!(
                    "Wikimedia rate-limited (429); retrying in {delay}s (attempt {}/{})",
                    attempts + 1,
                    MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(delay)).await;
                attempts += 1;
                current = next_builder;
                continue;
            }
        }

        return Ok(response);
    }
}

#[derive(Debug, Clone)]
pub struct WikipediaPage {
    pub title: String,
    pub revision_id: String,
    pub html: String,
}

#[derive(thiserror::Error, Debug)]
pub enum WikipediaError {
    #[error("failed to fetch url: {0}")]
    Request(#[from] reqwest::Error),
    #[error("upstream returned non-success status: {0}")]
    UpstreamStatus(reqwest::StatusCode),
}

pub async fn fetch_page(
    client: &Client,
    url: &str,
    wikimedia_contact: &str,
) -> Result<WikipediaPage, WikipediaError> {
    let response = wikimedia_send(
        client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html"),
        wikimedia_contact,
    )
    .await?;
    let status = response.status();

    if !status.is_success() {
        return Err(WikipediaError::UpstreamStatus(status));
    }

    let revision_id = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_owned())
        .unwrap_or_else(|| "unknown".to_string());

    let html = response.text().await?;
    let title = extract_title(&html).unwrap_or_else(|| "Untitled page".to_string());

    Ok(WikipediaPage {
        title,
        revision_id,
        html,
    })
}

fn extract_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);

    let h1_selector = Selector::parse("h1").ok()?;
    if let Some(title) = document
        .select(&h1_selector)
        .next()
        .map(|node| node.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(title);
    }

    let title_selector = Selector::parse("title").ok()?;
    document
        .select(&title_selector)
        .next()
        .map(|node| node.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
}
