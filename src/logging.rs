//! Safe, panic-resilient, size-capped application log writer (`app.log`).

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};

const MAX_LOG_BYTES: u64 = 1024 * 1024; // Rotate app.log once it reaches 1 MiB

static LOG_LOCK: Mutex<()> = Mutex::new(());
static LOG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

pub(crate) fn log_message(level: &str, message: &str) {
    let dir = LOG_DIR.get_or_init(|| {
        let dir = dirs::data_local_dir()?.join("cipherclipboard");
        let _ = std::fs::create_dir_all(&dir);
        crate::crypto::set_restricted_dir_permissions(&dir);
        Some(dir)
    });
    let Some(dir) = dir.as_ref() else {
        return;
    };

    // Guard rotation and appends against concurrent writers.
    // Inside a panic hook, use try_lock() so we never self-deadlock if the
    // panicking thread was already holding LOG_LOCK.
    let _guard = if std::thread::panicking() {
        match LOG_LOCK.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        }
    } else {
        LOG_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    };

    let path = dir.join("app.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() >= MAX_LOG_BYTES {
            let backup = dir.join("app.log.old");
            let _ = std::fs::remove_file(&backup);
            let _ = std::fs::rename(&path, &backup);
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    if let Ok(mut file) = opts.open(&path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{ts}] [{level}] {message}");
    }
}
