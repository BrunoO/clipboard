#[cfg(not(test))]
use keyring::Entry;
use rand::Rng;
use std::fs;
use std::path::Path;
use zeroize::{Zeroize, Zeroizing};

const SERVICE_NAME: &str = "cipherclipboard";
const LEGACY_SERVICE_NAME: &str = "rusty_clipboard";
const KEY_NAME: &str = "db_encryption_key";

/// Indicates where the active database encryption key is currently stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyStorageMode {
    /// Key is stored securely in the OS Keyring (Keychain / Credential Manager / Secret Service).
    OsKeyring,
    /// Key is stored in a restricted local `.encryption_key` file (debug cache or keyring unavailable).
    LocalFileFallback,
    /// Key is stored locally, but OS permission/ACL hardening for the file failed.
    LocalFileFallbackDegraded,
}

const fn local_fallback_mode(permissions_degraded: bool) -> KeyStorageMode {
    if permissions_degraded {
        KeyStorageMode::LocalFileFallbackDegraded
    } else {
        KeyStorageMode::LocalFileFallback
    }
}

fn warn_if_local_cache_hardening_failed(permissions_degraded: bool) {
    if permissions_degraded {
        crate::logging::log_message(
            "WARN",
            "Debug local key-file cache was created but its permissions could not be restricted; the active key remains in the OS keyring.",
        );
    }
}

/// Domain error type for encryption key retrieval and storage operations.
#[derive(Debug)]
pub(crate) enum KeyError {
    /// I/O error reading or writing the local key file.
    Io(std::io::Error),
    /// An existing encrypted database was found on disk, but its encryption key could not be recovered.
    ExistingDatabaseMissingKey(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "Key store I/O error: {err}"),
            Self::ExistingDatabaseMissingKey(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for KeyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::ExistingDatabaseMissingKey(_) => None,
        }
    }
}

impl From<std::io::Error> for KeyError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Retrieves or securely generates a 256-bit encryption key wrapped in [`Zeroizing`],
/// along with the [`KeyStorageMode`] indicating whether it is backed by the OS Keyring
/// or a local fallback file.
///
/// Strategy:
/// - **Debug builds** (`cfg!(debug_assertions)`): Checks `data_dir/.encryption_key` first so
///   recompiled debug binaries don't trigger repeated macOS Keychain ACL prompts, then falls back
///   to the OS Keyring.
/// - **Release builds** (`!cfg!(debug_assertions)`): Uses the **OS Keyring exclusively** (macOS
///   Keychain, Windows Credential Manager, Linux Secret Service). Any legacy `.encryption_key`
///   file is migrated into the OS Keyring and securely removed from disk. Falls back to the
///   restricted local key file only if the OS Keyring is unavailable (e.g., headless Linux).
///
/// # Errors
/// Returns [`KeyError`] if a key file cannot be read or written with restricted permissions,
/// or if an existing encrypted database is found but its key cannot be recovered.
pub(crate) fn get_or_create_encryption_key(
    data_dir: &Path,
    db_exists: bool,
) -> Result<(Zeroizing<String>, KeyStorageMode), KeyError> {
    get_or_create_encryption_key_with_mode(data_dir, db_exists, cfg!(debug_assertions))
}

