use chrono::Local;
use rusqlite::{params, Connection, Result};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Represents the type of clipboard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    Text,
    Image,
    Url,
    Code,
    Color,
    Secret,
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
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) created_at: String,
    pub(crate) embedded_colors: Vec<crate::color::ParsedColor>,
    pub(crate) label: Option<String>,
    pub(crate) notes: Option<String>,
    pub(crate) is_pinned: bool,
}

impl ClipboardItem {
    /// Attempts to parse the item content as an HTTP/HTTPS URL.
    pub(crate) fn parsed_url(&self) -> Option<url::Url> {
        if self.kind == ItemKind::Image || self.kind == ItemKind::Secret {
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
            || self.kind == ItemKind::Secret
        {
            return None;
        }
        crate::code::detect_code_language(&self.content)
    }

    /// Parses the item content as a color if recognized.
    pub(crate) fn parsed_color(&self) -> Option<crate::color::ParsedColor> {
        if self.kind == ItemKind::Image
            || self.kind == ItemKind::Url
            || self.kind == ItemKind::Secret
        {
            return None;
        }
        crate::color::parse_color(&self.content)
    }

    /// Returns the precomputed distinct colors embedded within the text or code content.
    #[must_use]
    pub(crate) fn embedded_colors(&self) -> &[crate::color::ParsedColor] {
        &self.embedded_colors
    }
}

/// Failure modes for a full history clear.
#[derive(Debug)]
pub(crate) enum ClearError {
    /// Row deletion failed, so no history was removed.
    Delete(Box<rusqlite::Error>),
    /// Rows were removed, but secure compaction could not be completed.
    Compaction {
        vacuum: Option<Box<rusqlite::Error>>,
        wal_checkpoint: Option<Box<rusqlite::Error>>,
    },
}

impl fmt::Display for ClearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Delete(error) => write!(f, "failed to delete clipboard history: {error}"),
            Self::Compaction {
                vacuum,
                wal_checkpoint,
            } => {
                write!(f, "history rows were deleted but secure compaction failed")?;
                if let Some(error) = vacuum {
                    write!(f, "; VACUUM: {error}")?;
                }
                if let Some(error) = wal_checkpoint {
                    write!(f, "; WAL checkpoint: {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ClearError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Delete(error) => Some(error.as_ref() as _),
            Self::Compaction {
                vacuum,
                wal_checkpoint,
            } => vacuum
                .as_ref()
                .or(wal_checkpoint.as_ref())
                .map(|error| error.as_ref() as _),
        }
    }
}

struct DatabaseInner {
    conn: Mutex<Connection>,
    max_history: AtomicUsize,
    ignore_concealed: AtomicBool,
    auto_detect_secrets: AtomicBool,
    preserve_secrets_on_prune: AtomicBool,
    transient_text: Mutex<Option<zeroize::Zeroizing<String>>>,
}

/// Thread-safe handle to the encrypted `SQLCipher` database.
#[derive(Clone)]
pub(crate) struct Database(Arc<DatabaseInner>);

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

/// Domain error type for database opening, authentication, and schema migration.
#[derive(Debug)]
pub(crate) enum DbOpenError {
    /// SQLite or `SQLCipher` error during connection, pragma, or query execution.
    Sqlite(rusqlite::Error),
    /// Schema migration failed for a column.
    Migration {
        sql: &'static str,
        source: rusqlite::Error,
    },
}

impl std::fmt::Display for DbOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "Database open error: {err}"),
            Self::Migration { sql, source } => {
                write!(f, "Schema migration failed for `{sql}`: {source}")
            }
        }
    }
}

impl std::error::Error for DbOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::Migration { source, .. } => Some(source),
        }
    }
}

impl From<rusqlite::Error> for DbOpenError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

type AnnotationMeta = (Option<String>, Option<String>, bool);

