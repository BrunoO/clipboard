# Security Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10 — all normative security controls in `docs/SECURITY_LESSONS.md` (keyring fail-closed, `KeyStorageMode::LocalFileFallbackDegraded`, DPAPI + `icacls` hardening, `zeroize::Zeroizing`, SQLCipher deletion and WAL truncation, `LIKE` escaping + secret exclusion, incoming/outgoing concealed markers, Windows UIPI fail-closed, `// SAFETY:` comments on all FFI `unsafe` blocks, URL allow-list, PNG decompression bomb guards, metadata-only debug clipboard logging, SHA-pinned CI and release verification) are verified in source; only two minor low-severity items remain.
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2 (0 Critical, 0 High, 0 Medium, 2 Low)
- **Estimated Remediation Effort**: 0.5 hours
- **Evidence Commands Run**:
  - `grep -RInE 'unwrap\(|expect\(|panic!|unsafe|#[^!]allow\(unsafe_code\)|#!\[allow\(unsafe_code\)' src` (261 matches; `#![allow(unsafe_code)]` strictly confined to `src/platform/macos.rs:1` and `src/platform/windows.rs:1`)
  - `grep -RInE 'webbrowser::open|Url::parse|image::open|decode\(|Limits' src` (8 matches)
  - `grep -RInE 'secure_delete|wal_checkpoint|VACUUM|ESCAPE|kind != .secret.|parameter|format!\(' src` (200 matches)
  - `grep -RInE 'println!|dbg!|info!|warn!|error!|debug!|trace!' src` (76 matches)
  - `grep -RIni 'safety' src/platform; echo "safety_exit=$?"` → `safety_exit=0` (44 matches across `src/platform/windows.rs` and `src/platform/macos.rs`)
  - `grep -RIn 'TEST_KEY' src/` → `src/settings.rs:572,578`
  - Direct file reads of `Cargo.toml`, `deny.toml`, `.github/workflows/{ci,release,sonarcloud}.yml`, `scripts/windows/setup-openssl-ci.ps1`, `docs/SECURITY_LESSONS.md`, `src/main.rs`, `src/crypto.rs`, `src/db.rs`, `src/monitor.rs`, `src/media.rs`, `src/logging.rs`, `src/secret.rs`, `src/password.rs`, `src/settings.rs`, `src/instance.rs`, `src/tray.rs`, `src/app/{mod.rs,history_view.rs,settings_view.rs,generator_view.rs}`, and `src/platform/{mod.rs,fs_security.rs,macos.rs,windows.rs,linux.rs}`.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium
No findings in this category.

### Low

**L1. Two remaining static 64-hex key-shaped literals in unit tests**
- **Threat / asset**: Secret-scanner signal quality in CI and repository scans (`docs/SECURITY_LESSONS.md:59`).
- **Severity**: Low.
- **Evidence** — `docs/SECURITY_LESSONS.md:59` verbatim:
  ```markdown
  - **Enforce `cargo-deny`** with `wildcards = "deny"`, `unknown-registry = "deny"`, and `unknown-git = "deny"`, and avoid static secret literals in unit tests (construct test fixtures dynamically so secret scanners don't mask real leaks behind false positives).
  ```
  While `src/crypto.rs:320-322` and `src/db.rs:870-872` now use dynamic `test_hex_key(seed: u64)` helpers, two static 64-character hex literals remain in test code (literal values redacted per Global Evidence Rule 10):
  1. `src/settings.rs:572-578` verbatim (value at `:572` redacted):
     ```rust
         const TEST_KEY: &str = "<redacted-64-hex-literal>";

         #[test]
         fn test_settings_default_and_roundtrip() {
             let dir = tempdir().unwrap();
             let db_path = dir.path().join("settings_module_test.db");
             let db = Database::open(&db_path, TEST_KEY).unwrap();
     ```
  2. `src/db.rs:1080-1082` verbatim (value at `:1081` redacted):
     ```rust
             // 2. Open with wrong key - must fail verification
             let wrong_key = "<redacted-64-hex-literal>";
             let result = Database::open(&db_path, wrong_key);
     ```