fn get_or_create_encryption_key_with_mode(
    data_dir: &Path,
    db_exists: bool,
    allow_key_file_cache: bool,
) -> Result<(Zeroizing<String>, KeyStorageMode), KeyError> {
    let key_file = data_dir.join(".encryption_key");

    if allow_key_file_cache {
        // Debug mode: prefer local key file to avoid Keychain prompts on every recompile.
        if let Some(key) = read_key_file(&key_file)? {
            let permissions_degraded =
                crate::platform::fs_security::set_restricted_permissions(&key_file);
            #[cfg(debug_assertions)]
            println!(
                "🔐 Loaded encryption key from local key store: {}",
                key_file.display()
            );
            return Ok((key, local_fallback_mode(permissions_degraded)));
        }

        if let Some((key, service)) = read_from_keyring() {
            #[cfg(debug_assertions)]
            println!("🔐 Loaded encryption key from OS Keyring ({service}).");
            #[cfg(not(debug_assertions))]
            let _ = service;
            let permissions_degraded = save_key_file(&key_file, &key)?;
            warn_if_local_cache_hardening_failed(permissions_degraded);
            return Ok((key, KeyStorageMode::OsKeyring));
        }
    } else {
        // Release mode: prefer OS Keyring exclusively and clean up any plaintext key file.
        if let Some((key, service)) = read_from_keyring() {
            #[cfg(debug_assertions)]
            println!("🔐 Loaded encryption key from OS Keyring ({service}).");
            if service != SERVICE_NAME {
                let _ = write_to_keyring(&key);
            }
            if key_file.exists() {
                remove_key_file(&key_file);
                #[cfg(debug_assertions)]
                println!("🧹 Removed legacy plaintext key file from disk.");
            }
            return Ok((key, KeyStorageMode::OsKeyring));
        }

        // Migration path: if an older version stored .encryption_key, move it into the OS Keyring.
        if let Some(key) = read_key_file(&key_file)? {
            if write_to_keyring(&key).is_ok() {
                remove_key_file(&key_file);
                #[cfg(debug_assertions)]
                println!("🔐 Migrated encryption key into OS Keyring and removed local key file.");
                return Ok((key, KeyStorageMode::OsKeyring));
            }
            let permissions_degraded =
                crate::platform::fs_security::set_restricted_permissions(&key_file);
            #[cfg(debug_assertions)]
            eprintln!(
                "ℹ️ OS Keyring unavailable; continuing with restricted local key file: {}",
                key_file.display()
            );
            return Ok((key, local_fallback_mode(permissions_degraded)));
        }
    }

    // Safety Check: If the database already exists on disk, we MUST NOT generate a new random key!
    if db_exists {
        return Err(KeyError::ExistingDatabaseMissingKey(
            missing_key_error_message(data_dir),
        ));
    }

    // Fresh setup: Generate a new random 256-bit key
    let new_key = generate_random_hex_key();
    #[cfg(debug_assertions)]
    println!("🔑 Generating new 256-bit encryption key for first-time setup...");

    let storage_mode = if allow_key_file_cache {
        let permissions_degraded = save_key_file(&key_file, &new_key)?;
        if let Err(e) = write_to_keyring(&new_key) {
            #[cfg(debug_assertions)]
            eprintln!(
                "ℹ️ Could not write key to OS Keyring ({e}); using local secure file instead."
            );
            #[cfg(not(debug_assertions))]
            let _ = e;
            local_fallback_mode(permissions_degraded)
        } else {
            warn_if_local_cache_hardening_failed(permissions_degraded);
            #[cfg(debug_assertions)]
            println!("🔑 Backed up encryption key to OS Keyring.");
            KeyStorageMode::OsKeyring
        }
    } else if let Err(e) = write_to_keyring(&new_key) {
        #[cfg(debug_assertions)]
        eprintln!(
            "ℹ️ Could not write key to OS Keyring ({e}); falling back to restricted local key file."
        );
        #[cfg(not(debug_assertions))]
        let _ = e;
        let permissions_degraded = save_key_file(&key_file, &new_key)?;
        local_fallback_mode(permissions_degraded)
    } else {
        #[cfg(debug_assertions)]
        println!("🔑 Stored encryption key in OS Keyring (Keyring-only release mode).");
        KeyStorageMode::OsKeyring
    };

    Ok((new_key, storage_mode))
}

fn missing_key_error_message(data_dir: &Path) -> String {
    crate::platform::fs_security::missing_key_error_message(data_dir)
}

fn read_key_file(path: &Path) -> Result<Option<Zeroizing<String>>, std::io::Error> {
    crate::platform::fs_security::read_protected_key_file(path)
}