fn normalize_annotation_field(value: Option<&str>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Returns `true` if the SQLite error represents an expected "duplicate column name" error
/// when running `ALTER TABLE` schema migrations on an already-migrated database.
fn is_duplicate_column_error(err: &rusqlite::Error) -> bool {
    if let rusqlite::Error::SqliteFailure(ffi_err, Some(msg)) = err {
        // SQLite uses generic SQLITE_ERROR (extended_code 1) for duplicate column additions,
        // with the error message prefixed by "duplicate column name".
        ffi_err.extended_code == 1 && msg.starts_with("duplicate column name")
    } else {
        false
    }
}

impl Database {
    /// Opens or creates the SQLite database, unlocks it with the `SQLCipher` key,
    /// enables WAL mode for high performance concurrency, and initializes or migrates the schema.
    ///
    /// # Errors
    /// Returns [`DbOpenError`] if the file cannot be opened, the encryption key is rejected,
    /// or SQLite pragmas/schema initialization/migration fail.
    pub(crate) fn open(
        path: &Path,
        encryption_key: &str,
    ) -> std::result::Result<Self, DbOpenError> {
        let conn = Connection::open(path)?;

        // Apply SQLCipher encryption key
        // Using pragma_update ensures the key is passed safely into SQLCipher.
        conn.pragma_update(None, "key", encryption_key)?;

        // Verify the encryption key is valid by reading the sqlite master catalog.
        // If the key is incorrect or database is corrupted, this will fail immediately.
        let _test: i64 =
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;

        // Wait up to 5 seconds if SQLite encounters a transient lock (e.g., WAL checkpoint
        // or another process briefly accessing the database).
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // Enable WAL mode, normal synchronous writing, foreign keys, and secure_delete
        // so deleted rows/secrets are immediately zero-overwritten on disk.
        let _: String = conn.query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))?;
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA secure_delete = ON;",
        )?;

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
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_kind_id ON clipboard_history(kind, id DESC);
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        // Graceful migration: ensure all columns exist if created with an older schema.
        // SQLite does not support `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, so duplicate-column
        // errors from already migrated schemas are filtered via `is_duplicate_column_error`.
        // Any unexpected migration failure immediately aborts `Database::open`.
        for sql in [
            "ALTER TABLE clipboard_history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
            "ALTER TABLE clipboard_history ADD COLUMN image_data BLOB",
            "ALTER TABLE clipboard_history ADD COLUMN width INTEGER",
            "ALTER TABLE clipboard_history ADD COLUMN height INTEGER",
            "ALTER TABLE clipboard_history ADD COLUMN label TEXT",
            "ALTER TABLE clipboard_history ADD COLUMN notes TEXT",
            "ALTER TABLE clipboard_history ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0",
        ] {
            if let Err(e) = conn.execute(sql, []) {
                if !is_duplicate_column_error(&e) {
                    return Err(DbOpenError::Migration { sql, source: e });
                }
            }
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_clipboard_history_pinned ON clipboard_history(is_pinned, id DESC);",
        )?;

        Ok(Self(Arc::new(DatabaseInner {
            conn: Mutex::new(conn),
            max_history: AtomicUsize::new(100),
            ignore_concealed: AtomicBool::new(true),
            auto_detect_secrets: AtomicBool::new(true),
            preserve_secrets_on_prune: AtomicBool::new(false),
            transient_text: Mutex::new(None),
        })))
    }

    /// Acquires the database mutex, recovering from poison if a previous thread panicked.
    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.conn.lock().unwrap_or_else(|poisoned| {
            eprintln!("⚠️ Database mutex recovered from poison");
            poisoned.into_inner()
        })
    }

    /// Acquires the transient text mutex, recovering from poison if a previous thread panicked.
    fn lock_transient(&self) -> std::sync::MutexGuard<'_, Option<zeroize::Zeroizing<String>>> {
        self.0.transient_text.lock().unwrap_or_else(|poisoned| {
            eprintln!("⚠️ Transient text mutex recovered from poison");
            poisoned.into_inner()
        })
    }

    /// Sets the maximum history limit dynamically.
    pub(crate) fn set_max_history(&self, max: usize) {
        self.0.max_history.store(max, Ordering::Relaxed);
    }

    /// Retrieves the current maximum history limit.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn max_history(&self) -> usize {
        self.0.max_history.load(Ordering::Relaxed)
    }

    /// Sets whether to ignore concealed clipboard data from password managers.
    pub(crate) fn set_ignore_concealed(&self, ignore: bool) {
        self.0.ignore_concealed.store(ignore, Ordering::Relaxed);
    }

    /// Checks whether to ignore concealed clipboard data.
    #[must_use]
    pub(crate) fn ignore_concealed(&self) -> bool {
        self.0.ignore_concealed.load(Ordering::Relaxed)
    }

    /// Sets whether to automatically classify likely secrets/API keys as `secret`.
    pub(crate) fn set_auto_detect_secrets(&self, enabled: bool) {
        self.0.auto_detect_secrets.store(enabled, Ordering::Relaxed);
    }

    /// Checks whether automatic secret detection is enabled.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn auto_detect_secrets(&self) -> bool {
        self.0.auto_detect_secrets.load(Ordering::Relaxed)
    }

    /// Sets whether to preserve secrets and passwords from being deleted during history pruning.
    pub(crate) fn set_preserve_secrets_on_prune(&self, preserve: bool) {
        self.0
            .preserve_secrets_on_prune
            .store(preserve, Ordering::Relaxed);
    }

    /// Checks whether secrets are preserved from deletion during history pruning.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn preserve_secrets_on_prune(&self) -> bool {
        self.0.preserve_secrets_on_prune.load(Ordering::Relaxed)
    }

    /// Synchronizes runtime atomic settings flags from persistent application settings.
    pub(crate) fn apply_settings(&self, settings: &crate::settings::AppSettings) {
        self.set_max_history(settings.max_history);
        self.set_ignore_concealed(settings.ignore_concealed);
        self.set_auto_detect_secrets(settings.auto_detect_secrets);
        self.set_preserve_secrets_on_prune(settings.preserve_secrets_on_prune);
    }

    /// Marks the given text to be ignored once by the background clipboard monitor.
    pub(crate) fn skip_next_capture(&self, text: &str) {
        let mut guard = self.lock_transient();
        *guard = Some(zeroize::Zeroizing::new(text.to_string()));
    }

    /// Clears any pending transient skip marker (e.g. if a clipboard write failed).
    pub(crate) fn clear_skip_capture(&self) {
        let mut guard = self.lock_transient();
        *guard = None;
    }

    /// Returns true (and clears the marker) if `text` matches a pending transient copy.
    #[must_use]
    pub(crate) fn take_if_skipped(&self, text: &str) -> bool {
        let mut guard = self.lock_transient();
        if guard.as_deref().map(String::as_str) == Some(text) {
            *guard = None;
            return true;
        }
        false
    }

    /// Prunes older clipboard items exceeding the maximum history threshold.
    /// Pinned vault items are always exempt from pruning.
    /// Returns the number of purged rows.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the prune `DELETE` fails.
    pub(crate) fn prune_history(&self, max_history: usize) -> Result<usize, rusqlite::Error> {
        if max_history == 0 {
            return Ok(0);
        }
        let conn = self.lock_conn();
        let limit_i64 = i64::try_from(max_history).unwrap_or(i64::MAX);
        let preserve = self.0.preserve_secrets_on_prune.load(Ordering::Relaxed);
        let deleted = if preserve {
            conn.execute(
                "DELETE FROM clipboard_history
                 WHERE is_pinned = 0
                   AND kind != 'secret'
                   AND id NOT IN (
                       SELECT id FROM clipboard_history
                       WHERE is_pinned = 0 AND kind != 'secret'
                       ORDER BY id DESC LIMIT ?1
                   )",
                params![limit_i64],
            )?
        } else {
            conn.execute(
                "DELETE FROM clipboard_history
                 WHERE is_pinned = 0
                   AND id NOT IN (
                       SELECT id FROM clipboard_history
                       WHERE is_pinned = 0
                       ORDER BY id DESC LIMIT ?1
                   )",
                params![limit_i64],
            )?
        };
        Ok(deleted)
    }

    /// Compacts and defragments the encrypted database to reclaim free disk space.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if `VACUUM` fails.
    #[cfg(test)]
    pub(crate) fn vacuum(&self) -> Result<(), rusqlite::Error> {
        let conn = self.lock_conn();
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Classifies text content without holding the database lock.
    fn classify_text_kind(&self, content: &str) -> &'static str {
        if self.0.auto_detect_secrets.load(Ordering::Relaxed)
            && crate::secret::is_likely_secret(content)
        {
            "secret"
        } else if is_valid_url(content) {
            "url"
        } else if crate::color::parse_color(content).is_some() {
            "color"
        } else if crate::code::detect_code_language(content).is_some() {
            "code"
        } else {
            "text"
        }
    }

    /// Prunes entries exceeding `max_limit` within an active SQLite transaction.
    /// Pinned vault items are always exempt from pruning.
    fn prune_in_tx(
        tx: &rusqlite::Transaction<'_>,
        max_limit: usize,
        preserve: bool,
    ) -> Result<(), rusqlite::Error> {
        if max_limit > 0 {
            let limit_i64 = i64::try_from(max_limit).unwrap_or(i64::MAX);
            if preserve {
                tx.execute(
                    "DELETE FROM clipboard_history
                     WHERE is_pinned = 0
                       AND kind != 'secret'
                       AND id NOT IN (
                           SELECT id FROM clipboard_history
                           WHERE is_pinned = 0 AND kind != 'secret'
                           ORDER BY id DESC LIMIT ?1
                       )",
                    params![limit_i64],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM clipboard_history
                     WHERE is_pinned = 0
                       AND id NOT IN (
                           SELECT id FROM clipboard_history
                           WHERE is_pinned = 0
                           ORDER BY id DESC LIMIT ?1
                       )",
                    params![limit_i64],
                )?;
            }
        }
        Ok(())
    }

    /// Captures the creation timestamp and prune settings shared by every insert path.
    fn insert_context(&self) -> (String, usize, bool) {
        (
            Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            self.0.max_history.load(Ordering::Relaxed),
            self.0.preserve_secrets_on_prune.load(Ordering::Relaxed),
        )
    }

    /// Reads the `(kind, content)` of the most recent history row, if any.
    fn last_text_entry(tx: &rusqlite::Transaction<'_>) -> Result<Option<(String, String)>> {
        let mut stmt =
            tx.prepare("SELECT kind, content FROM clipboard_history ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
            None => Ok(None),
        }
    }

    fn existing_text_metadata(
        tx: &rusqlite::Transaction<'_>,
        content: &str,
    ) -> Result<Option<AnnotationMeta>> {
        let mut stmt = tx.prepare(
            "SELECT label, notes, is_pinned
             FROM clipboard_history
             WHERE content = ?1 AND kind IN ('text', 'url', 'code', 'color', 'secret')
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![content])?;
        match rows.next()? {
            Some(row) => Ok(Some((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i64>(2).unwrap_or(0) != 0,
            ))),
            None => Ok(None),
        }
    }

    fn existing_secret_metadata(
        tx: &rusqlite::Transaction<'_>,
        content: &str,
    ) -> Result<Option<AnnotationMeta>> {
        let mut stmt = tx.prepare(
            "SELECT label, notes, is_pinned
             FROM clipboard_history
             WHERE content = ?1 AND kind = 'secret'
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![content])?;
        match rows.next()? {
            Some(row) => Ok(Some((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i64>(2).unwrap_or(0) != 0,
            ))),
            None => Ok(None),
        }
    }

    fn existing_image_metadata(
        tx: &rusqlite::Transaction<'_>,
        width: u32,
        height: u32,
        png_bytes: &[u8],
    ) -> Result<Option<AnnotationMeta>> {
        let mut stmt = tx.prepare(
            "SELECT label, notes, is_pinned
             FROM clipboard_history
             WHERE kind = 'image' AND width = ?1 AND height = ?2 AND image_data = ?3
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![width, height, png_bytes])?;
        match rows.next()? {
            Some(row) => Ok(Some((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i64>(2).unwrap_or(0) != 0,
            ))),
            None => Ok(None),
        }
    }

    /// Inserts a new text clipboard entry, deduplicating against history.
    /// If an identical item was already the most recent, returns `Ok(None)`.
    /// If an identical item exists earlier in history, deletes the stale copy and promotes
    /// the item to the top with an updated timestamp.
    /// Also automatically enforces the `max_history` limit.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if any statement in the insert transaction fails.
    pub(crate) fn insert(&self, content: &str) -> Result<Option<i64>, rusqlite::Error> {
        // Run CPU-bound classification (secret/URL/color/code heuristics) before
        // acquiring the database mutex so UI reads are not blocked.
        let kind_str = self.classify_text_kind(content);
        let (now, max_limit, preserve) = self.insert_context();

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        // 1. Check if the most recent entry has the exact same content
        if let Some((last_kind, last_content)) = Self::last_text_entry(&tx)? {
            if matches!(
                last_kind.as_str(),
                "text" | "url" | "code" | "color" | "secret"
            ) && last_content == content
            {
                return Ok(None); // Exact consecutive duplicate, ignore
            }
        }

        // 2. Remove any older duplicate so the item moves to the top of history,
        // preserving any existing vault annotation/pin metadata.
        let existing = Self::existing_text_metadata(&tx, content)?;
        tx.execute(
            "DELETE FROM clipboard_history
             WHERE content = ?1 AND kind IN ('text', 'url', 'code', 'color', 'secret')",
            params![content],
        )?;

        let (label, notes, is_pinned) = existing.unwrap_or((None, None, false));
        tx.execute(
            "INSERT INTO clipboard_history (kind, content, created_at, label, notes, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kind_str, content, now, label, notes, i64::from(is_pinned)],
        )?;

        let new_id = tx.last_insert_rowid();

        // 3. Automatically prune older entries beyond max_history
        Self::prune_in_tx(&tx, max_limit, preserve)?;
        tx.commit()?;

        Ok(Some(new_id))
    }

    /// Inserts a secret/sensitive text entry (e.g., generated password), deduplicating against
    /// existing secret entries and enforcing the `max_history` limit.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if any statement in the insert transaction fails.
    pub(crate) fn insert_secret(&self, content: &str) -> Result<Option<i64>, rusqlite::Error> {
        let (now, max_limit, preserve) = self.insert_context();

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        if let Some((last_kind, last_content)) = Self::last_text_entry(&tx)? {
            if last_kind == "secret" && last_content == content {
                return Ok(None);
            }
        }

        let existing = Self::existing_secret_metadata(&tx, content)?;
        tx.execute(
            "DELETE FROM clipboard_history WHERE content = ?1 AND kind = 'secret'",
            params![content],
        )?;

        let (label, notes, is_pinned) = existing.unwrap_or((None, None, false));
        tx.execute(
            "INSERT INTO clipboard_history (kind, content, created_at, label, notes, is_pinned)
             VALUES ('secret', ?1, ?2, ?3, ?4, ?5)",
            params![content, now, label, notes, i64::from(is_pinned)],
        )?;

        let new_id = tx.last_insert_rowid();

        Self::prune_in_tx(&tx, max_limit, preserve)?;
        tx.commit()?;

        Ok(Some(new_id))
    }

    /// Inserts a new image clipboard entry, deduplicating against history.
    /// If an identical image was already the most recent, returns `Ok(None)`.
    /// If an identical image exists earlier in history, deletes the stale copy and
    /// promotes the item to the top with an updated timestamp.
    /// Also automatically enforces the `max_history` limit.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if any statement in the insert transaction fails.
    pub(crate) fn insert_image(
        &self,
        width: u32,
        height: u32,
        png_bytes: &[u8],
    ) -> Result<Option<i64>, rusqlite::Error> {
        let (now, max_limit, preserve) = self.insert_context();
        let content = format!("[Image {width}x{height}]");

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        // 1. Check if the most recent entry has the exact same image content
        {
            let mut stmt = tx.prepare(
                "SELECT kind, image_data FROM clipboard_history ORDER BY id DESC LIMIT 1",
            )?;
            let mut rows = stmt.query([])?;
            if let Some(row) = rows.next()? {
                let last_kind: String = row.get(0)?;
                if last_kind == "image" {
                    if let Ok(last_bytes) = row.get::<_, Vec<u8>>(1) {
                        if last_bytes == png_bytes {
                            return Ok(None); // Exact consecutive duplicate, ignore
                        }
                    }
                }
            }
        }

        // 2. Remove any older duplicate image so the new copy moves to the top,
        // preserving any existing vault annotation/pin metadata.
        let existing = Self::existing_image_metadata(&tx, width, height, png_bytes)?;
        tx.execute(
            "DELETE FROM clipboard_history
             WHERE kind = 'image' AND width = ?1 AND height = ?2 AND image_data = ?3",
            params![width, height, png_bytes],
        )?;

        let (label, notes, is_pinned) = existing.unwrap_or((None, None, false));
        tx.execute(
            "INSERT INTO clipboard_history (kind, content, image_data, width, height, created_at, label, notes, is_pinned)
             VALUES ('image', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                content,
                png_bytes,
                width,
                height,
                now,
                label,
                notes,
                i64::from(is_pinned)
            ],
        )?;

        let new_id = tx.last_insert_rowid();

        // 3. Automatically prune older entries beyond max_history
        Self::prune_in_tx(&tx, max_limit, preserve)?;
        tx.commit()?;

        Ok(Some(new_id))
    }
}

