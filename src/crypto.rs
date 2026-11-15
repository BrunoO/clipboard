#[cfg(not(test))]
use keyring::Entry;
use rand::Rng;
use std::fs;
use std::path::Path;

const SERVICE_NAME: &str = "cipherclipboard";
const LEGACY_SERVICE_NAME: &str = "rusty_clipboard";
const KEY_NAME: &str = "db_encryption_key";

/// Retrieves or securely generates a 256-bit encryption key.
///
/// Strategy:
/// - **Debug builds** (`cfg!(debug_assertions)`): Checks `data_dir/.encryption_key` first so
///   recompiled debug binaries don't trigger repeated macOS Keychain ACL prompts, then falls back
///   to the OS Keyring.
/// - **Release builds** (`!cfg!(debug_assertions)`): Uses the **OS Keyring exclusively** (macOS
///   Keychain, Windows Credential Manager, Linux Secret Service). Any legacy `.encryption_key`
///   file is migrated into the OS Keyring and securely removed from disk. Falls back to the
///   restricted local key file only if the OS Keyring is unavailable (e.g., headless Linux).
pub(crate) fn get_or_create_encryption_key(
    data_dir: &Path,
    db_exists: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    get_or_create_encryption_key_with_mode(data_dir, db_exists, cfg!(debug_assertions))
}

fn get_or_create_encryption_key_with_mode(
    data_dir: &Path,
    db_exists: bool,
    allow_key_file_cache: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let key_file = data_dir.join(".encryption_key");

    if allow_key_file_cache {
        // Debug mode: prefer local key file to avoid Keychain prompts on every recompile.
        if let Some(key) = read_key_file(&key_file)? {
            set_restricted_permissions(&key_file);
            #[cfg(debug_assertions)]
            println!(
                "🔐 Loaded encryption key from local key store: {}",
                key_file.display()
            );
            return Ok(key);
        }

        if let Some((key, service)) = read_from_keyring() {
            #[cfg(debug_assertions)]
            println!("🔐 Loaded encryption key from OS Keyring ({service}).");
            #[cfg(not(debug_assertions))]
            let _ = service;
            save_key_file(&key_file, &key)?;
            return Ok(key);
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
            return Ok(key);
        }

        // Migration path: if an older version stored .encryption_key, move it into the OS Keyring.
        if let Some(key) = read_key_file(&key_file)? {
            if write_to_keyring(&key).is_ok() {
                remove_key_file(&key_file);
                #[cfg(debug_assertions)]
                println!("🔐 Migrated encryption key into OS Keyring and removed local key file.");
            } else {
                set_restricted_permissions(&key_file);
                #[cfg(debug_assertions)]
                eprintln!(
                    "ℹ️ OS Keyring unavailable; continuing with restricted local key file: {}",
                    key_file.display()
                );
            }
            return Ok(key);
        }
    }

    // Safety Check: If the database already exists on disk, we MUST NOT generate a new random key!
    if db_exists {
        return Err(format!(
            "An existing encrypted database was found in '{}', but its encryption key could not be found.\n\
             Generating a new random key would fail to decrypt the existing database.\n\
             If you want to start fresh with a new database, delete the directory and restart:\n\
             rm -rf '{}'",
            data_dir.display(),
            data_dir.display()
        ).into());
    }

    // Fresh setup: Generate a new random 256-bit key
    let new_key = generate_random_hex_key();
    #[cfg(debug_assertions)]
    println!("🔑 Generating new 256-bit encryption key for first-time setup...");

    if allow_key_file_cache {
        save_key_file(&key_file, &new_key)?;
        if let Err(e) = write_to_keyring(&new_key) {
            #[cfg(debug_assertions)]
            eprintln!(
                "ℹ️ Could not write key to OS Keyring ({e}); using local secure file instead."
            );
            #[cfg(not(debug_assertions))]
            let _ = e;
        } else {
            #[cfg(debug_assertions)]
            println!("🔑 Backed up encryption key to OS Keyring.");
        }
    } else if let Err(e) = write_to_keyring(&new_key) {
        #[cfg(debug_assertions)]
        eprintln!(
            "ℹ️ Could not write key to OS Keyring ({e}); falling back to restricted local key file."
        );
        #[cfg(not(debug_assertions))]
        let _ = e;
        save_key_file(&key_file, &new_key)?;
    } else {
        #[cfg(debug_assertions)]
        println!("🔑 Stored encryption key in OS Keyring (Keyring-only release mode).");
    }

    Ok(new_key)
}

