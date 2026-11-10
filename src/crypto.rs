use keyring::Entry;
use rand::RngCore;
use std::fs;
use std::path::Path;

const SERVICE_NAME: &str = "rusty_clipboard";
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
pub fn get_or_create_encryption_key(
    data_dir: &Path,
    db_exists: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let key_file = data_dir.join(".encryption_key");

    // 1. Try reading the local persistent key file
    if key_file.exists() {
        let key = fs::read_to_string(&key_file)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            println!("🔐 Loaded encryption key from local key store: {}", key_file.display());
            return Ok(trimmed);
        }
    }

    // 2. Try reading from the OS Keyring
    let keyring_entry = Entry::new(SERVICE_NAME, KEY_NAME).ok();
    if let Some(ref entry) = keyring_entry {
        if let Ok(key) = entry.get_password() {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() {
                println!("🔐 Loaded encryption key from OS Keyring.");
                // Cache locally so future rebuilds don't trigger macOS Keychain prompts
                save_key_file(&key_file, &trimmed)?;
                return Ok(trimmed);
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
    if let Some(entry) = keyring_entry {
        if let Err(e) = entry.set_password(&new_key) {
            eprintln!("ℹ️ Could not write key to OS Keyring ({e}); using local secure file instead.");
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
    rand::thread_rng().fill_bytes(&mut key_bytes);
    hex::encode(key_bytes)
}

#[cfg(unix)]
fn set_restricted_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_restricted_permissions(_path: &Path) {
    // Windows inherits folder ACLs in %LOCALAPPDATA%
}