struct RawRecentRow {
    id: i64,
    kind_str: String,
    content: String,
    width: Option<u32>,
    height: Option<u32>,
    created_at: String,
    label: Option<String>,
    notes: Option<String>,
    is_pinned: bool,
}

impl Database {
    /// Queries raw recent rows from SQLite holding the connection lock only during row extraction.
    fn fetch_raw_recent_rows(
        &self,
        limit: usize,
        filter: Option<&str>,
    ) -> Result<Vec<RawRecentRow>, rusqlite::Error> {
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let search_pattern = filter.filter(|s| !s.trim().is_empty()).map(|search| {
            let escaped = search
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            format!("%{escaped}%")
        });

        let conn = self.lock_conn();
        let mut raw_rows = Vec::new();
        let extract_row = |row: &rusqlite::Row| -> Result<RawRecentRow, rusqlite::Error> {
            Ok(RawRecentRow {
                id: row.get(0)?,
                kind_str: row.get(1).unwrap_or_else(|_| "text".to_string()),
                content: row.get(2)?,
                width: row.get(3)?,
                height: row.get(4)?,
                created_at: row.get(5)?,
                label: row.get(6)?,
                notes: row.get(7)?,
                is_pinned: row.get::<_, i64>(8).unwrap_or(0) != 0,
            })
        };

        if let Some(pattern) = search_pattern {
            let mut stmt = conn.prepare(
                "SELECT id, kind, content, width, height, created_at, label, notes, is_pinned
                 FROM clipboard_history
                 WHERE (
                     CASE WHEN kind = 'secret' THEN
                         (COALESCE(label, '') LIKE ?1 ESCAPE '\\' OR COALESCE(notes, '') LIKE ?1 ESCAPE '\\')
                     ELSE
                         (content LIKE ?1 ESCAPE '\\' OR COALESCE(label, '') LIKE ?1 ESCAPE '\\' OR COALESCE(notes, '') LIKE ?1 ESCAPE '\\')
                     END
                 )
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![pattern, limit_i64])?;
            while let Some(row) = rows.next()? {
                raw_rows.push(extract_row(row)?);
            }
        } else {
            let preserve = self.0.preserve_secrets_on_prune.load(Ordering::Relaxed);
            if preserve {
                let mut stmt = conn.prepare(
                    "SELECT id, kind, content, width, height, created_at, label, notes, is_pinned
                     FROM clipboard_history
                     WHERE is_pinned = 1
                        OR kind = 'secret'
                        OR id IN (
                            SELECT id FROM clipboard_history
                            WHERE is_pinned = 0 AND kind != 'secret'
                            ORDER BY id DESC LIMIT ?1
                        )
                     ORDER BY id DESC",
                )?;
                let mut rows = stmt.query(params![limit_i64])?;
                while let Some(row) = rows.next()? {
                    raw_rows.push(extract_row(row)?);
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, kind, content, width, height, created_at, label, notes, is_pinned
                     FROM clipboard_history
                     WHERE is_pinned = 1
                        OR id IN (
                            SELECT id FROM clipboard_history
                            WHERE is_pinned = 0
                            ORDER BY id DESC LIMIT ?1
                        )
                     ORDER BY id DESC",
                )?;
                let mut rows = stmt.query(params![limit_i64])?;
                while let Some(row) = rows.next()? {
                    raw_rows.push(extract_row(row)?);
                }
            }
        }
        Ok(raw_rows)
    }

