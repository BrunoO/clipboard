# Error Handling Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 5
- **Estimated Remediation Effort**: 7 hours
- **Evidence Commands Run**: `grep -RInE 'unwrap\(|expect\(|panic!|unreachable!|let _ =|map_err|with_context|\.context\(' src`; `grep -RIn 'Result<' src | head -200`; `grep -RInE 'transaction\(|wal_checkpoint|secure_delete' src`; direct source reads of `src/db.rs`, `src/app/mod.rs`, `src/app/history_view.rs`, `src/app/settings_view.rs`, `src/app/generator_view.rs`, `src/settings.rs`, `src/main.rs`, `src/monitor.rs`, `src/hotkey.rs`, `src/tray.rs`, `src/crypto.rs`, `src/logging.rs`, `src/instance.rs`, `src/media.rs`, `src/platform/mod.rs`, `src/platform/fs_security.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/platform/linux.rs`.

## Findings

**Note on Resolved Prior Findings**: All High and Medium error-handling issues from the earlier `2026-11-21` review (`EH-1` `Database::clear()` `ClearError` + UI toast branching at `src/db.rs:802-833` and `src/app/mod.rs:1387-1409`, `EH-2` `ClipboardApp::persist_settings()` helper replacing discarded `save()` results, `EH-3` `prune_in_tx` returning `Result<(), rusqlite::Error>` propagated with `?` at `src/db.rs:408-437,493,537,599`, `EH-4` `Database::open` aborting on non-duplicate-column schema migration errors at `src/db.rs:172-180,240-251`, `EH-5` `lock_or_recover` on macOS `REPAINT_CTX` at `src/platform/macos.rs:14-33,216`, `EH-6` logged startup prune warning at `src/main.rs:116-118`, `EH-7` `report_db_write_failure` on row deletion failure at `src/app/mod.rs:840-843` and `src/app/history_view.rs:183-186`, and `EH-8` documented `try_send` comments in `src/monitor.rs:168,226` and `src/hotkey.rs:114`) were verified in source as fixed.

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**EH-1 — Three settings-save sites in `src/app/settings_view.rs` bypass `persist_settings()` and print only to `stderr` before showing a success toast**
1. **Failure path**: User changes the theme, selects a hotkey preset, or applies a custom hotkey while the database write (`self.settings.save(&self.db)`) fails.
2. **Severity**: Medium
3. **Evidence** — `src/app/settings_view.rs:78-90`:
   ```rust
                               self.settings.theme = selected_theme;
                               if let Err(e) = self.settings.save(&self.db) {
                                   eprintln!("Failed to save settings: {e}");
                               }
                               let new_is_dark = selected_theme.resolve(ui.ctx());
                               crate::theme::setup_theme(ui.ctx(), new_is_dark);
                               self.current_is_dark = Some(new_is_dark);
                               ui.ctx().request_repaint();
                               self.show_toast(&format!(
                                   "{} Theme changed to {}",
                                   icons::CHECK,
                                   selected_theme.display_name()
                               ));
   ```
   `src/app/settings_view.rs:138-147`:
   ```rust
               self.settings.hotkey = preset;
               if let Err(e) = self.settings.save(&self.db) {
                   eprintln!("Failed to save settings: {e}");
               }
               if self.update_hotkeys_from_settings() {
                   self.show_toast(&format!(
                       "{} Shortcut set to {}",
                       icons::CHECK,
                       preset.badge()
                   ));
   ```
   `src/app/settings_view.rs:302-311`:
   ```rust
           if let Some(parsed) = crate::settings::ParsedHotkey::parse(&self.pending_custom_hotkey) {
               self.settings.custom_hotkey = parsed.format();
               if let Err(e) = self.settings.save(&self.db) {
                   eprintln!("Failed to save settings: {e}");
               }
               if self.update_hotkeys_from_settings() {
                   self.show_toast(&format!(
                       "{} Custom shortcut set to {}",
                       icons::CHECK,
                       self.settings.custom_hotkey
                   ));
   ```
   Contrast with `ClipboardApp::persist_settings()` at `src/app/mod.rs:351-359`:
   ```rust
       fn persist_settings(&mut self) {
           if let Err(error) = self.settings.save(&self.db) {
               crate::logging::log_message("ERROR", &format!("Failed to save settings: {error}"));
               self.show_error_toast(&format!(
                   "{} Failed to save settings. Changes will be lost on restart.",
                   icons::WARNING
               ));
           }
       }
   ```
