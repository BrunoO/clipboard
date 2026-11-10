use chrono::Local;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Represents a single clipboard record in history.
#[derive(Debug, Clone)]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub created_at: String,
}

/// Thread-safe handle to the encrypted SQLCipher database.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens or creates the SQLite database, unlocks it with the SQLCipher key,
    /// and initializes the schema.
    pub fn open(path: &Path, encryption_key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(path)?;

        // Apply SQLCipher encryption key
        // Using pragma_update ensures the key is passed safely into SQLCipher.
        conn.pragma_update(None, "key", encryption_key)?;

        // Verify the encryption key is valid by reading the sqlite master catalog.
        // If the key is incorrect or database is corrupted, this will fail immediately.
        let _test: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;

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
    pub fn insert(&self, content: &str) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Check if the most recent entry has the exact same content
        let mut stmt = conn.prepare("SELECT content FROM clipboard_history ORDER BY id DESC LIMIT 1")?;
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
    pub fn get_recent(&self, limit: usize, filter: Option<&str>) -> Result<Vec<ClipboardItem>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut items = Vec::new();

        if let Some(search) = filter.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", search.trim());
            let mut stmt = conn.prepare(
                "SELECT id, content, created_at FROM clipboard_history
                 WHERE content LIKE ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![pattern, limit as i64])?;
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
            let mut rows = stmt.query(params![limit as i64])?;
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
    pub fn delete(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Clears all clipboard history.
    pub fn clear(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_history", [])?;
        Ok(())
    }
}
