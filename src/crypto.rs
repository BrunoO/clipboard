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
/// Returns an error if a key file cannot be read or written with restricted permissions,
/// or if an existing encrypted database is found but its key cannot be recovered.
pub(crate) fn get_or_create_encryption_key(
    data_dir: &Path,
    db_exists: bool,
) -> Result<(Zeroizing<String>, KeyStorageMode), Box<dyn std::error::Error>> {
    get_or_create_encryption_key_with_mode(data_dir, db_exists, cfg!(debug_assertions))
}

fn get_or_create_encryption_key_with_mode(
    data_dir: &Path,
    db_exists: bool,
    allow_key_file_cache: bool,
) -> Result<(Zeroizing<String>, KeyStorageMode), Box<dyn std::error::Error>> {
    let key_file = data_dir.join(".encryption_key");

    if allow_key_file_cache {
        // Debug mode: prefer local key file to avoid Keychain prompts on every recompile.
        if let Some(key) = read_key_file(&key_file)? {
            let permissions_degraded = set_restricted_permissions(&key_file);
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
            let permissions_degraded = set_restricted_permissions(&key_file);
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
        return Err(missing_key_error_message(data_dir).into());
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
    let dir = data_dir.display();
    #[cfg(target_os = "windows")]
    {
        format!(
            "An existing encrypted database was found in '{dir}', but its encryption key could not be found.\n\
             Generating a new random key would fail to decrypt the existing database.\n\
             If you want to start fresh with a new database, delete that folder in File Explorer \
             (or run in PowerShell: Remove-Item -Recurse -Force '{dir}') and restart."
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!(
            "An existing encrypted database was found in '{dir}', but its encryption key could not be found.\n\
             Generating a new random key would fail to decrypt the existing database.\n\
             If you want to start fresh with a new database, delete the directory and restart:\n\
             rm -rf '{dir}'"
        )
    }
}

fn read_key_file(path: &Path) -> Result<Option<Zeroizing<String>>, std::io::Error> {
    if !path.exists() {
        return Ok(None);
    }
    #[cfg(target_os = "windows")]
    let key = {
        let ciphertext = fs::read(path)?;
        if ciphertext.is_empty() {
            return Ok(None);
        }
        let plain_bytes = crate::platform::windows::dpapi_unprotect(&ciphertext)
            .map_err(std::io::Error::other)?;
        let utf8 = std::str::from_utf8(&plain_bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Zeroizing::new(utf8.to_owned())
    };
    #[cfg(not(target_os = "windows"))]
    let key = Zeroizing::new(fs::read_to_string(path)?);

    let trimmed = Zeroizing::new(key.trim().to_string());
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    let permissions_degraded = {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(key.as_bytes())?;
        file.sync_all()?;
        // Also tighten permissions if the file already existed with broader mode.
        set_restricted_permissions(path)
    };
    #[cfg(target_os = "windows")]
    let permissions_degraded = {
        let protected = crate::platform::windows::dpapi_protect(key.as_bytes())
            .map_err(std::io::Error::other)?;
        fs::write(path, &protected)?;
        set_restricted_permissions(path)
    };
    #[cfg(not(any(unix, target_os = "windows")))]
    let permissions_degraded = {
        fs::write(path, key)?;
        set_restricted_permissions(path)
    };
    Ok(permissions_degraded)
}

fn generate_random_hex_key() -> Zeroizing<String> {
    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let hex_key = Zeroizing::new(hex::encode(key_bytes));
    key_bytes.zeroize();
    hex_key
}

#[cfg(unix)]
pub(crate) fn set_restricted_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
pub(crate) const fn set_restricted_dir_permissions(_path: &Path) {}

/// Restricts a fallback key file and returns `true` when hardening could not be applied.
#[cfg(unix)]
fn set_restricted_permissions(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    if fs::set_permissions(path, fs::Permissions::from_mode(0o600)).is_ok() {
        false
    } else {
        crate::logging::log_message(
            "WARN",
            "Could not set 0600 permissions on the local key file. Unlock the OS keyring and restart to migrate the key.",
        );
        true
    }
}

#[cfg(any(target_os = "windows", test))]
fn resolve_windows_icacls_path(system_root: Option<&str>) -> std::path::PathBuf {
    let root = system_root
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(r"C:\Windows");
    let mut path = std::path::PathBuf::from(root);
    path.push("System32");
    path.push("icacls.exe");
    path
}

#[cfg(any(target_os = "windows", test))]
fn windows_icacls_grant(username: &str, domain: Option<&str>) -> Option<String> {
    let username = username.trim();
    if username.is_empty() {
        return None;
    }
    let user = match domain.map(str::trim).filter(|d| !d.is_empty()) {
        Some(domain) if !username.contains('\\') => format!("{domain}\\{username}"),
        _ => username.to_owned(),
    };
    Some(format!("{user}:(F)"))
}

/// Restricts a fallback key file with `icacls` and returns `true` when hardening failed.
#[cfg(target_os = "windows")]
fn set_restricted_permissions(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Some(path_str) = path.to_str() else {
        crate::logging::log_message(
            "WARN",
            "Skipping key file ACL hardening: key file path is not valid Unicode. Unlock the OS keyring and restart to migrate the key.",
        );
        return true;
    };

    // Best-effort hardening: remove inherited ACLs and grant only the current user full control.
    // This reduces exposure compared to inheriting broad folder ACLs in %LOCALAPPDATA%.
    //
    // Prefer the unambiguous `USERDOMAIN\\USERNAME` form: a bare user name can be ambiguous
    // (a local and a domain account sharing the same name) or fail to resolve on domain-joined
    // and Microsoft Entra (Azure AD) machines, in which case icacls errors out and the broad
    // inherited ACLs would silently remain in place.
    let username = std::env::var("USERNAME").unwrap_or_default();
    if username.trim().is_empty() {
        crate::logging::log_message(
            "WARN",
            "Skipping key file ACL hardening: USERNAME is empty. Set USERNAME/USERDOMAIN or unlock the OS keyring and restart to migrate the key.",
        );
        return true;
    }
    let domain = std::env::var("USERDOMAIN").ok();
    let Some(grant) = windows_icacls_grant(&username, domain.as_deref()) else {
        crate::logging::log_message(
            "WARN",
            "Skipping key file ACL hardening: USERNAME is empty. Set USERNAME/USERDOMAIN or unlock the OS keyring and restart to migrate the key.",
        );
        return true;
    };
    let system_root = std::env::var("SystemRoot").ok();
    let icacls_path = resolve_windows_icacls_path(system_root.as_deref());

    match Command::new(&icacls_path)
        .args([path_str, "/inheritance:r", "/grant:r", &grant])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) if output.status.success() => false,
        Ok(output) => {
            let msg = format!(
                "Failed to restrict key file ACLs via {} (exit {:?}). Check USERDOMAIN\\USERNAME resolution and icacls permissions, or unlock the OS keyring and restart to migrate the key.",
                icacls_path.display(),
                output.status.code()
            );
            eprintln!("⚠️ {msg}");
            crate::logging::log_message("WARN", &msg);
            true
        }
        Err(e) => {
            let msg = format!(
                "Failed to execute {} for key file ACL hardening: {e}. Check icacls availability, or unlock the OS keyring and restart to migrate the key.",
                icacls_path.display()
            );
            eprintln!("⚠️ {msg}");
            crate::logging::log_message("WARN", &msg);
            true
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn set_restricted_permissions(_path: &Path) -> bool {
    // No platform-specific hardening implemented.
    false
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

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&key_file).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Key file permissions must be 0600 on Unix");
        }

        #[cfg(target_os = "windows")]
        {
            let raw_on_disk = fs::read(&key_file).unwrap();
            assert_ne!(
                raw_on_disk.as_slice(),
                test_key.as_bytes(),
                "Windows fallback key file must be DPAPI-encrypted on disk"
            );
        }

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
        assert!(res.is_err());
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

    #[cfg(unix)]
    #[test]
    fn test_restricted_dir_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let sub = dir.path().join("secure_data");
        fs::create_dir_all(&sub).unwrap();
        set_restricted_dir_permissions(&sub);
        let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o700,
            "Data directory permissions must be 0700 on Unix"
        );
    }

    #[test]
    fn test_resolve_windows_icacls_path() {
        let custom = resolve_windows_icacls_path(Some(r"D:\WinOS"));
        assert!(custom.ends_with("icacls.exe"));
        assert!(custom.to_string_lossy().contains("System32"));
        assert!(custom.to_string_lossy().starts_with(r"D:\WinOS"));

        let fallback = resolve_windows_icacls_path(None);
        assert!(fallback.to_string_lossy().starts_with(r"C:\Windows"));
        assert!(fallback.ends_with("icacls.exe"));

        let blank = resolve_windows_icacls_path(Some("   "));
        assert!(blank.to_string_lossy().starts_with(r"C:\Windows"));
    }

    #[test]
    fn test_windows_icacls_grant_requires_username() {
        assert_eq!(windows_icacls_grant("", None), None);
        assert_eq!(windows_icacls_grant("   ", Some("CORP")), None);
    }

    #[test]
    fn test_windows_icacls_grant_qualifies_domain_accounts() {
        assert_eq!(
            windows_icacls_grant("alice", Some("CORP")),
            Some("CORP\\alice:(F)".to_owned())
        );
        assert_eq!(
            windows_icacls_grant("alice", Some("   ")),
            Some("alice:(F)".to_owned())
        );
        assert_eq!(
            windows_icacls_grant("CORP\\alice", Some("OTHER")),
            Some("CORP\\alice:(F)".to_owned())
        );
    }
}