4. **Problem**: At these three sites in `src/app/settings_view.rs`, a persistence failure is neither written to `app.log` via `logging::log_message` nor surfaced to the user (in release builds on Windows with `#![windows_subsystem = "windows"]`, `eprintln!` is invisible), and the immediately following `self.show_toast(...)` displays a green success toast claiming the setting was saved.
5. **Expected behavior**: All settings mutations should route through `persist_settings()` (returning `bool` so callers only show their success toast when persistence succeeded, or letting `persist_settings()` override the toast with the error toast and log to `app.log`).
6. **Recommendation**: Have `persist_settings(&mut self) -> bool` return `true` on `Ok(())` and `false` on `Err` (logging via `logging::log_message("ERROR", ...)` and showing the error toast), and call `self.persist_settings()` at `src/app/settings_view.rs:79`, `:139`, and `:303`.

**EH-2 — `refresh_items()` and `render_max_history_selector()` silently swallow SQLite errors during runtime history queries and pruning**
1. **Failure path**: `Database::get_recent` fails during `ClipboardApp::refresh_items()` (`src/app/mod.rs:316-329`), or `Database::prune_history` fails when the user lowers the history size limit (`src/app/settings_view.rs:430`).
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:322-325`:
   ```rust
           self.items = self
               .db
               .get_recent(self.settings.max_history, filter)
               .unwrap_or_default();
   ```
   and `src/app/settings_view.rs:428-432`:
   ```rust
                               self.settings.max_history = limit;
                               self.db.set_max_history(limit);
                               let _ = self.db.prune_history(limit);
                               self.persist_settings();
                               self.refresh_items();
   ```
4. **Problem**: Whereas startup in `ClipboardApp::new` (`src/app/mod.rs:242-253`) explicitly matches on `db.get_recent(...)` and shows an error toast on failure, `refresh_items()` uses `.unwrap_or_default()`, which silently replaces the UI list with an empty history view ("No clipboard history found.") and evicts `self.texture_cache` (`:327-328`) without logging to `app.log` or notifying the user. Similarly, `let _ = self.db.prune_history(limit);` at `src/app/settings_view.rs:430` discards prune errors when the user reduces the history cap.
5. **Expected behavior**: If `db.get_recent` or `db.prune_history` fails at runtime, the error is logged to `app.log` and surfaced via `report_db_write_failure` / `show_error_toast`, and `refresh_items()` avoids wiping the in-memory list and texture cache on transient read errors.
6. **Recommendation**: Match on `self.db.get_recent(...)` in `refresh_items()` (logging `"ERROR"` via `logging::log_message`) and match on `self.db.prune_history(limit)` in `render_max_history_selector()` (calling `self.report_db_write_failure("prune history", &error)` on `Err`).

**EH-3 — Failed database insert of a generated password in `copy_generated_password` is only printed to `stderr` while the UI shows "Copied & saved password as Secret!"**
1. **Failure path**: User clicks "Copy" or "Copy & Paste" in the Password Generator with `skip_history == false`, and `self.db.insert_secret(&secret)` returns `Err`.
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:592-605`:
   ```rust
               if !self.password_gen.skip_history {
                   if let Err(e) = self.db.insert_secret(&secret) {
                       eprintln!("Failed to save generated password to database: {e}");
                   } else {
                       self.refresh_items();
                   }
               }
               let msg = if self.password_gen.skip_history {
                   format!("{} Copied password (not saved to history)", icons::CHECK)
               } else {
                   format!("{} Copied & saved password as Secret!", icons::LOCK)
               };
               self.finish_copy(ctx, &msg, dismiss);
   ```
4. **Problem**: If `self.db.insert_secret(&secret)` fails, the error is not recorded in `app.log`, and the UI still displays `"Copied & saved password as Secret!"`. If the user relies on the password having been persisted in encrypted history (especially since `schedule_secret_clipboard_clear` will wipe it from the OS clipboard after 30 seconds), the generated password can be permanently lost when the clipboard auto-clears.
5. **Expected behavior**: Log the database failure via `logging::log_message("ERROR", ...)` and show a warning/error toast stating that the password was copied to the clipboard but could **not** be saved to history (and do not auto-clear or clearly warn before auto-clearing).
6. **Recommendation**: Track whether `insert_secret` succeeded; on `Err(e)`, call `crate::logging::log_message("ERROR", &format!("Failed to save generated password to database: {e}"))`, show a warning toast (`"Copied password to clipboard, but failed to save to history"`), and avoid misleading the user that it was saved.

