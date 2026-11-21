//! Safe, panic-resilient, size-capped application log writer (`app.log`).

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};

const MAX_LOG_BYTES: u64 = 1024 * 1024; // Rotate app.log once it reaches 1 MiB

static LOG_LOCK: Mutex<()> = Mutex::new(());
static LOG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOG_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();

pub(crate) fn log_dir() -> Option<&'static std::path::Path> {
    LOG_DIR
        .get_or_init(|| {
            let dir = dirs::data_local_dir()?.join("cipherclipboard");
            let _ = std::fs::create_dir_all(&dir);
            crate::crypto::set_restricted_dir_permissions(&dir);
            Some(dir)
        })
        .as_deref()
}

pub(crate) fn log_file_path() -> Option<&'static std::path::Path> {
    LOG_FILE
        .get_or_init(|| log_dir().map(|d| d.join("app.log")))
        .as_deref()
}

pub(crate) fn log_message(level: &str, message: &str) {
    // Unit tests intentionally exercise failure paths (forced VACUUM failures,
    // poisoned-mutex recovery). Writing those diagnostics to the user's real
    // app.log makes production logs look corrupt, so keep test runs silent.
    if cfg!(test) {
        return;
    }

    let Some(dir) = log_dir() else {
        return;
    };
    let Some(path) = log_file_path() else {
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

    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= MAX_LOG_BYTES {
            let backup = dir.join("app.log.old");
            let _ = std::fs::remove_file(&backup);
            let _ = std::fs::rename(path, &backup);
        }
    }
    if let Ok(mut file) = crate::platform::fs_security::open_restricted_append_file(path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{ts}] [{level}] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_file_path_ends_with_app_log() {
        if let Some(path) = log_file_path() {
            assert!(path.ends_with("app.log"));
        }
    }

    #[test]
    fn test_log_dir_ends_with_app_name() {
        if let Some(dir) = log_dir() {
            assert!(dir.ends_with("cipherclipboard"));
        }
    }
}