    /// Classifies raw SQLite rows into typed `ClipboardItem`s outside the database lock.
    fn classify_raw_rows(raw_rows: Vec<RawRecentRow>) -> Vec<ClipboardItem> {
        raw_rows
            .into_iter()
            .map(|r| {
                let kind = match r.kind_str.as_str() {
                    "image" => ItemKind::Image,
                    "url" => ItemKind::Url,
                    "code" => ItemKind::Code,
                    "color" => ItemKind::Color,
                    "secret" => ItemKind::Secret,
                    _ => {
                        if is_valid_url(&r.content) {
                            ItemKind::Url
                        } else if crate::color::parse_color(&r.content).is_some() {
                            ItemKind::Color
                        } else if crate::code::detect_code_language(&r.content).is_some() {
                            ItemKind::Code
                        } else {
                            ItemKind::Text
                        }
                    }
                };
                let embedded_colors = if kind == ItemKind::Text || kind == ItemKind::Code {
                    crate::color::extract_colors(&r.content)
                } else if kind == ItemKind::Color {
                    crate::color::parse_color(&r.content).into_iter().collect()
                } else {
                    Vec::new()
                };
                let label = r.label.and_then(|s| {
                    let trimmed = s.trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                });
                let notes = r.notes.and_then(|s| {
                    let trimmed = s.trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                });
                ClipboardItem {
                    id: r.id,
                    kind,
                    content: r.content,
                    width: r.width,
                    height: r.height,
                    created_at: r.created_at,
                    embedded_colors,
                    label,
                    notes,
                    is_pinned: r.is_pinned,
                }
            })
            .collect()
    }

    /// Fetches the most recent clipboard entries, optionally filtered by search text.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the query or row extraction fails.
    pub(crate) fn get_recent(
        &self,
        limit: usize,
        filter: Option<&str>,
    ) -> Result<Vec<ClipboardItem>, rusqlite::Error> {
        let raw_rows = self.fetch_raw_recent_rows(limit, filter)?;
        Ok(Self::classify_raw_rows(raw_rows))
    }