- **Attack path or failure mode**: High-entropy/hex secret scanners flag 64-character hex strings as potential 256-bit keys; maintaining ignore rules for test files can mask an accidentally committed real key.
- **Impact**: Auditability / secret-scanner hygiene only (no runtime confidentiality or integrity impact).
- **Recommendation / test**: Replace `TEST_KEY` in `src/settings.rs:572` with `format!("{:016x}", 1_u64).repeat(4)` and `wrong_key` in `src/db.rs:1081` with `&test_hex_key(0xFFFF_FFFF_FFFF_FFFF)`.

**L2. `[Inferred]` Single-instance lock and PID sidecar files on Unix use default `umask` rather than explicit `0600`**
- **Threat / asset**: `.instance.lock` and `.instance.pid` inside the application data directory (`src/instance.rs`).
- **Severity**: Low.
- **Evidence** — `src/instance.rs:25-30` verbatim:
  ```rust
          let mut file = OpenOptions::new()
              .read(true)
              .write(true)
              .create(true)
              .truncate(false)
              .open(lock_path)?;
  ```
  and `src/instance.rs:42-43` verbatim:
  ```rust
                  let sidecar = lock_path.with_extension("pid");
                  let _ = std::fs::write(&sidecar, std::process::id().to_string());
  ```
  Contrast `src/logging.rs:67-71` and `src/platform/fs_security.rs:80-85`, which set `OpenOptionsExt::mode(0o600)` at creation.
- **Attack path or failure mode**: `[Inferred]` The parent `data_dir` is already restricted to `0700` on Unix via `crypto::set_restricted_dir_permissions(&data_dir)` (`src/main.rs:81` → `src/platform/fs_security.rs:108-111`), which prevents traversal by other local users, and the files contain only the process PID. However, if `data_dir` permissions were ever loosened manually by a user, `.instance.lock` and `.instance.pid` would inherit default `umask` (`0644`).
- **Impact**: Minimal (PID disclosure only if parent directory `0700` permissions are externally weakened).
- **Recommendation / test**: Apply `OpenOptionsExt::mode(0o600)` (or a shared helper in `src/platform/fs_security.rs`) when creating `.instance.lock` and `.instance.pid` on Unix for defense-in-depth consistency with `app.log` and `.encryption_key`.

## Quick Wins
1. Replace `const TEST_KEY` in `src/settings.rs:572` and `wrong_key` in `src/db.rs:1081` with dynamically constructed hex fixtures (`test_hex_key(...)`) (L1; ~10 min).
2. Set `0600` creation mode on `.instance.lock` and `.instance.pid` on Unix (L2; ~15 min).

## Recommended Actions
1. **L1 (Low, ~0.25h)**: Eliminate the last two static 64-hex literals in `src/settings.rs:572` and `src/db.rs:1081` so the entire test suite uses dynamic fixtures.
2. **L2 (Low, ~0.25h)**: Use `OpenOptionsExt::mode(0o600)` on Unix when creating `.instance.lock` and `.instance.pid` in `src/instance.rs`.

## Coverage
- **1. Key Management and Secret Lifetime**:
  - OS keyring is preferred in release builds (`src/crypto.rs:93-125`), and an existing encrypted DB fails closed without generating a replacement key (`src/crypto.rs:127-130` verbatim: `if db_exists { return Err(missing_key_error_message(data_dir).into()); }`).
  - Local fallback and degraded permission states are surfaced in UI via `KeyStorageMode::LocalFileFallback` and `KeyStorageMode::LocalFileFallbackDegraded` (`src/crypto.rs:14-29`, `src/app/mod.rs:383-404,1215-1240`, `src/app/settings_view.rs:539-568`).
  - Unix fallback key protection uses `0700` on `data_dir` (`src/platform/fs_security.rs:108-111`) and `OpenOptionsExt::mode(0o600)` at file creation (`src/platform/fs_security.rs:80-90`).
  - Windows fallback key protection uses user-scoped DPAPI with `CRYPTPROTECT_UI_FORBIDDEN` and application entropy `DPAPI_ENTROPY`, zeroizing OS-allocated plaintext buffers before `LocalFree` (`src/platform/windows.rs:727-831`), plus ACL hardening via `%SystemRoot%\System32\icacls.exe` with `USERDOMAIN\USERNAME` and `CREATE_NO_WINDOW` (`src/platform/fs_security.rs:134-227`).
  - `zeroize::Zeroizing` wraps DB keys (`src/crypto.rs:59,303-309`), transient skip buffers (`src/db.rs:157,322-325`), pending clipboard secrets (`src/app/mod.rs:202,457-464`), generated passwords (`src/password.rs:114,132,142`), and DPAPI plaintext (`src/platform/windows.rs:784,825-826`).
  - DB key lifetime is scoped to `open_encrypted_database` and dropped before entering `eframe::run_native` (`src/main.rs:107,258-266`).
  - No findings in this category.
