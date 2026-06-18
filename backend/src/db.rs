use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// How long an analysis is kept before lazy cleanup removes it (from `checked_at`).
pub const RETENTION_DAYS: i64 = 7;

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

    // WAL keeps reads non-blocking while the lazy expiry DELETE holds a write lock.
    conn.pragma_update(None, "journal_mode", "WAL")?;

    // Migrate old single-row oauth_tokens table (id=1 constraint) to per-session schema.
    let has_session_col: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('oauth_tokens') WHERE name = 'session_id'",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0) > 0;
    if !has_session_col {
        conn.execute("DROP TABLE IF EXISTS oauth_tokens", [])?;
    }

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
        CREATE TABLE IF NOT EXISTS always_wrong_words (
            word TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS oauth_tokens (
            session_id TEXT PRIMARY KEY,
            access_token TEXT NOT NULL,
            refresh_token TEXT,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS edit_counts (
            username TEXT PRIMARY KEY,
            edit_count INTEGER NOT NULL DEFAULT 0,
            last_edit_at TEXT NOT NULL
        );
        "#,
    )?;

    // Scope analyses to the browser session. Pre-existing rows get NULL and stop
    // appearing for anyone; the lazy expiry sweep removes them.
    let has_session_col: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'session_id'",
        [],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0) > 0;
    if !has_session_col {
        conn.execute("ALTER TABLE articles ADD COLUMN session_id TEXT", [])?;
    }

    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_articles_checked_at ON articles(checked_at);
        CREATE INDEX IF NOT EXISTS idx_articles_session_id ON articles(session_id);
        "#,
    )?;
    Ok(())
}

pub fn insert_article(
    db_path: &str,
    session_id: &str,
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
        INSERT INTO articles (session_id, page_title, page_url, revision_id, wrong_words, checked_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            session_id,
            page_title,
            page_url,
            revision_id,
            wrong_words_json,
            checked_at
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn list_articles_for_session(
    db_path: &str,
    session_id: &str,
) -> rusqlite::Result<Vec<ArticleRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, page_title, page_url, revision_id, wrong_words, checked_at
        FROM articles
        WHERE session_id = ?1
        ORDER BY id DESC
        "#,
    )?;

    let rows = stmt.query_map(params![session_id], |row| {
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

pub fn delete_article(db_path: &str, session_id: &str, id: i64) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "DELETE FROM articles WHERE id = ?1 AND session_id = ?2",
        params![id, session_id],
    )
}

/// Lazy cleanup: removes analyses older than the cutoff (RFC3339). Returns rows deleted.
pub fn delete_expired_articles(db_path: &str, cutoff: &str) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM articles WHERE checked_at < ?1", params![cutoff])
}

pub fn get_article(
    db_path: &str,
    session_id: &str,
    id: i64,
) -> rusqlite::Result<Option<ArticleRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, page_title, page_url, revision_id, wrong_words, checked_at FROM articles WHERE id = ?1 AND session_id = ?2",
    )?;
    let mut rows = stmt.query_map(params![id, session_id], |row| {
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
    session_id: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: &str,
) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO oauth_tokens (session_id, access_token, refresh_token, expires_at, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(session_id) DO UPDATE SET
            access_token = excluded.access_token,
            refresh_token = excluded.refresh_token,
            expires_at = excluded.expires_at,
            created_at = excluded.created_at
        "#,
        params![session_id, access_token, refresh_token, expires_at, created_at],
    )?;
    Ok(())
}

pub fn get_oauth_token(db_path: &str, session_id: &str) -> rusqlite::Result<Option<OAuthToken>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT access_token, refresh_token, expires_at FROM oauth_tokens WHERE session_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![session_id], |row| {
        Ok(OAuthToken {
            access_token: row.get(0)?,
            refresh_token: row.get(1)?,
            expires_at: row.get(2)?,
        })
    })?;
    rows.next().transpose()
}

pub fn delete_oauth_token(db_path: &str, session_id: &str) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM oauth_tokens WHERE session_id = ?1", params![session_id])
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

pub fn insert_always_wrong_word(db_path: &str, word: &str) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    let created_at = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO always_wrong_words (word, created_at)
        VALUES (?1, ?2)
        ON CONFLICT(word) DO NOTHING
        "#,
        params![word, created_at],
    )?;
    Ok(())
}