    /// Loads the PNG bytes for a single image entry only when the UI needs them.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the query fails; a missing id or
    /// non-image row yields `Ok(None)`.
    pub(crate) fn get_image_data(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.lock_conn();
        match conn.query_row(
            "SELECT image_data FROM clipboard_history WHERE id = ?1 AND kind = 'image'",
            params![id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        ) {
            Ok(data) => Ok(data),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Toggles whether a non-image history entry is classified as `secret`.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the lookup or `UPDATE` fails.
    pub(crate) fn set_item_secret(&self, id: i64, is_secret: bool) -> Result<(), rusqlite::Error> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare("SELECT kind, content FROM clipboard_history WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(());
        };
        let kind_str: String = row.get(0)?;
        if kind_str == "image" {
            return Ok(());
        }
        let content: String = row.get(1)?;
        drop(rows);
        drop(stmt);

        let new_kind = if is_secret {
            "secret"
        } else if is_valid_url(&content) {
            "url"
        } else if crate::color::parse_color(&content).is_some() {
            "color"
        } else if crate::code::detect_code_language(&content).is_some() {
            "code"
        } else {
            "text"
        };

        conn.execute(
            "UPDATE clipboard_history SET kind = ?1 WHERE id = ?2",
            params![new_kind, id],
        )?;
        Ok(())
    }

    /// Updates the user annotation (label and notes) for a history entry.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the `UPDATE` fails.
    pub(crate) fn set_item_annotation(
        &self,
        id: i64,
        label: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let label = normalize_annotation_field(label);
        let notes = normalize_annotation_field(notes);
        let conn = self.lock_conn();
        conn.execute(
            "UPDATE clipboard_history SET label = ?1, notes = ?2 WHERE id = ?3",
            params![label, notes, id],
        )?;
        Ok(())
    }

    /// Toggles the pinned/vault state of a history entry and returns the new state.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the lookup or `UPDATE` fails.
    pub(crate) fn toggle_item_pinned(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "UPDATE clipboard_history SET is_pinned = 1 - is_pinned WHERE id = ?1 RETURNING is_pinned",
        )?;
        let mut rows = stmt.query(params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(false);
        };
        let pinned: i64 = row.get(0).unwrap_or(0);
        Ok(pinned != 0)
    }

    /// Inserts a manually created secret/vault entry directly into the encrypted database.
    /// This does not touch the OS clipboard. Returns the new row id.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the insert transaction fails.
    pub(crate) fn insert_manual_secret(
        &self,
        secret: &str,
        label: Option<&str>,
        notes: Option<&str>,
        is_pinned: bool,
    ) -> Result<i64, rusqlite::Error> {
        let label = normalize_annotation_field(label);
        let notes = normalize_annotation_field(notes);
        let (now, max_limit, preserve) = self.insert_context();

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        if let Some((last_kind, last_content)) = Self::last_text_entry(&tx)? {
            if last_kind == "secret" && last_content == secret {
                // Promote existing identical secret instead of creating a duplicate row.
                tx.execute(
                    "UPDATE clipboard_history
                     SET created_at = ?1, label = COALESCE(?2, label), notes = COALESCE(?3, notes), is_pinned = MAX(is_pinned, ?4)
                     WHERE id = (SELECT id FROM clipboard_history WHERE kind = 'secret' AND content = ?5 ORDER BY id DESC LIMIT 1)",
                    params![now, label, notes, i64::from(is_pinned), secret],
                )?;
                let id: i64 = tx.query_row(
                    "SELECT id FROM clipboard_history WHERE kind = 'secret' AND content = ?1 ORDER BY id DESC LIMIT 1",
                    params![secret],
                    |row| row.get(0),
                )?;
                Self::prune_in_tx(&tx, max_limit, preserve)?;
                tx.commit()?;
                return Ok(id);
            }
        }

        let existing = Self::existing_secret_metadata(&tx, secret)?;
        tx.execute(
            "DELETE FROM clipboard_history WHERE content = ?1 AND kind = 'secret'",
            params![secret],
        )?;

        let (existing_label, existing_notes, existing_pinned) =
            existing.unwrap_or((None, None, false));
        let merged_label = label.or(existing_label);
        let merged_notes = notes.or(existing_notes);
        let merged_pinned = is_pinned || existing_pinned;
        tx.execute(
            "INSERT INTO clipboard_history (kind, content, created_at, label, notes, is_pinned)
             VALUES ('secret', ?1, ?2, ?3, ?4, ?5)",
            params![
                secret,
                now,
                merged_label,
                merged_notes,
                i64::from(merged_pinned)
            ],
        )?;
        let new_id = tx.last_insert_rowid();
        Self::prune_in_tx(&tx, max_limit, preserve)?;
        tx.commit()?;
        Ok(new_id)
    }

    /// Deletes a specific entry by its primary key ID.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the `DELETE` fails.
    pub(crate) fn delete(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.lock_conn();
        conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Deletes non-pinned history and then securely compacts the database and WAL
    /// so previously deleted secrets do not linger on disk. Pinned vault items
    /// are always preserved.
    ///
    /// # Errors
    /// Returns [`ClearError::Delete`] if row deletion fails, or
    /// [`ClearError::Compaction`] if rows were removed but `VACUUM` or WAL
    /// truncation failed. Callers must not report success for either case.
    pub(crate) fn clear(&self) -> Result<(), ClearError> {
        let conn = self.lock_conn();
        conn.execute("DELETE FROM clipboard_history WHERE is_pinned = 0", [])
            .map_err(|error| ClearError::Delete(Box::new(error)))?;

        let vacuum_error = conn.execute_batch("VACUUM;").err().map(Box::new);
        let wal_checkpoint_error = conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .err()
            .map(Box::new);

        if let Some(error) = &vacuum_error {
            crate::logging::log_message(
                "ERROR",
                &format!("Failed to VACUUM database after clear: {error}"),
            );
        }
        if let Some(error) = &wal_checkpoint_error {
            crate::logging::log_message(
                "ERROR",
                &format!("Failed to truncate WAL after clear: {error}"),
            );
        }

        if vacuum_error.is_some() || wal_checkpoint_error.is_some() {
            return Err(ClearError::Compaction {
                vacuum: vacuum_error,
                wal_checkpoint: wal_checkpoint_error,
            });
        }
        Ok(())
    }

    /// Fetches a setting value by key from the database.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the query or row extraction fails.
    pub(crate) fn get_setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Stores or updates a setting value by key.
    ///
    /// # Errors
    /// Returns the underlying SQLite error if the upsert fails.
    pub(crate) fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let conn = self.lock_conn();
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

    fn test_hex_key(seed: u64) -> String {
        format!("{seed:016x}").repeat(4)
    }

    #[test]
    fn test_open_and_insert() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(&db_path, &test_hex_key(1)).expect("Failed to open encrypted db");

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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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

        let loaded = db.get_image_data(items[0].id).unwrap();
        assert_eq!(loaded.as_deref(), Some(dummy_png.as_slice()));
        assert_eq!(db.get_image_data(999).unwrap(), None);
    }

    #[test]
    fn test_deduplication() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("dedup.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let first = db.insert("Duplicate content").unwrap();
        assert!(first.is_some());

        let second = db.insert("Duplicate content").unwrap();
        assert_eq!(second, None, "Consecutive identical insert must be skipped");

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);

        // Alternating deduplication: A -> B -> A promotes A to the top
        let id_a1 = db.insert("Alpha").unwrap().unwrap();
        let _id_b = db.insert("Beta").unwrap().unwrap();
        let id_a2 = db.insert("Alpha").unwrap().unwrap();
        assert_ne!(id_a1, id_a2, "Re-copied item should receive new highest id");

        let items_alt = db.get_recent(10, None).unwrap();
        // Should only have "Alpha", "Beta", and "Duplicate content" (3 items total, no duplicate Alpha)
        assert_eq!(items_alt.len(), 3);
        assert_eq!(items_alt[0].content, "Alpha");
        assert_eq!(items_alt[1].content, "Beta");
        assert_eq!(items_alt[2].content, "Duplicate content");
    }

