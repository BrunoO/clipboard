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
/// 1. Check local secure key file (`data_dir/.encryption_key`).
///    This avoids macOS Keychain ACL issues where debug binaries recompiled by
///    `cargo run` change hash and get blocked by the OS.
/// 2. If not in the local file, check the OS Keyring (macOS Keychain, Windows Credential Manager).
/// 3. If neither exists:
///    - If the database file ALREADY exists, fail loudly rather than generating a new random key!
///    - Otherwise, generate a new key, save it with restricted permissions (0600 on Unix),
///      and back it up to the OS Keyring.
pub(crate) fn get_or_create_encryption_key(
    data_dir: &Path,
    db_exists: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let key_file = data_dir.join(".encryption_key");

    // 1. Try reading the local persistent key file
    if key_file.exists() {
        let key = fs::read_to_string(&key_file)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            // Best-effort: enforce restrictive permissions for existing key files (e.g. legacy/plain ACLs on Windows).
            set_restricted_permissions(&key_file);
            println!(
                "🔐 Loaded encryption key from local key store: {}",
                key_file.display()
            );
            return Ok(trimmed);
        }
    }

    // 2. Try reading from the OS Keyring (checking current service, then legacy fallback)
    for service in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        if let Ok(entry) = Entry::new(service, KEY_NAME) {
            if let Ok(key) = entry.get_password() {
                let trimmed = key.trim().to_string();
                if !trimmed.is_empty() {
                    println!("🔐 Loaded encryption key from OS Keyring ({service}).");
                    // Cache locally so future rebuilds don't trigger macOS Keychain prompts
                    save_key_file(&key_file, &trimmed)?;
                    return Ok(trimmed);
                }
            }
        }
    }

    // 3. Safety Check: If the database already exists on disk, we MUST NOT generate a new random key!
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

    // 4. Fresh setup: Generate a new random 256-bit key
    let new_key = generate_random_hex_key();
    println!("🔑 Generating new 256-bit encryption key for first-time setup...");

    // Save to local key file with restricted permissions
    save_key_file(&key_file, &new_key)?;

    // Best-effort backup to OS Keyring
    if let Ok(entry) = Entry::new(SERVICE_NAME, KEY_NAME) {
        if let Err(e) = entry.set_password(&new_key) {
            eprintln!(
                "ℹ️ Could not write key to OS Keyring ({e}); using local secure file instead."
            );
        } else {
            println!("🔑 Backed up encryption key to OS Keyring.");
        }
    }

    Ok(new_key)
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
    use std::process::Command;

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
}