#[cfg(not(test))]
fn read_from_keyring() -> Option<(Zeroizing<String>, &'static str)> {
    for service in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        if let Ok(entry) = Entry::new(service, KEY_NAME) {
            if let Ok(key) = entry.get_password() {
                let key = Zeroizing::new(key);
                let trimmed = Zeroizing::new(key.trim().to_string());
                if !trimmed.is_empty() {
                    return Some((trimmed, service));
                }
            }
        }
    }
    None
}

#[cfg(not(test))]
fn write_to_keyring(key: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE_NAME, KEY_NAME)?;
    entry.set_password(key)
}

#[cfg(test)]
pub(crate) mod mock_keyring {
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub(crate) struct MockKeyringState {
        pub(crate) entries: HashMap<(String, String), String>,
        pub(crate) fail_writes: bool,
    }

    pub(crate) static MOCK_KEYRING: Mutex<Option<MockKeyringState>> = Mutex::new(None);
    pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn with_state<F, R>(f: F) -> R
    where
        F: FnOnce(&mut MockKeyringState) -> R,
    {
        let mut guard = MOCK_KEYRING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(MockKeyringState::default());
        }
        f(guard.as_mut().unwrap())
    }

    pub(crate) fn reset() {
        let mut guard = MOCK_KEYRING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(MockKeyringState::default());
    }

    pub(crate) fn set_fail_writes(fail: bool) {
        with_state(|s| s.fail_writes = fail);
    }

    pub(crate) fn insert(service: &str, key_name: &str, password: &str) {
        with_state(|s| {
            s.entries.insert(
                (service.to_string(), key_name.to_string()),
                password.to_string(),
            );
        });
    }

    pub(crate) fn get(service: &str, key_name: &str) -> Option<String> {
        with_state(|s| {
            s.entries
                .get(&(service.to_string(), key_name.to_string()))
                .cloned()
        })
    }
}

#[cfg(test)]
fn read_from_keyring() -> Option<(Zeroizing<String>, &'static str)> {
    for service in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        if let Some(key) = mock_keyring::get(service, KEY_NAME) {
            let trimmed = Zeroizing::new(key.trim().to_string());
            if !trimmed.is_empty() {
                return Some((trimmed, service));
            }
        }
    }
    None
}

#[cfg(test)]
fn write_to_keyring(key: &str) -> Result<(), keyring::Error> {
    mock_keyring::with_state(|s| {
        if s.fail_writes {
            Err(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "mock keyring write failure",
                ),
            )))
        } else {
            s.entries.insert(
                (SERVICE_NAME.to_string(), KEY_NAME.to_string()),
                key.to_string(),
            );
            Ok(())
        }
    })
}

fn remove_key_file(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let zeros = vec![b'0'; usize::try_from(meta.len()).unwrap_or(64)];
        let _ = fs::write(path, zeros);
    }
    let _ = fs::remove_file(path);
}

/// Writes a local key file and reports whether OS permission hardening failed.
fn save_key_file(path: &Path, key: &str) -> Result<bool, std::io::Error> {
    crate::platform::fs_security::write_protected_key_file(path, key)
}

fn generate_random_hex_key() -> Zeroizing<String> {
    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let hex_key = Zeroizing::new(hex::encode(key_bytes));
    key_bytes.zeroize();
    hex_key
}

