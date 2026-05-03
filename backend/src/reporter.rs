use serde::Serialize;

use crate::db::ArticleRecord;

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub status: String,
    pub result: Option<ArticleResult>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArticleResult {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub revision_id: String,
    pub wrong_words: Vec<String>,
    pub checked_at: String,
}

impl From<ArticleRecord> for ArticleResult {
    fn from(value: ArticleRecord) -> Self {
        Self {
            id: value.id,
            title: value.page_title,
            url: value.page_url,
            revision_id: value.revision_id,
            wrong_words: value.wrong_words,
            checked_at: value.checked_at,
        }
    }
}

pub fn ok_message(title: String) -> CheckResponse {
    CheckResponse {
        status: "ok".to_string(),
        result: None,
        message: Some(format!(
            "No se encontraron errores ortográficos claros en {title}"
        )),
    }
}

pub fn errors_found(result: ArticleResult) -> CheckResponse {
    CheckResponse {
        status: "errors".to_string(),
        result: Some(result),
        message: None,
    }
}
