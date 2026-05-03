use reqwest::Client;
use scraper::{Html, Selector};

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

pub async fn fetch_page(client: &Client, url: &str) -> Result<WikipediaPage, WikipediaError> {
    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Ortobot/0.1 (self-hosted spelling checker)",
        )
        .header("Api-User-Agent", "Ortobot/0.1 (self-hosted spelling checker)")
        .header(reqwest::header::ACCEPT, "text/html")
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
