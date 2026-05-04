use reqwest::Client;
use scraper::{Html, Selector};

/// Identifies Ortobot per [WMF User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy).
/// `wikimedia_contact` comes from [`crate::config::OrtobotConfig`] (`wikimedia_contact` in `ortobot.toml`).
pub fn wikimedia_http_user_agent(wikimedia_contact: &str) -> String {
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    format!(
        "OrtobotBot/{VERSION} ({wikimedia_contact}; Wikipedia Spanish orthography checker) ortobot-backend/{VERSION} reqwest/0.12"
    )
}

pub fn wikimedia_identified(
    builder: reqwest::RequestBuilder,
    wikimedia_contact: &str,
) -> reqwest::RequestBuilder {
    let ua = wikimedia_http_user_agent(wikimedia_contact);
    builder
        .header(reqwest::header::USER_AGENT, ua.as_str())
        .header(reqwest::header::HeaderName::from_static("api-user-agent"), ua)
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
    let response = wikimedia_identified(
        client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html"),
        wikimedia_contact,
    )
        .send()
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