- **2. Database and Deletion Hygiene**:
  - All SQL queries use parameterized `params![...]` (`src/db.rs:365,373,482,487,527,532,587,593,656,674,684,738,754,780,791,842,859`); schema migration `ALTER TABLE` statements iterate over a fixed array of string literals (`src/db.rs:240-251`).
  - `PRAGMA secure_delete = ON;` is enabled at `src/db.rs:211-215`.
  - Full clear runs `DELETE FROM clipboard_history`, `VACUUM;`, and `PRAGMA wal_checkpoint(TRUNCATE);` and surfaces partial compaction failures via `ClearError::Compaction` (`src/db.rs:802-833`, `src/app/mod.rs:1387-1409`).
  - `LIKE` wildcards `\`, `%`, `_` are escaped with `ESCAPE '\\'` and masked secrets are excluded via `WHERE kind != 'secret'` (`src/db.rs:625-655`).
  - Coupled write operations (duplicate check → delete stale → insert → `prune_in_tx`) execute inside a single `conn.transaction()?` (`src/db.rs:456-495,510-539,563-601`).
  - No findings in this category.
- **3. Clipboard Privacy Boundaries**:
  - Incoming concealed markers honored on macOS (`src/platform/macos.rs:394-431`), Windows (`src/platform/windows.rs:480-512`), and Linux X11 (`src/platform/linux.rs:261-351`), gated at `src/monitor.rs:75-78`.
  - Outgoing secret writes call `crate::platform::mark_clipboard_concealed()` and `schedule_secret_clipboard_clear` (`src/app/mod.rs:554-555,590-591`), and Linux's X11 selection-ownership limitation is explicitly disclosed in the UI via `concealed_marker_limitation()` (`src/platform/mod.rs:301-329`, `src/app/settings_view.rs:471-478`).
  - Pending secrets are cleared from the clipboard on shutdown and `Drop` via `shutdown_resources()` → `clear_pending_secret_from_clipboard()` (`src/app/mod.rs:466-477,1593-1628`).
  - Synthetic paste verifies target foreground ownership before injecting keystrokes (`src/platform/mod.rs:493-508`).
  - Windows UIPI elevation check uses an 8-byte-aligned 256-byte `TokenIntegrityBuf` and fails closed when `TOKEN_QUERY` is denied (`src/platform/windows.rs:622-697`, `src/platform/mod.rs:358-371`).
  - macOS accessibility permission status (`AXIsProcessTrusted`) and settings deep-link (`x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`) are surfaced in Settings (`src/platform/macos.rs:243-272`, `src/app/settings_view.rs:386-414`).
  - No findings in this category.
- **4. Unsafe and FFI Safety**:
  - Crate-wide `unsafe_code = "deny"` in `Cargo.toml:90-91`; `#![allow(unsafe_code)]` is confined strictly to `src/platform/macos.rs:1` and `src/platform/windows.rs:1`.
  - Every `unsafe` block and `unsafe fn` in `src/platform/macos.rs` (15 `// SAFETY:` comments) and `src/platform/windows.rs` (25 `// SAFETY:` comments and 4 `/// # Safety` doc comments) now carries an explicit safety justification (`grep -RIni 'safety' src/platform` → 44 matches, `exit=0`).
  - Checked integer conversions (`u32::try_from`, `usize::try_from`, `i32::try_from`), explicit raw pointers (`&raw mut`, `&raw const`, `addr_of_mut!`), and explicit `#[link(name = "...")]` attributes (`src/platform/macos.rs:220,226,238`) are used throughout.
  - macOS FFI guards `NSApplication` calls with `is_main_thread()`, wraps ObjC calls in `objc::rc::autoreleasepool`, filters `[ns_app windows]` with `canBecomeMainWindow`, defers reopen handling out of `NSNotificationCenter` via `REOPEN_REQUESTED`, stores `REOPEN_OBSERVER` in `thread_local!`, and exits cleanly via `on_exit` → `shutdown_and_exit` → `std::process::exit(0)` (`src/platform/macos.rs:44-218`, `src/app/mod.rs:1517-1522,1617-1621`).
  - No findings in this category.