#[allow(clippy::missing_const_for_fn)]
pub(crate) fn set_restricted_dir_permissions(path: &Path) {
    crate::platform::fs_security::set_restricted_dir_permissions(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_hex_key(seed: u64) -> String {
        format!("{seed:016x}").repeat(4)
    }

    #[test]
    fn test_generate_random_hex_key() {
        let key = generate_random_hex_key();
        assert_eq!(key.len(), 64);
        assert!(hex::decode(&key).is_ok());
    }

    #[test]
    fn test_local_fallback_mode_reports_permission_degradation() {
        assert_eq!(
            local_fallback_mode(false),
            KeyStorageMode::LocalFileFallback
        );
        assert_eq!(
            local_fallback_mode(true),
            KeyStorageMode::LocalFileFallbackDegraded
        );
    }

    #[test]
    fn test_save_and_read_key_file() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = test_hex_key(0x0123_4567_89AB_CDEF);

        save_key_file(&key_file, &test_key).unwrap();
        assert!(key_file.exists());
        crate::platform::fs_security::assert_protected_key_file_on_disk(&key_file, &test_key);

        let (loaded, mode) = get_or_create_encryption_key(dir.path(), false).unwrap();
        assert_eq!(loaded.as_str(), test_key.as_str());
        assert!(matches!(
            mode,
            KeyStorageMode::LocalFileFallback | KeyStorageMode::LocalFileFallbackDegraded
        ));
    }

    #[test]
    fn test_remove_key_file_deletes_file() {
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let short_key = format!("fixture-{:x}", 0x0000_C0DE);
        save_key_file(&key_file, &short_key).unwrap();
        assert!(key_file.exists());

        remove_key_file(&key_file);
        assert!(!key_file.exists());
    }

    #[test]
    fn test_existing_db_without_key_fails_loudly() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let dir = tempdir().unwrap();
        let res = get_or_create_encryption_key_with_mode(dir.path(), true, true);
        assert!(matches!(res, Err(KeyError::ExistingDatabaseMissingKey(_))));
        if let Err(err) = res {
            assert!(err.to_string().contains("existing encrypted database"));
            assert!(std::error::Error::source(&err).is_none());
        }
    }

    #[test]
    fn test_release_mode_reads_legacy_key_file_when_keyring_unavailable() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        mock_keyring::set_fail_writes(true);
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = test_hex_key(0xFEDC_BA98_7654_3210);
        save_key_file(&key_file, &test_key).unwrap();

        let (loaded, mode) =
            get_or_create_encryption_key_with_mode(dir.path(), true, false).unwrap();
        assert_eq!(loaded.as_str(), test_key.as_str());
        assert!(matches!(
            mode,
            KeyStorageMode::LocalFileFallback | KeyStorageMode::LocalFileFallbackDegraded
        ));
        // Since write_to_keyring failed, the key_file should remain on disk
        assert!(key_file.exists());
    }

    #[test]
    fn test_release_mode_migrates_legacy_key_file_to_keyring() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = test_hex_key(0xFEDC_BA98_7654_3210);
        save_key_file(&key_file, &test_key).unwrap();

        let (loaded, mode) =
            get_or_create_encryption_key_with_mode(dir.path(), true, false).unwrap();
        assert_eq!(loaded.as_str(), test_key.as_str());
        assert_eq!(mode, KeyStorageMode::OsKeyring);
        // Successful migration should remove the key file and persist into keyring
        assert!(!key_file.exists());
        assert_eq!(mock_keyring::get(SERVICE_NAME, KEY_NAME), Some(test_key));
    }

    #[test]
    fn test_release_mode_migrates_legacy_service_key() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let legacy_key = test_hex_key(0x1111_2222_3333_4444);
        mock_keyring::insert(LEGACY_SERVICE_NAME, KEY_NAME, &legacy_key);
        let dir = tempdir().unwrap();

        let (loaded, mode) =
            get_or_create_encryption_key_with_mode(dir.path(), false, false).unwrap();
        assert_eq!(loaded.as_str(), legacy_key.as_str());
        assert_eq!(mode, KeyStorageMode::OsKeyring);
        // Should have migrated into current SERVICE_NAME
        assert_eq!(mock_keyring::get(SERVICE_NAME, KEY_NAME), Some(legacy_key));
    }

    #[test]
    fn test_keyring_mock_isolation() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        assert_eq!(read_from_keyring(), None);
        let mock_key = format!("mock-{}-{}", 1, 2);
        write_to_keyring(&mock_key).unwrap();
        assert_eq!(
            read_from_keyring(),
            Some((Zeroizing::new(mock_key), SERVICE_NAME))
        );
        mock_keyring::reset();
        assert_eq!(read_from_keyring(), None);
    }
}