### Low

**EH-4 — `AppSettings::load` silently falls back to defaults without logging when `app_config` JSON is corrupted or `get_setting` fails**
1. **Failure path**: Startup `AppSettings::load(&db)` encounters a SQLite read error or malformed JSON in `app_settings`.
2. **Severity**: Low
3. **Evidence** — `src/settings.rs:538-545`:
   ```rust
       pub(crate) fn load(db: &Database) -> Self {
           if let Ok(Some(json)) = db.get_setting(Self::SETTINGS_KEY) {
               if let Ok(settings) = serde_json::from_str(&json) {
                   return settings;
               }
           }
           Self::default()
       }
   ```
4. **Problem**: Falling back to `Self::default()` is resilient, but discarding both `db.get_setting` errors and `serde_json::from_str` parse errors without logging to `app.log` leaves zero diagnostic trace when a user's preferences unexpectedly reset.
5. **Expected behavior**: Log a `WARN` entry via `crate::logging::log_message` when `db.get_setting` returns `Err` or `serde_json::from_str` fails, then return `Self::default()`.
6. **Recommendation**: Replace the nested `if let Ok(...)` in `AppSettings::load` with `match` arms that log a warning (without logging raw setting values) before returning `Self::default()`.

**EH-5 — DPAPI and internal module error returns use `String` / `Box<dyn Error>` without OS error codes**
1. **Failure path**: `dpapi_protect` or `dpapi_unprotect` fails on Windows (`src/platform/windows.rs:733-831`), or `Database::open` / `AppSettings::save` fails.
2. **Severity**: Low
3. **Evidence** — `src/platform/windows.rs:764-766` and `:817-819`:
   ```rust
       if ok == 0 || out_blob.pbData.is_null() {
           return Err("Windows DPAPI CryptProtectData failed".to_string());
       }
   ```
   ```rust
       if ok == 0 || out_blob.pbData.is_null() {
           return Err("Windows DPAPI CryptUnprotectData failed".to_string());
       }
   ```
   and `src/platform/fs_security.rs:52-53`:
   ```rust
           let plain_bytes =
               super::windows::dpapi_unprotect(&ciphertext).map_err(std::io::Error::other)?;
   ```
4. **Problem**: When `CryptProtectData` or `CryptUnprotectData` returns `0`, the static string `"Windows DPAPI CryptUnprotectData failed"` does not capture `std::io::Error::last_os_error()`, making it harder to diagnose whether DPAPI failed due to a profile/credential state (`NTE_BAD_KEY_STATE`) or corrupt ciphertext (`ERROR_INVALID_DATA`). Cross-referenced with TD-3 in `TECH_DEBT_REVIEW_2026-11-21-b.md`.
5. **Expected behavior**: Capture `std::io::Error::last_os_error()` immediately after `CryptProtectData` / `CryptUnprotectData` fails and return a typed error or `std::io::Error` directly.
6. **Recommendation**: Change `dpapi_protect` and `dpapi_unprotect` to return `Result<..., std::io::Error>` using `std::io::Error::last_os_error()` when `ok == 0`, eliminating `.map_err(std::io::Error::other)` in `src/platform/fs_security.rs:53,94`.

## Quick Wins
1. **EH-1**: Replace the three `if let Err(e) = self.settings.save(&self.db) { eprintln!(...); }` blocks in `src/app/settings_view.rs:79,139,303` with `self.persist_settings()` (~0.5 h).
2. **EH-3**: Log `insert_secret` failures to `app.log` and show an accurate warning toast in `src/app/mod.rs:593-604` (~0.5 h).
3. **EH-2**: Log and toast errors from `db.prune_history(limit)` in `src/app/settings_view.rs:430` and `db.get_recent(...)` in `src/app/mod.rs:322` (~1 h).
4. **EH-4**: Log a warning in `AppSettings::load` (`src/settings.rs:538-545`) when `get_setting` or `serde_json::from_str` fails (~0.25 h).