- **5. Input Validation and Resource Exhaustion**:
  - Browser URLs are validated with `url::Url::parse` in `sanitize_browser_url`, allowing only `http`/`https` with a non-empty host (`src/app/history_view.rs:196-213,1103-1110`); the only other `webbrowser::open` call uses a compile-time constant repository URL (`src/app/settings_view.rs:684`).
  - PNG decoding checks header dimensions (`<= 40_000_000` pixels, non-zero) via `into_dimensions()` before allocation and sets `image::Limits::max_alloc` to 160 MB (`src/media.rs:36-61`).
  - Clipboard text length is bounded by `MAX_TEXT_LENGTH = 500_000` (`src/monitor.rs:12,154`), secret scanning is bounded by `MAX_SCAN_BYTES = 65_536` and `MAX_ENV_SCAN_LINES = 200` (`src/secret.rs:1-2,26,198`), and database growth is bounded by `prune_in_tx` on every insert (`src/db.rs:493,537,599`).
  - No findings in this category.
- **6. Logging, Telemetry, and Observability**:
  - Release builds compile `log_captured_text` to a no-op (`src/monitor.rs:252-253`); debug builds print only classification, character count, and byte length with `preview withheld` (`src/monitor.rs:238-250`).
  - `app.log` is created with `0600` permissions inside the `0700` data directory, rotated at 1 MiB (`MAX_LOG_BYTES = 1024 * 1024`), uses `try_lock()` when panicking to avoid self-deadlock, and returns early under `cfg!(test)` so unit-test failure simulations do not pollute user logs (`src/logging.rs:7-76`).
  - Findings L1, L2.
- **7. Cryptographic Choices**:
  - Key generation and password generation use OS CSPRNG via `rand::rng()` (`src/crypto.rs:303-309`, `src/password.rs:216,240`).
  - Database encryption uses bundled SQLCipher (`rusqlite` `0.42` with `bundled-sqlcipher`, `Cargo.toml:67`), key applied via `conn.pragma_update(None, "key", encryption_key)` and verified immediately against `sqlite_master` (`src/db.rs:197-203`).
  - `[Inferred]` SQLCipher default KDF/cipher parameters (PBKDF2-HMAC-SHA512, 256,000 iterations, AES-256-CBC in SQLCipher 4) are used without custom pragma overrides, which matches current SQLCipher 4 best practices.
  - No findings in this category.
- **8. CI/CD and Release Trust**:
  - Every GitHub Action in `.github/workflows/{ci,release,sonarcloud}.yml` is pinned to a full 40-char commit SHA with least-privilege `permissions: contents: read` at workflow level and job-scoped write permissions only for release publishing/attestation (`release.yml:25-28,179-182`).
  - All CI `clippy`, `test`, `build`, and `llvm-cov` commands use `--locked` (`ci.yml:38,69,72,87`; `sonarcloud.yml:38`), with no `continue-on-error`.
  - Sonar scan runs unconditionally on `push` to `main` (`if: github.event_name == 'push' || env.SONAR_TOKEN != ''`, `sonarcloud.yml:48`), failing loudly if the token is missing on `main`.
  - Release workflow verifies Microsoft Authenticode signature on `vc_redist.x64.exe` (`release.yml:112-115`), derives `OPENSSL_RUNTIME_DLL` from `libcrypto.lib` and asserts import in `CipherClipboard.exe` (`scripts/windows/setup-openssl-ci.ps1:67-84`, `release.yml:84-101`), computes SHA-256 checksums (`release.yml:139-154,241-249`), and publishes SLSA build provenance attestations (`release.yml:156-161,250-254`).
  - `deny.toml` enforces `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`, and a single scoped advisory ignore (`RUSTSEC-2026-0543` citing both `db.rs` and `logging.rs`, `deny.toml:13-16`).
  - No findings in this category.