    #[test]
    fn test_history_auto_pruning() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("prune.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        // Enforce max history of 3 items
        db.set_max_history(3);
        assert_eq!(db.max_history(), 3);

        db.insert("Item 1").unwrap();
        db.insert("Item 2").unwrap();
        db.insert("Item 3").unwrap();
        db.insert("Item 4").unwrap();
        db.insert("Item 5").unwrap();

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 3, "Database must prune beyond max_history");
        assert_eq!(items[0].content, "Item 5");
        assert_eq!(items[1].content, "Item 4");
        assert_eq!(items[2].content, "Item 3");

        // Explicit manual prune to 2 items
        let purged = db.prune_history(2).unwrap();
        assert_eq!(purged, 1);
        let items_pruned = db.get_recent(10, None).unwrap();
        assert_eq!(items_pruned.len(), 2);
        assert_eq!(items_pruned[0].content, "Item 5");
        assert_eq!(items_pruned[1].content, "Item 4");

        // Vacuum database test
        db.vacuum().unwrap();
    }

    #[test]
    fn test_preserve_secrets_on_prune() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("preserve_secrets.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        // Enforce max history of 2 items
        db.set_max_history(2);
        db.set_preserve_secrets_on_prune(true);
        assert!(db.preserve_secrets_on_prune());

        // Insert a secret
        db.insert_secret("MySecretPassword123").unwrap();

        // Insert 3 regular text items
        db.insert("Regular 1").unwrap();
        db.insert("Regular 2").unwrap();
        db.insert("Regular 3").unwrap();

        // With preserve_secrets_on_prune = true:
        // Non-secret items are pruned to max_history (2 items: Regular 3 and Regular 2),
        // but the secret is preserved!
        let items = db.get_recent(2, None).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].content, "Regular 3");
        assert_eq!(items[1].content, "Regular 2");
        assert_eq!(items[2].content, "MySecretPassword123");
        assert_eq!(items[2].kind, ItemKind::Secret);

        // Explicit manual prune to 1
        let purged = db.prune_history(1).unwrap();
        assert_eq!(purged, 1); // Regular 2 pruned
        let items_after = db.get_recent(1, None).unwrap();
        assert_eq!(items_after.len(), 2);
        assert_eq!(items_after[0].content, "Regular 3");
        assert_eq!(items_after[1].content, "MySecretPassword123");

        // Now disable preserve_secrets_on_prune and prune to 1
        db.set_preserve_secrets_on_prune(false);
        assert!(!db.preserve_secrets_on_prune());
        db.prune_history(1).unwrap();
        let items_disabled = db.get_recent(10, None).unwrap();
        assert_eq!(items_disabled.len(), 1);
        assert_eq!(items_disabled[0].content, "Regular 3");
    }

    #[test]
    fn test_search_filter() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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
            let db = Database::open(&db_path, &test_hex_key(1)).unwrap();
            db.insert("Secret message").unwrap();
        }

        // 2. Open with wrong key - must fail verification
        let wrong_key = test_hex_key(2);
        let result = Database::open(&db_path, &wrong_key);
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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

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

    #[test]
    fn test_insert_secret_and_toggle_secret() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("secret_test.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let secret = format!("fixture-{:08x}", 0x005E_C3E7);
        let id = db
            .insert_secret(&secret)
            .unwrap()
            .expect("Should insert secret");
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Secret);

        // Consecutive duplicate secret is ignored
        assert_eq!(db.insert_secret(&secret).unwrap(), None);

        // Unmarking secret restores auto-detected kind (text)
        db.set_item_secret(id, false).unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items[0].kind, ItemKind::Text);

        // Marking back as secret
        db.set_item_secret(id, true).unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items[0].kind, ItemKind::Secret);
    }

    #[test]
    fn test_auto_detect_secrets_on_insert() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auto_detect_secret_test.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let token = format!("ghp_{}", "a1".repeat(20));
        db.insert(&token).unwrap().expect("Should insert token");
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items[0].kind, ItemKind::Secret);

        // When disabled, a newly inserted token is not classified as secret
        db.set_auto_detect_secrets(false);
        let token2 = format!("{}-proj-{}", "sk", "fixture".repeat(5));
        db.insert(&token2).unwrap().expect("Should insert token2");
        let items = db.get_recent(10, None).unwrap();
        assert_ne!(items[0].kind, ItemKind::Secret);
    }

    #[test]
    fn test_secure_delete_and_wal_truncate_on_clear() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("secure_delete_test.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let secure_delete: i64 = db
            .lock_conn()
            .query_row("PRAGMA secure_delete;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(secure_delete, 1, "PRAGMA secure_delete must be ON");

        db.insert_secret(&format!("fixture-{:08x}", 0x0000_C1EA))
            .unwrap();
        db.clear().unwrap();

        let wal_path = dir.path().join("secure_delete_test.db-wal");
        if let Ok(meta) = std::fs::metadata(&wal_path) {
            assert_eq!(
                meta.len(),
                0,
                "WAL file must be truncated to 0 bytes after clear()"
            );
        }
    }

    #[test]
    fn test_clear_reports_delete_failure() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("clear_delete_failure.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();
        db.insert("fixture entry").unwrap();

        db.lock_conn()
            .pragma_update(None, "query_only", "ON")
            .unwrap();

        assert!(matches!(db.clear(), Err(ClearError::Delete(_))));
    }

    #[test]
    fn test_clear_reports_compaction_failure_after_deleting_rows() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("clear_compaction_failure.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();
        db.insert("fixture entry").unwrap();

        // An open transaction on the same connection makes VACUUM fail after
        // the DELETE succeeds, exercising the compaction error path.
        db.lock_conn().execute_batch("BEGIN").unwrap();
        let result = db.clear();
        db.lock_conn().execute_batch("ROLLBACK").unwrap();

        assert!(matches!(
            result,
            Err(ClearError::Compaction {
                vacuum: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn test_search_escapes_wildcards_and_excludes_secrets() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search_security_test.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        db.insert("100% pure rust").unwrap();
        db.insert("snake_case_name").unwrap();
        db.insert("plain hello world").unwrap();
        let marker = format!("{}-{}", "top", "secret");
        db.insert_secret(&format!("my-{marker}-password-999"))
            .unwrap();

        // '%' must match literally, not act as a wildcard matching all rows
        let pct_hits = db.get_recent(10, Some("%")).unwrap();
        assert_eq!(pct_hits.len(), 1);
        assert_eq!(pct_hits[0].content, "100% pure rust");

        // '_' must match literally, not act as a single-char wildcard
        let underscore_hits = db.get_recent(10, Some("_")).unwrap();
        assert_eq!(underscore_hits.len(), 1);
        assert_eq!(underscore_hits[0].content, "snake_case_name");

        // Masked secrets must not be probeable via search
        let secret_hits = db.get_recent(10, Some(marker.as_str())).unwrap();
        assert!(secret_hits.is_empty());

        // Unfiltered query still returns the secret (so it appears in History/Secrets tab)
        let all = db.get_recent(10, None).unwrap();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_prune_in_tx_failure_rolls_back_insert() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("prune_failure_rollback.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();
        db.set_max_history(2);

        // Populate two baseline entries
        let id1 = db
            .insert("initial entry 1")
            .unwrap()
            .expect("insert initial 1");
        let id2 = db
            .insert("initial entry 2")
            .unwrap()
            .expect("insert initial 2");

        // Install a trigger that aborts any DELETE when clipboard_history exceeds max_limit (2),
        // cleanly isolating the prune DELETE from deduplication DELETEs.
        db.lock_conn()
            .execute_batch(
                "CREATE TRIGGER abort_prune BEFORE DELETE ON clipboard_history
                 WHEN (SELECT count(*) FROM clipboard_history) > 2
                 BEGIN
                     SELECT RAISE(ABORT, 'forced prune failure');
                 END;",
            )
            .unwrap();

        // 1. insert() path: prune fails, returning Err and rolling back insert
        let insert_res = db.insert("entry 3 - should fail");
        assert!(
            insert_res.is_err(),
            "insert must return Err when prune fails"
        );

        let items = db.get_recent(10, None).unwrap();
        assert_eq!(
            items.len(),
            2,
            "Transaction must rollback, leaving exactly 2 items"
        );
        assert_eq!(items[0].id, id2);
        assert_eq!(items[1].id, id1);

        // 2. insert_secret() path: prune fails, returning Err and rolling back secret insert
        let secret_res = db.insert_secret("secret 3 - should fail");
        assert!(
            secret_res.is_err(),
            "insert_secret must return Err when prune fails"
        );

        let items_after_secret = db.get_recent(10, None).unwrap();
        assert_eq!(
            items_after_secret.len(),
            2,
            "Transaction must rollback secret insert"
        );
        assert_eq!(items_after_secret[0].id, id2);
        assert_eq!(items_after_secret[1].id, id1);

        // 3. insert_image() path: prune fails, returning Err and rolling back image insert
        let image_res = db.insert_image(10, 10, &[0x89, b'P', b'N', b'G']);
        assert!(
            image_res.is_err(),
            "insert_image must return Err when prune fails"
        );

        let items_after_image = db.get_recent(10, None).unwrap();
        assert_eq!(
            items_after_image.len(),
            2,
            "Transaction must rollback image insert"
        );
        assert_eq!(items_after_image[0].id, id2);
        assert_eq!(items_after_image[1].id, id1);

        // 4. insert() with preserve_secrets_on_prune = true: also fails and rolls back
        db.set_preserve_secrets_on_prune(true);
        let insert_preserve_res = db.insert("entry 4 - should fail with preserve");
        assert!(
            insert_preserve_res.is_err(),
            "insert with preserve must return Err when prune fails"
        );

        let items_after_preserve = db.get_recent(10, None).unwrap();
        assert_eq!(items_after_preserve.len(), 2);
        assert_eq!(items_after_preserve[0].id, id2);
        assert_eq!(items_after_preserve[1].id, id1);
    }

    #[test]
    fn test_is_duplicate_column_error_classifier() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("dup_column_classifier.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (col1 TEXT);").unwrap();

        // Adding col1 again produces an actual duplicate column error
        let dup_err = conn
            .execute("ALTER TABLE t ADD COLUMN col1 TEXT", [])
            .unwrap_err();
        assert!(
            is_duplicate_column_error(&dup_err),
            "Expected duplicate column error to be classified as true"
        );

        // A syntax error or missing table error must NOT be classified as duplicate column error
        let syntax_err = conn.execute("INVALID SQL STATEMENT", []).unwrap_err();
        assert!(
            !is_duplicate_column_error(&syntax_err),
            "Syntax error must not be classified as duplicate column error"
        );

        let missing_table_err = conn
            .execute("ALTER TABLE nonexistent ADD COLUMN col1 TEXT", [])
            .unwrap_err();
        assert!(
            !is_duplicate_column_error(&missing_table_err),
            "Missing table error must not be classified as duplicate column error"
        );
    }

    #[test]
    fn test_schema_migration_unexpected_error_aborts_open() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("migration_failure.db");
        let key = test_hex_key(1);

        // Pre-create an encrypted database where clipboard_history already has the maximum
        // number of columns allowed by SQLite (2000).
        // During Database::open, the migration statement attempting to ALTER TABLE fails because
        // the table exceeds SQLite's column limit. Since this is not a duplicate column error,
        // it exercises the unexpected migration failure abort path.
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.pragma_update(None, "key", &key).unwrap();
            let mut cols = String::with_capacity(2000 * 8);
            for i in 0..1998 {
                use std::fmt::Write as _;
                let _ = write!(cols, ", c{i} INT");
            }
            conn.execute_batch(&format!(
                "CREATE TABLE clipboard_history (id INTEGER PRIMARY KEY, kind TEXT{cols});"
            ))
            .unwrap();
        }

        let open_res = Database::open(&db_path, &key);
        assert!(
            open_res.is_err(),
            "Database::open must fail when schema migration encounters an unexpected error"
        );

        let err = open_res.err().unwrap();
        assert!(matches!(err, DbOpenError::Migration { .. }));
        assert!(std::error::Error::source(&err).is_some());
        let err_msg = err.to_string();
        assert!(
            err_msg.contains(
                "Schema migration failed for `ALTER TABLE clipboard_history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'`"
            ),
            "Error message must provide migration context with failing SQL statement, got: {err_msg}"
        );
    }

    #[test]
    fn test_database_apply_settings() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("apply_settings.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let settings = crate::settings::AppSettings {
            max_history: 42,
            ignore_concealed: false,
            auto_detect_secrets: false,
            preserve_secrets_on_prune: true,
            ..Default::default()
        };

        db.apply_settings(&settings);

        assert_eq!(db.max_history(), 42);
        assert!(!db.ignore_concealed());
        assert!(!db.auto_detect_secrets());
        assert!(db.preserve_secrets_on_prune());
    }

    #[test]
    fn test_clipboard_item_precomputed_embedded_colors() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("embedded_colors.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        db.insert("Theme palette: primary #ff5733 and secondary rgb(0, 128, 255)")
            .unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        let embedded = items[0].embedded_colors();
        assert_eq!(embedded.len(), 2);
        assert_eq!(embedded[0].to_hex(), "#FF5733");
    }

    #[test]
    fn test_schema_migration_adds_annotation_columns() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("annotation_migration.db");
        let key = test_hex_key(1);

        {
            let conn = Connection::open(&db_path).unwrap();
            conn.pragma_update(None, "key", &key).unwrap();
            conn.execute_batch(
                "CREATE TABLE clipboard_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL DEFAULT 'text',
                    content TEXT NOT NULL,
                    image_data BLOB,
                    width INTEGER,
                    height INTEGER,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO clipboard_history (kind, content, created_at) VALUES ('text', 'old', '2026-01-01 00:00:00');",
            )
            .unwrap();
        }

        let db = Database::open(&db_path, &key).unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, None);
        assert_eq!(items[0].notes, None);
        assert!(!items[0].is_pinned);
    }

    #[test]
    fn test_set_item_annotation_and_search() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("annotation_search.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let id = db.insert("plain content").unwrap().unwrap();
        db.set_item_annotation(id, Some("GitHub PAT"), Some("user: dev@company.com"))
            .unwrap();

        let items = db.get_recent(10, Some("github")).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label.as_deref(), Some("GitHub PAT"));
        assert_eq!(items[0].notes.as_deref(), Some("user: dev@company.com"));

        let notes_match = db.get_recent(10, Some("dev@company")).unwrap();
        assert_eq!(notes_match.len(), 1);
    }

    #[test]
    fn test_secret_search_matches_only_annotations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("secret_annotation_search.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let secret = "super-secret-token-value";
        let id = db.insert_secret(secret).unwrap().unwrap();
        db.set_item_annotation(id, Some("AWS Root"), Some("prod account"))
            .unwrap();

        let by_label = db.get_recent(10, Some("aws")).unwrap();
        assert_eq!(by_label.len(), 1);
        assert_eq!(by_label[0].id, id);

        let by_notes = db.get_recent(10, Some("prod")).unwrap();
        assert_eq!(by_notes.len(), 1);

        let by_content = db.get_recent(10, Some("super-secret-token")).unwrap();
        assert!(
            by_content.is_empty(),
            "secret content must not be searchable"
        );
    }

    #[test]
    fn test_pinned_items_exempt_from_prune_and_clear() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("pinned_prune_clear.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        db.set_max_history(2);
        let pinned_id = db.insert("Pinned item").unwrap().unwrap();
        assert!(db.toggle_item_pinned(pinned_id).unwrap());

        db.insert("Item 2").unwrap();
        db.insert("Item 3").unwrap();
        db.insert("Item 4").unwrap();

        let items = db.get_recent(10, None).unwrap();
        assert!(items.iter().any(|i| i.id == pinned_id));
        assert!(items.iter().filter(|i| !i.is_pinned).count() <= 2);

        db.clear().unwrap();
        let after_clear = db.get_recent(10, None).unwrap();
        assert_eq!(after_clear.len(), 1);
        assert_eq!(after_clear[0].id, pinned_id);
        assert!(after_clear[0].is_pinned);
    }

    #[test]
    fn test_insert_manual_secret() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("manual_secret.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let id = db
            .insert_manual_secret("s3cret", Some("VPN"), Some("user: admin"), true)
            .unwrap();
        let items = db.get_recent(10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].kind, ItemKind::Secret);
        assert_eq!(items[0].label.as_deref(), Some("VPN"));
        assert_eq!(items[0].notes.as_deref(), Some("user: admin"));
        assert!(items[0].is_pinned);
    }

    #[test]
    fn test_recopy_pinned_annotated_text_preserves_metadata() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("recopy_text.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let id = db.insert("keep me").unwrap().unwrap();
        db.set_item_annotation(id, Some("Label"), Some("Notes"))
            .unwrap();
        assert!(db.toggle_item_pinned(id).unwrap());

        db.insert("other").unwrap();
        let new_id = db.insert("keep me").unwrap().unwrap();
        assert_ne!(id, new_id);

        let items = db.get_recent(10, None).unwrap();
        let item = items.iter().find(|i| i.content == "keep me").unwrap();
        assert_eq!(item.id, new_id);
        assert_eq!(item.label.as_deref(), Some("Label"));
        assert_eq!(item.notes.as_deref(), Some("Notes"));
        assert!(item.is_pinned);
    }

    #[test]
    fn test_recopy_pinned_annotated_secret_preserves_metadata() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("recopy_secret.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let id = db.insert_secret("api-key").unwrap().unwrap();
        db.set_item_annotation(id, Some("API"), Some("prod"))
            .unwrap();
        assert!(db.toggle_item_pinned(id).unwrap());

        db.insert("noise").unwrap();
        let new_id = db.insert_secret("api-key").unwrap().unwrap();
        assert_ne!(id, new_id);

        let items = db.get_recent(10, None).unwrap();
        let item = items
            .iter()
            .find(|i| i.kind == ItemKind::Secret && i.content == "api-key")
            .unwrap();
        assert_eq!(item.id, new_id);
        assert_eq!(item.label.as_deref(), Some("API"));
        assert_eq!(item.notes.as_deref(), Some("prod"));
        assert!(item.is_pinned);
    }

    #[test]
    fn test_recopy_pinned_annotated_image_preserves_metadata() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("recopy_image.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let png = vec![1_u8, 2, 3, 4];
        let id = db.insert_image(2, 2, &png).unwrap().unwrap();
        db.set_item_annotation(id, Some("Shot"), Some("before"))
            .unwrap();
        assert!(db.toggle_item_pinned(id).unwrap());

        db.insert("noise").unwrap();
        let new_id = db.insert_image(2, 2, &png).unwrap().unwrap();
        assert_ne!(id, new_id);

        let items = db.get_recent(10, None).unwrap();
        let item = items.iter().find(|i| i.kind == ItemKind::Image).unwrap();
        assert_eq!(item.id, new_id);
        assert_eq!(item.label.as_deref(), Some("Shot"));
        assert_eq!(item.notes.as_deref(), Some("before"));
        assert!(item.is_pinned);
    }

    #[test]
    fn test_insert_manual_secret_merges_older_duplicate_metadata() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("manual_merge.db");
        let db = Database::open(&db_path, &test_hex_key(1)).unwrap();

        let id = db.insert_secret("dup-secret").unwrap().unwrap();
        db.set_item_annotation(id, Some("Old"), Some("keep"))
            .unwrap();
        assert!(db.toggle_item_pinned(id).unwrap());
        db.insert("noise").unwrap();

        let new_id = db
            .insert_manual_secret("dup-secret", Some("New"), None, false)
            .unwrap();
        assert_ne!(id, new_id);

        let items = db.get_recent(10, None).unwrap();
        let item = items
            .iter()
            .find(|i| i.kind == ItemKind::Secret && i.content == "dup-secret")
            .unwrap();
        assert_eq!(item.id, new_id);
        assert_eq!(item.label.as_deref(), Some("New"));
        assert_eq!(item.notes.as_deref(), Some("keep"));
        assert!(item.is_pinned);
    }
}