pub fn list_always_wrong_words(db_path: &str) -> rusqlite::Result<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT word
        FROM always_wrong_words
        ORDER BY word ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn delete_always_wrong_word(db_path: &str, word: &str) -> rusqlite::Result<usize> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM always_wrong_words WHERE word = ?1", params![word])
}

#[derive(Debug, Clone, Serialize)]
pub struct EditCount {
    pub username: String,
    pub edit_count: i64,
}

/// Records one edit by `username`, bumping their running total by one.
pub fn increment_edit_count(db_path: &str, username: &str) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO edit_counts (username, edit_count, last_edit_at)
        VALUES (?1, 1, ?2)
        ON CONFLICT(username) DO UPDATE SET
            edit_count = edit_count + 1,
            last_edit_at = excluded.last_edit_at
        "#,
        params![username, now],
    )?;
    Ok(())
}

/// Lists all editors and their totals, most prolific first.
pub fn list_edit_counts(db_path: &str) -> rusqlite::Result<Vec<EditCount>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT username, edit_count
        FROM edit_counts
        ORDER BY edit_count DESC, username ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(EditCount {
            username: row.get(0)?,
            edit_count: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Removes `word` from the article's wrong_words list.
/// Returns `true` if the article was deleted (no words left), `false` if updated.
pub fn remove_word_from_article(
    db_path: &str,
    session_id: &str,
    article_id: i64,
    word: &str,
) -> rusqlite::Result<bool> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT wrong_words FROM articles WHERE id = ?1 AND session_id = ?2",
    )?;
    let wrong_words_json: Option<String> = stmt
        .query_map(params![article_id, session_id], |row| row.get(0))?
        .next()
        .transpose()?;

    let Some(json) = wrong_words_json else {
        return Ok(false);
    };

    let mut words: Vec<String> = serde_json::from_str(&json)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    words.retain(|w| w != word);

    if words.is_empty() {
        conn.execute(
            "DELETE FROM articles WHERE id = ?1 AND session_id = ?2",
            params![article_id, session_id],
        )?;
        return Ok(true);
    }

    let updated_json = serde_json::to_string(&words)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE articles SET wrong_words = ?1 WHERE id = ?2 AND session_id = ?3",
        params![updated_json, article_id, session_id],
    )?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    use super::{
        delete_expired_articles, delete_ignored_word, init_db, insert_article, insert_ignored_word,
        list_articles_for_session, list_ignored_words,
    };
    use rusqlite::{params, Connection};

    fn temp_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid clock")
            .as_nanos();
        std::env::temp_dir().join(format!("orthonaut-test-{nanos}.db"))
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

    #[test]
    fn articles_are_scoped_to_session() {
        let db_path = temp_db_path();
        let db = db_path.to_string_lossy().to_string();
        init_db(&db).expect("db init should work");

        insert_article(&db, "sess-a", "Page A", "http://a", "1", &["foo".into()])
            .expect("insert a");
        insert_article(&db, "sess-b", "Page B", "http://b", "2", &["bar".into()])
            .expect("insert b");

        let a = list_articles_for_session(&db, "sess-a").expect("list a");
        let b = list_articles_for_session(&db, "sess-b").expect("list b");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].page_title, "Page A");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].page_title, "Page B");
        // An unknown session sees nothing.
        assert!(list_articles_for_session(&db, "nobody").expect("list").is_empty());

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn expired_articles_are_swept() {
        let db_path = temp_db_path();
        let db = db_path.to_string_lossy().to_string();
        init_db(&db).expect("db init should work");

        let old_id = insert_article(&db, "sess", "Old", "http://old", "1", &["foo".into()])
            .expect("insert old");
        insert_article(&db, "sess", "Fresh", "http://fresh", "2", &["bar".into()])
            .expect("insert fresh");

        // Backdate the first article well past any retention window.
        let conn = Connection::open(&db).expect("open");
        conn.execute(
            "UPDATE articles SET checked_at = ?1 WHERE id = ?2",
            params!["2000-01-01T00:00:00+00:00", old_id],
        )
        .expect("backdate");
        drop(conn);

        let cutoff = "2020-01-01T00:00:00+00:00";
        let removed = delete_expired_articles(&db, cutoff).expect("sweep");
        assert_eq!(removed, 1);

        let remaining = list_articles_for_session(&db, "sess").expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].page_title, "Fresh");

        let _ = fs::remove_file(db_path);
    }
}
