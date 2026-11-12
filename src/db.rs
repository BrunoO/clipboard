use chrono::Local;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Represents the type of clipboard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    Text,
    Image,
    Url,
    Code,
    Color,
}

/// Returns true if the string is a valid HTTP or HTTPS URL with a host.
pub(crate) fn is_valid_url(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        if let Ok(parsed) = url::Url::parse(trimmed) {
            return parsed.has_host();
        }
    }
    false
}

/// Represents a single clipboard record in history.
#[derive(Debug, Clone)]
pub(crate) struct ClipboardItem {
    pub(crate) id: i64,
    pub(crate) kind: ItemKind,
    pub(crate) content: String,
    pub(crate) image_data: Option<Vec<u8>>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) created_at: String,
}

impl ClipboardItem {
    /// Attempts to parse the item content as an HTTP/HTTPS URL.
    pub(crate) fn parsed_url(&self) -> Option<url::Url> {
        if self.kind == ItemKind::Image {
            return None;
        }
        let trimmed = self.content.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            if let Ok(u) = url::Url::parse(trimmed) {
                if u.has_host() {
                    return Some(u);
                }
            }
        }
        None
    }

    /// Extracts the host domain name (e.g. "github.com") if this item is a valid URL.
    pub(crate) fn url_host(&self) -> Option<String> {
        self.parsed_url()
            .and_then(|u| u.host_str().map(str::to_lowercase))
    }

    /// Detects if the item content is a recognized programming language or data format.
    pub(crate) fn code_language(&self) -> Option<crate::code::CodeLanguage> {
        if self.kind == ItemKind::Image
            || self.kind == ItemKind::Url
            || self.kind == ItemKind::Color
        {
            return None;
        }
        crate::code::detect_code_language(&self.content)
    }

    /// Parses the item content as a color if recognized.
    pub(crate) fn parsed_color(&self) -> Option<crate::color::ParsedColor> {
        if self.kind == ItemKind::Image || self.kind == ItemKind::Url {
            return None;
        }
        crate::color::parse_color(&self.content)
    }

    /// Extracts distinct colors embedded within the text or code content.
    #[must_use]
    pub(crate) fn embedded_colors(&self) -> Vec<crate::color::ParsedColor> {
        if self.kind == ItemKind::Image || self.kind == ItemKind::Url {
            return Vec::new();
        }
        if self.kind == ItemKind::Color {
            return self.parsed_color().into_iter().collect();
        }
        crate::color::extract_colors(&self.content)
    }
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
    /// and initializes or migrates the schema.
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
                kind TEXT NOT NULL DEFAULT 'text',
                content TEXT NOT NULL,
                image_data BLOB,
                width INTEGER,
                height INTEGER,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_id ON clipboard_history(id DESC);
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        // Graceful migration: ensure all columns exist if created with an older schema
        let _ = conn.execute(
            "ALTER TABLE clipboard_history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE clipboard_history ADD COLUMN image_data BLOB",
            [],
        );
        let _ = conn.execute("ALTER TABLE clipboard_history ADD COLUMN width INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE clipboard_history ADD COLUMN height INTEGER",
            [],
        );

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Inserts a new text clipboard entry, deduplicating against the latest entry.
    /// Returns `Ok(Some(id))` if inserted, or `Ok(None)` if duplicate.
    pub(crate) fn insert(&self, content: &str) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Check if the most recent entry has the exact same content
        let mut stmt =
            conn.prepare("SELECT kind, content FROM clipboard_history ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let last_kind: String = row.get(0)?;
            let last_content: String = row.get(1)?;
            if (last_kind == "text"
                || last_kind == "url"
                || last_kind == "code"
                || last_kind == "color")
                && last_content == content
            {
                return Ok(None); // Duplicate, ignore
            }
        }

        let is_url = is_valid_url(content);
        let is_color = !is_url && crate::color::parse_color(content).is_some();
        let is_code = !is_url && !is_color && crate::code::detect_code_language(content).is_some();
        let kind_str = if is_url {
            "url"
        } else if is_color {
            "color"
        } else if is_code {
            "code"
        } else {
            "text"
        };

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO clipboard_history (kind, content, created_at) VALUES (?1, ?2, ?3)",
            params![kind_str, content, now],
        )?;

        Ok(Some(conn.last_insert_rowid()))
    }

    /// Inserts a new image clipboard entry, deduplicating against the latest entry.
    /// Returns `Ok(Some(id))` if inserted, or `Ok(None)` if duplicate.
    pub(crate) fn insert_image(
        &self,
        width: u32,
        height: u32,
        png_bytes: &[u8],
    ) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Check if the most recent entry has the exact same image content
        let mut stmt = conn
            .prepare("SELECT kind, image_data FROM clipboard_history ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let last_kind: String = row.get(0)?;
            if last_kind == "image" {
                if let Ok(last_bytes) = row.get::<_, Vec<u8>>(1) {
                    if last_bytes == png_bytes {
                        return Ok(None); // Duplicate, ignore
                    }
                }
            }
        }

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let content = format!("[Image {width}x{height}]");
        conn.execute(
            "INSERT INTO clipboard_history (kind, content, image_data, width, height, created_at)
             VALUES ('image', ?1, ?2, ?3, ?4, ?5)",
            params![content, png_bytes, width, height, now],
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

        let row_to_item = |row: &rusqlite::Row| -> Result<ClipboardItem, rusqlite::Error> {
            let kind_str: String = row.get(1).unwrap_or_else(|_| "text".to_string());
            let content: String = row.get(2)?;
            let kind = match kind_str.as_str() {
                "image" => ItemKind::Image,
                "url" => ItemKind::Url,
                "code" => ItemKind::Code,
                "color" => ItemKind::Color,
                _ => {
                    if is_valid_url(&content) {
                        ItemKind::Url
                    } else if crate::color::parse_color(&content).is_some() {
                        ItemKind::Color
                    } else if crate::code::detect_code_language(&content).is_some() {
                        ItemKind::Code
                    } else {
                        ItemKind::Text
                    }
                }
            };
            Ok(ClipboardItem {
                id: row.get(0)?,
                kind,
                content,
                image_data: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
                created_at: row.get(6)?,
            })
        };

        if let Some(search) = filter.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", search.trim());
            let mut stmt = conn.prepare(
                "SELECT id, kind, content, image_data, width, height, created_at
                 FROM clipboard_history
                 WHERE content LIKE ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![pattern, limit_i64])?;
            while let Some(row) = rows.next()? {
                items.push(row_to_item(row)?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, kind, content, image_data, width, height, created_at
                 FROM clipboard_history
                 ORDER BY id DESC LIMIT ?1",
            )?;
            let mut rows = stmt.query(params![limit_i64])?;
            while let Some(row) = rows.next()? {
                items.push(row_to_item(row)?);
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

    /// Fetches a setting value by key from the database.
    pub(crate) fn get_setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Stores or updates a setting value by key.
    pub(crate) fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
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
        assert_eq!(items[0].kind, ItemKind::Text);
        assert_eq!(items[0].content, "Hello World");
    }

    #[test]
    fn test_insert_image_and_deduplication() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("image_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let dummy_png = vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3, 4];
        let id = db.insert_image(320, 240, &dummy_png).unwrap();
        assert_eq!(id, Some(1));

        // Duplicate insert should be skipped
        let dup = db.insert_image(320, 240, &dummy_png).unwrap();
        assert_eq!(dup, None);

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Image);
        assert_eq!(items[0].width, Some(320));
        assert_eq!(items[0].height, Some(240));
        assert_eq!(items[0].image_data.as_deref(), Some(dummy_png.as_slice()));
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
        let dummy_png = vec![1, 2, 3];
        db.insert_image(1920, 1080, &dummy_png).unwrap();

        let cargo_items = db.get_recent(10, Some("cargo")).unwrap();
        assert_eq!(cargo_items.len(), 2);

        let git_items = db.get_recent(10, Some("git")).unwrap();
        assert_eq!(git_items.len(), 1);
        assert_eq!(git_items[0].content, "git commit -m 'feat: add tests'");

        let image_items = db.get_recent(10, Some("Image")).unwrap();
        assert_eq!(image_items.len(), 1);
        assert_eq!(image_items[0].kind, ItemKind::Image);

        let empty_search = db.get_recent(10, Some("   ")).unwrap();
        assert_eq!(empty_search.len(), 4);
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

    #[test]
    fn test_is_valid_url() {
        assert!(is_valid_url("https://github.com/rust-lang/rust"));
        assert!(is_valid_url("http://localhost:8080/dashboard"));
        assert!(is_valid_url("  https://docs.rs/eframe/latest  \n"));
        assert!(!is_valid_url("not a url"));
        assert!(!is_valid_url("ftp://files.example.com"));
        assert!(!is_valid_url("https://"));
        assert!(!is_valid_url("Hello https://github.com world"));
    }

    #[test]
    fn test_insert_url_and_domain_extraction() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("url_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let id = db
            .insert("https://github.com/BrunoO/clipboard")
            .unwrap()
            .unwrap();
        assert_eq!(id, 1);

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Url);
        assert_eq!(items[0].url_host().as_deref(), Some("github.com"));

        // Deduplication for URLs
        let dup = db.insert("https://github.com/BrunoO/clipboard").unwrap();
        assert_eq!(dup, None);
    }

    #[test]
    fn test_insert_code_and_detection() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("code_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let rust_code = "pub fn add(x: i32, y: i32) -> i32 {\n    x + y\n}";
        let id = db.insert(rust_code).unwrap().unwrap();
        assert_eq!(id, 1);

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Code);
        assert_eq!(
            items[0].code_language(),
            Some(crate::code::CodeLanguage::Rust)
        );

        // Deduplication for code
        let dup = db.insert(rust_code).unwrap();
        assert_eq!(dup, None);
    }

    #[test]
    fn test_insert_color_and_detection() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("color_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        let hex_color = "#FF5733";
        let id = db.insert(hex_color).unwrap().unwrap();
        assert_eq!(id, 1);

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Color);
        let parsed = items[0].parsed_color().expect("Should parse color");
        assert_eq!(parsed.r, 255);
        assert_eq!(parsed.g, 87);
        assert_eq!(parsed.b, 51);
        assert_eq!(parsed.to_hex(), "#FF5733");

        // Deduplication for colors
        let dup = db.insert(hex_color).unwrap();
        assert_eq!(dup, None);
    }

    #[test]
    fn test_settings_persistence() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("settings_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        assert_eq!(db.get_setting("hotkey").unwrap(), None);

        db.set_setting("hotkey", "CtrlShiftV").unwrap();
        assert_eq!(
            db.get_setting("hotkey").unwrap(),
            Some("CtrlShiftV".to_string())
        );

        // Test overwrite / upsert
        db.set_setting("hotkey", "AltSpace").unwrap();
        assert_eq!(
            db.get_setting("hotkey").unwrap(),
            Some("AltSpace".to_string())
        );
    }
}
