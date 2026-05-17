use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleRecord {
    pub id: i64,
    pub page_title: String,
    pub page_url: String,
    pub revision_id: String,
    pub wrong_words: Vec<String>,
    pub checked_at: String,
}

pub fn init_db(db_path: &str) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS articles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            page_title TEXT NOT NULL,
            page_url TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            wrong_words TEXT NOT NULL,
            checked_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ignored_words (
            word TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS oauth_tokens (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            access_token TEXT NOT NULL,
            refresh_token TEXT,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

pub fn insert_article(
    db_path: &str,
    page_title: &str,
    page_url: &str,
    revision_id: &str,
    wrong_words: &[String],
) -> rusqlite::Result<i64> {
    let conn = Connection::open(db_path)?;
    let checked_at = Utc::now().to_rfc3339();
    let wrong_words_json = serde_json::to_string(wrong_words)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    conn.execute(
        r#"
        INSERT INTO articles (page_title, page_url, revision_id, wrong_words, checked_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            page_title,
            page_url,
            revision_id,
            wrong_words_json,
            checked_at
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn list_articles(db_path: &str) -> rusqlite::Result<Vec<ArticleRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, page_title, page_url, revision_id, wrong_words, checked_at
        FROM articles
        ORDER BY id DESC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        let wrong_words_json: String = row.get(4)?;
        let wrong_words: Vec<String> = serde_json::from_str(&wrong_words_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;

        Ok(ArticleRecord {
            id: row.get(0)?,
            page_title: row.get(1)?,
            page_url: row.get(2)?,
            revision_id: row.get(3)?,
            wrong_words,
            checked_at: row.get(5)?,
        })
    })?;

    rows.collect()
}

pub fn delete_article(db_path: &str, id: i64) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM articles WHERE id = ?1", params![id])
}

pub fn get_article(db_path: &str, id: i64) -> rusqlite::Result<Option<ArticleRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, page_title, page_url, revision_id, wrong_words, checked_at FROM articles WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        let wrong_words_json: String = row.get(4)?;
        let wrong_words: Vec<String> = serde_json::from_str(&wrong_words_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(ArticleRecord {
            id: row.get(0)?,
            page_title: row.get(1)?,
            page_url: row.get(2)?,
            revision_id: row.get(3)?,
            wrong_words,
            checked_at: row.get(5)?,
        })
    })?;
    rows.next().transpose()
}

#[derive(Debug, Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: String,
}

pub fn store_oauth_token(
    db_path: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: &str,
) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO oauth_tokens (id, access_token, refresh_token, expires_at, created_at)
        VALUES (1, ?1, ?2, ?3, ?4)
        ON CONFLICT(id) DO UPDATE SET
            access_token = excluded.access_token,
            refresh_token = excluded.refresh_token,
            expires_at = excluded.expires_at,
            created_at = excluded.created_at
        "#,
        params![access_token, refresh_token, expires_at, created_at],
    )?;
    Ok(())
}

pub fn get_oauth_token(db_path: &str) -> rusqlite::Result<Option<OAuthToken>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT access_token, refresh_token, expires_at FROM oauth_tokens WHERE id = 1",
    )?;
    let mut rows = stmt.query_map([], |row| {
        Ok(OAuthToken {
            access_token: row.get(0)?,
            refresh_token: row.get(1)?,
            expires_at: row.get(2)?,
        })
    })?;
    rows.next().transpose()
}

pub fn delete_oauth_token(db_path: &str) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM oauth_tokens WHERE id = 1", [])
}

pub fn insert_ignored_word(db_path: &str, word: &str) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO ignored_words (word, created_at)
        VALUES (?1, ?2)
        ON CONFLICT(word) DO NOTHING
        "#,
        params![word, created_at],
    )?;
    Ok(())
}

pub fn list_ignored_words(db_path: &str) -> rusqlite::Result<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT word
        FROM ignored_words
        ORDER BY word ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn delete_ignored_word(db_path: &str, word: &str) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM ignored_words WHERE word = ?1", params![word])
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    use super::{delete_ignored_word, init_db, insert_ignored_word, list_ignored_words};

    fn temp_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid clock")
            .as_nanos();
        std::env::temp_dir().join(format!("wordfixer-test-{nanos}.db"))
    }

    #[test]
    fn ignored_words_crud_roundtrip() {
        let db_path = temp_db_path();
        let db_path_str = db_path.to_string_lossy().to_string();

        init_db(&db_path_str).expect("db init should work");
        insert_ignored_word(&db_path_str, "mustafá").expect("insert should work");
        insert_ignored_word(&db_path_str, "mustafá").expect("duplicate insert should be ignored");
        insert_ignored_word(&db_path_str, "fazil").expect("insert should work");

        let words = list_ignored_words(&db_path_str).expect("list should work");
        assert_eq!(words, vec!["fazil".to_string(), "mustafá".to_string()]);

        let deleted = delete_ignored_word(&db_path_str, "fazil").expect("delete should work");
        assert_eq!(deleted, 1);
        let words_after_delete = list_ignored_words(&db_path_str).expect("list should work");
        assert_eq!(words_after_delete, vec!["mustafá".to_string()]);

        let _ = fs::remove_file(db_path);
    }
}