## Recommended Actions
1. **EH-3 (data-loss prevention)**: Ensure `copy_generated_password` never claims a generated password was saved to history when `db.insert_secret` failed.
2. **EH-1 & EH-2**: Route the remaining three `settings.save` call sites and runtime `prune_history` / `get_recent` calls through `persist_settings()` and `report_db_write_failure()`.
3. **EH-4 & EH-5**: Add diagnostic logging to `AppSettings::load` fallback and include `std::io::Error::last_os_error()` in Windows DPAPI error paths.

## Coverage
- **1. Error Type Design** — Finding EH-5 (and cross-referenced with TD-3). Positive evidence: `ClearError` (`src/db.rs:105-149`) models `Delete` vs. `Compaction { vacuum, wal_checkpoint }` with full `Display` and `Error::source` support.
- **2. Propagation and Context** — Findings EH-1, EH-2, EH-3, EH-4. Positive evidence: `Database::open` now aborts startup with the exact failing SQL on unexpected schema migration failures (`src/db.rs:240-251`), `prune_in_tx` propagates errors via `?` (`src/db.rs:408-437,493,537,599`), and `main::main` logs fatal startup errors to `stderr`, `app.log`, and the Windows error dialog before exiting (`src/main.rs:34-40`).
- **3. Panic and Abort Discipline** — No findings in this category. Every `unwrap()`, `expect()`, and `panic!()` in `src/` was inspected and confirmed to reside inside `#[cfg(test)]` modules (`src/crypto.rs:225,345..462`, `src/color.rs:572..638`, `src/code.rs:431`, `src/settings.rs:576..712`, `src/instance.rs:77..99`, `src/media.rs:79..110`, `src/app/mod.rs:1870..2144`, `src/tray.rs:192`, `src/platform/mod.rs:740`, `src/platform/fs_security.rs:245..249`, `src/platform/macos.rs:471`, `src/db.rs:875..1490`). The panic hook (`src/main.rs:45-72`) uses non-blocking `try_lock()` in `logging::log_message` (`src/logging.rs:48-53`) when `std::thread::panicking()` is true, preventing re-entrant deadlock before `panic = "abort"` terminates the process.
- **4. OS/FFI Boundaries** — Finding EH-5. Positive evidence: `crypto::get_or_create_encryption_key_with_mode` fails closed when `db_exists` is true and the key is missing (`src/crypto.rs:128-130`); `KeyStorageMode::LocalFileFallback` and `LocalFileFallbackDegraded` surface key-storage degradation in the UI (`src/crypto.rs:14-21`, `src/app/settings_view.rs:539-568`); `init_clipboard_with_retry` (`src/monitor.rs:121-137`) and `ensure_clipboard_writer` (`src/app/mod.rs:489-499`) retry transient clipboard unavailability.
- **5. SQLite / Transaction Errors** — Finding EH-2. Positive evidence: `insert`, `insert_secret`, and `insert_image` wrap duplicate check, old-row deletion, insertion, and `prune_in_tx` in a single `conn.transaction()?` (`src/db.rs:456,510,563`); `Database::clear()` runs `DELETE`, `VACUUM;`, and `PRAGMA wal_checkpoint(TRUNCATE);` and returns `ClearError::Compaction` if either compaction step fails (`src/db.rs:802-833`).
- **6. Synchronization and Poisoning** — No findings in this category. All mutexes across the crate (`Database::lock_conn` and `lock_transient` at `src/db.rs:264-277`, `REPAINT_CTX` via `lock_or_recover` at `src/platform/macos.rs:14-22`, `PREVIOUS_WINDOW` at `src/platform/windows.rs:51-70`, `PasteWorker` at `src/platform/mod.rs:579-601`, `HotkeyService::target_ids` at `src/hotkey.rs:107-110,153-156`, `LOG_LOCK` at `src/logging.rs:48-56`, and `mock_keyring::MOCK_KEYRING` at `src/crypto.rs:219-232`) consistently recover via `PoisonError::into_inner`.
- **7. User-Facing Errors** — Findings EH-1, EH-2, EH-3. Positive evidence: `platform::fs_security::missing_key_error_message` (`src/platform/fs_security.rs:14-34`) provides OS-specific recovery instructions (PowerShell on Windows, `rm -rf` on Unix); `logging::log_message` stays silent during `cfg!(test)` (`src/logging.rs:34-36`) and creates `app.log` with `0o600` permissions on Unix (`src/logging.rs:67-71`); no clipboard contents or secrets are logged in release builds.
