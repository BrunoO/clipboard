//! Platform-specific filesystem permissions, ACL hardening, and fallback key protection.
//!
//! All implementations in this module are 100% safe Rust; OS-native encryption (such as
//! Windows DPAPI) is delegated to the target-gated FFI submodules (`platform::windows`).

use std::fs;
use std::path::Path;
use zeroize::Zeroizing;

/// Formats the user-facing error message when an encrypted database already exists
/// on disk, but its encryption key cannot be found in either the OS keyring or the
/// local fallback key file.
#[must_use]
pub(crate) fn missing_key_error_message(data_dir: &Path) -> String {
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

/// Reads and unprotects the raw encryption key string from a local fallback key file.
///
/// On Windows, the file contents are decrypted via user-scoped DPAPI (`CryptUnprotectData`).
/// On other platforms, the file is read as a UTF-8 string.
pub(crate) fn read_protected_key_file(
    path: &Path,
) -> Result<Option<Zeroizing<String>>, std::io::Error> {
    if !path.exists() {
        return Ok(None);
    }
    #[cfg(target_os = "windows")]
    let key = {
        let ciphertext = fs::read(path)?;
        if ciphertext.is_empty() {
            return Ok(None);
        }
        let plain_bytes =
            super::windows::dpapi_unprotect(&ciphertext).map_err(std::io::Error::other)?;
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

/// Protects and writes the encryption key to a local fallback key file with hardened
/// owner-only permissions. Returns `Ok(true)` if permission hardening was degraded.
pub(crate) fn write_protected_key_file(path: &Path, key: &str) -> Result<bool, std::io::Error> {
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
        let protected =
            super::windows::dpapi_protect(key.as_bytes()).map_err(std::io::Error::other)?;
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

/// Restricts the application data directory to owner-only access (`0o700`) on Unix.
#[cfg(unix)]
pub(crate) fn set_restricted_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

/// No-op on non-Unix platforms (Windows directory ACLs are inherited from `%LOCALAPPDATA%`
/// and individual sensitive files are hardened via DPAPI and `icacls`).
#[cfg(not(unix))]
pub(crate) const fn set_restricted_dir_permissions(_path: &Path) {}

/// Opens or creates a file in append mode with owner-only (`0o600`) permissions on Unix.
///
/// # Errors
/// Returns the underlying I/O error if the file cannot be opened or created.
pub(crate) fn open_restricted_append_file(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Restricts a fallback key file and returns `true` when hardening could not be applied.
#[cfg(unix)]
pub(crate) fn set_restricted_permissions(path: &Path) -> bool {
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
pub(crate) fn resolve_windows_icacls_path(system_root: Option<&str>) -> std::path::PathBuf {
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
pub(crate) fn windows_icacls_grant(username: &str, domain: Option<&str>) -> Option<String> {
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
pub(crate) fn set_restricted_permissions(path: &Path) -> bool {
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
pub(crate) fn set_restricted_permissions(_path: &Path) -> bool {
    // No platform-specific hardening implemented.
    false
}

#[cfg(test)]
pub(crate) fn assert_protected_key_file_on_disk(path: &Path, expected_key: &str) {
    let _ = (path, expected_key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Key file permissions must be 0600 on Unix");
    }

    #[cfg(target_os = "windows")]
    {
        let raw_on_disk = fs::read(path).unwrap();
        assert_ne!(
            raw_on_disk.as_slice(),
            expected_key.as_bytes(),
            "Windows fallback key file must be DPAPI-encrypted on disk"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