fn read_key_file(path: &Path) -> Result<Option<String>, std::io::Error> {
    if !path.exists() {
        return Ok(None);
    }
    let key = fs::read_to_string(path)?;
    let trimmed = key.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

#[cfg(not(test))]
fn read_from_keyring() -> Option<(String, &'static str)> {
    for service in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        if let Ok(entry) = Entry::new(service, KEY_NAME) {
            if let Ok(key) = entry.get_password() {
                let trimmed = key.trim().to_string();
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
        let mut guard = MOCK_KEYRING.lock().unwrap();
        if guard.is_none() {
            *guard = Some(MockKeyringState::default());
        }
        f(guard.as_mut().unwrap())
    }

    pub(crate) fn reset() {
        let mut guard = MOCK_KEYRING.lock().unwrap();
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
fn read_from_keyring() -> Option<(String, &'static str)> {
    for service in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        if let Some(key) = mock_keyring::get(service, KEY_NAME) {
            let trimmed = key.trim().to_string();
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

fn save_key_file(path: &Path, key: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, key)?;
    set_restricted_permissions(path);
    Ok(())
}

fn generate_random_hex_key() -> String {
    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let hex_key = hex::encode(key_bytes);
    key_bytes.fill(0);
    hex_key
}

#[cfg(unix)]
fn set_restricted_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn set_restricted_permissions(path: &Path) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Some(path_str) = path.to_str() else {
        return;
    };

    // Best-effort hardening: remove inherited ACLs and grant only the current user full control.
    // This reduces exposure compared to inheriting broad folder ACLs in %LOCALAPPDATA%.
    let username = match std::env::var("USERNAME") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => return,
    };
    let grant = format!("{username}:(F)");

    let _ = Command::new("icacls")
        .args([path_str, "/inheritance:r", "/grant:r", &grant])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

#[cfg(not(any(unix, windows)))]
fn set_restricted_permissions(_path: &Path) {
    // No platform-specific hardening implemented.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_random_hex_key() {
        let key = generate_random_hex_key();
        assert_eq!(key.len(), 64);
        assert!(hex::decode(&key).is_ok());
    }

    #[test]
    fn test_save_and_read_key_file() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        save_key_file(&key_file, test_key).unwrap();
        assert!(key_file.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&key_file).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Key file permissions must be 0600 on Unix");
        }

        let loaded = get_or_create_encryption_key(dir.path(), false).unwrap();
        assert_eq!(loaded, test_key);
    }

    #[test]
    fn test_remove_key_file_deletes_file() {
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        save_key_file(&key_file, "secret").unwrap();
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
        assert!(res.is_err());
    }

    #[test]
    fn test_release_mode_reads_legacy_key_file_when_keyring_unavailable() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        mock_keyring::set_fail_writes(true);
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        save_key_file(&key_file, test_key).unwrap();

        let loaded = get_or_create_encryption_key_with_mode(dir.path(), true, false).unwrap();
        assert_eq!(loaded, test_key);
        // Since write_to_keyring failed, the key_file should remain on disk
        assert!(key_file.exists());
    }

    #[test]
    fn test_release_mode_migrates_legacy_key_file_to_keyring() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let dir = tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let test_key = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        save_key_file(&key_file, test_key).unwrap();

        let loaded = get_or_create_encryption_key_with_mode(dir.path(), true, false).unwrap();
        assert_eq!(loaded, test_key);
        // Successful migration should remove the key file and persist into keyring
        assert!(!key_file.exists());
        assert_eq!(
            mock_keyring::get(SERVICE_NAME, KEY_NAME),
            Some(test_key.to_string())
        );
    }

    #[test]
    fn test_release_mode_migrates_legacy_service_key() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        let legacy_key = "1111222233334444111122223333444411112222333344441111222233334444";
        mock_keyring::insert(LEGACY_SERVICE_NAME, KEY_NAME, legacy_key);
        let dir = tempdir().unwrap();

        let loaded = get_or_create_encryption_key_with_mode(dir.path(), false, false).unwrap();
        assert_eq!(loaded, legacy_key);
        // Should have migrated into current SERVICE_NAME
        assert_eq!(
            mock_keyring::get(SERVICE_NAME, KEY_NAME),
            Some(legacy_key.to_string())
        );
    }

    #[test]
    fn test_keyring_mock_isolation() {
        let _lock = mock_keyring::TEST_LOCK.lock().unwrap();
        mock_keyring::reset();
        assert_eq!(read_from_keyring(), None);
        write_to_keyring("abc123mockkey").unwrap();
        assert_eq!(
            read_from_keyring(),
            Some(("abc123mockkey".to_string(), SERVICE_NAME))
        );
        mock_keyring::reset();
        assert_eq!(read_from_keyring(), None);
    }
}
