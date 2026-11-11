use chrono::Local;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Represents a single clipboard record in history.
#[derive(Debug, Clone)]
pub(crate) struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub created_at: String,
}

/// Thread-safe handle to the encrypted `SQLCipher` database.
#[derive(Clone)]
pub(crate) struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Opens or creates the SQLite database, unlocks it with the `SQLCipher` key,
    /// and initializes the schema.
    pub(crate) fn open(
        path: &Path,
        encryption_key: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(path)?;

        // Apply SQLCipher encryption key
        // Using pragma_update ensures the key is passed safely into SQLCipher.
        conn.pragma_update(None, "key", encryption_key)?;

        // Verify the encryption key is valid by reading the sqlite master catalog.
        // If the key is incorrect or database is corrupted, this will fail immediately.
        let _test: i64 =
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;

        // Initialize schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_id ON clipboard_history(id DESC);",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Inserts a new clipboard entry, deduplicating against the latest entry.
    /// Returns `Ok(Some(id))` if inserted, or `Ok(None)` if duplicate.
    pub(crate) fn insert(&self, content: &str) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Check if the most recent entry has the exact same content
        let mut stmt =
            conn.prepare("SELECT content FROM clipboard_history ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let last_content: String = row.get(0)?;
            if last_content == content {
                return Ok(None); // Duplicate, ignore
            }
        }

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO clipboard_history (content, created_at) VALUES (?1, ?2)",
            params![content, now],
        )?;

        Ok(Some(conn.last_insert_rowid()))
    }

    /// Fetches the most recent clipboard entries, optionally filtered by search text.
    pub(crate) fn get_recent(
        &self,
        limit: usize,
        filter: Option<&str>,
    ) -> Result<Vec<ClipboardItem>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut items = Vec::new();
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        if let Some(search) = filter.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", search.trim());
            let mut stmt = conn.prepare(
                "SELECT id, content, created_at FROM clipboard_history
                 WHERE content LIKE ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![pattern, limit_i64])?;
            while let Some(row) = rows.next()? {
                items.push(ClipboardItem {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    created_at: row.get(2)?,
                });
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, content, created_at FROM clipboard_history
                 ORDER BY id DESC LIMIT ?1",
            )?;
            let mut rows = stmt.query(params![limit_i64])?;
            while let Some(row) = rows.next()? {
                items.push(ClipboardItem {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    created_at: row.get(2)?,
                });
            }
        }

        Ok(items)
    }

    /// Deletes a specific entry by its primary key ID.
    pub(crate) fn delete(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Clears all clipboard history.
    pub(crate) fn clear(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_history", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn test_open_and_insert() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path, TEST_KEY).expect("Failed to open encrypted db");

        let id = db.insert("Hello World").expect("Insert failed");
        assert_eq!(id, Some(1));

        let items = db.get_recent(10, None).expect("Query failed");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "Hello World");
    }

    #[test]
    fn test_deduplication() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("dedup.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let first = db.insert("Duplicate content").unwrap();
        assert!(first.is_some());

        let second = db.insert("Duplicate content").unwrap();
        assert_eq!(second, None, "Consecutive identical insert must be skipped");

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_search_filter() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        db.insert("cargo build --release").unwrap();
        db.insert("git commit -m 'feat: add tests'").unwrap();
        db.insert("cargo clippy --all-targets").unwrap();

        let cargo_items = db.get_recent(10, Some("cargo")).unwrap();
        assert_eq!(cargo_items.len(), 2);

        let git_items = db.get_recent(10, Some("git")).unwrap();
        assert_eq!(git_items.len(), 1);
        assert_eq!(git_items[0].content, "git commit -m 'feat: add tests'");

        let empty_search = db.get_recent(10, Some("   ")).unwrap();
        assert_eq!(empty_search.len(), 3);
    }

    #[test]
    fn test_delete_and_clear() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("delete.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let id1 = db.insert("First").unwrap().unwrap();
        let id2 = db.insert("Second").unwrap().unwrap();

        db.delete(id1).unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id2);

        db.clear().unwrap();
        let items_after_clear = db.get_recent(10, None).unwrap();
        assert!(items_after_clear.is_empty());
    }

    #[test]
    fn test_invalid_key_fails_to_open() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("crypto_gate.db");

        // 1. Create with correct key
        {
            let db = Database::open(&db_path, TEST_KEY).unwrap();
            db.insert("Secret message").unwrap();
        }

        // 2. Open with wrong key - must fail verification
        let wrong_key = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let result = Database::open(&db_path, wrong_key);
        assert!(result.is_err(), "Opening with wrong key must fail");
    }
}
