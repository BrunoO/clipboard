# Error Handling Review - 2026-11-21

## Executive Summary
- **Health Score**: 7/10
- **Critical Issues**: 0
- **High Issues**: 2
- **Total Findings**: 9
- **Estimated Remediation Effort**: 18 hours
- **Evidence Commands Run**: `grep -RInE 'unwrap\(|expect\(|panic!|unreachable!|let _ =|map_err|with_context|\.context\(' src` (parent-captured `error-inventory.log`, 319 lines, read verbatim); `grep -RIn 'Result<' src`; `grep -RInE 'transaction\(|wal_checkpoint|secure_delete' src`; direct source reads of `src/db.rs`, `src/app/mod.rs`, `src/app/settings_view.rs`, `src/settings.rs`, `src/main.rs`, `src/monitor.rs`, `src/hotkey.rs`, `src/crypto.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/logging.rs`, `src/code.rs`. No cargo/clippy/test command was run for this report — build and static-analysis results are out of scope and belong to the separate static-analysis report.

## Findings

### Critical
No findings in this category.

### High

**EH-1 — Full-history clear returns success while VACUUM/WAL errors are swallowed, and the UI confirms success unconditionally**
1. **Failure path**: User confirms "Clear All"; `Database::clear()` deletes rows, then fails to compact or truncate the WAL.
2. **Severity**: High
3. **Evidence** — `src/db.rs:622-632`:
   ```rust
       pub(crate) fn clear(&self) -> Result<(), rusqlite::Error> {
           let conn = self.lock_conn();
           conn.execute("DELETE FROM clipboard_history", [])?;
           if let Err(e) = conn.execute_batch("VACUUM;") {
               eprintln!("⚠️ Failed to VACUUM database after clear: {e}");
           }
           if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
               eprintln!("⚠️ Failed to truncate WAL after clear: {e}");
           }
           Ok(())
       }
   ```
   `src/app/mod.rs:1281-1286`:
   ```rust
                       if ui.add_sized([btn_width, btn_height], clear_btn).clicked() {
                           let _ = self.db.clear();
                           self.texture_cache.clear();
                           self.refresh_items();
                           self.show_clear_confirm_dialog = false;
                           self.show_toast(&format!("{} History cleared.", icons::CHECK));
                       }
   ```
4. **Problem**: Both compaction errors go only to stderr; `clear()` returns `Ok(())`; the caller discards the `Result` and shows a success toast. `docs/SECURITY_LESSONS.md:19` requires "Compact and truncate WAL on full clear" so deleted secrets do not linger on disk; the error-handling prompt §5 requires WAL checkpoint failure to be "surfaced or retried, not silently ignored." Context is lost at three layers.
5. **Expected behavior**: On compaction failure, retry or tell the user that history rows were removed but encrypted data may remain in the WAL, and that data safety requires a restart/manual step.
6. **Recommendation**: Return a typed error from `clear()` when either step fails (aggregate with `thiserror` or `anyhow::Context`), propagate with `?`, and branch the UI toast on the result; log with operation context via `logging::log_message`.

**EH-2 — Settings persistence failures are silently discarded at ten UI call sites**
1. **Failure path**: Any settings toggle, custom-hotkey clear, modal dismissal, or auto-clear interval change.
2. **Severity**: High
3. **Evidence** — `src/settings.rs:565-568`:
   ```rust
       pub(crate) fn save(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
           let json = serde_json::to_string(self)?;
           db.set_setting(Self::SETTINGS_KEY, &json)?;
           Ok(())
       }
   ```
   Call sites (verbatim `let _ = self.settings.save(&self.db);`): `src/app/settings_view.rs:227`, `:338`, `:347`, `:359`, `:371`, `:477`, `:493`, `:534`; `src/app/mod.rs:1307`, `:1360`, `:1370`. Representative source context — `src/app/settings_view.rs:337-338`:
   ```rust
                   self.settings.hide_on_copy = hide_on_copy;
                   let _ = self.settings.save(&self.db);
   ```
   `src/app/settings_view.rs:476-477`:
   ```rust
                   self.db.set_ignore_concealed(self.settings.ignore_concealed);
                   let _ = self.settings.save(&self.db);
   ```
   `src/app/mod.rs:1305-1307`:
   ```rust
               self.show_keyring_explanation = false;
               self.settings.keyring_explanation_shown = true;
               let _ = self.settings.save(&self.db);
   ```
4. **Problem**: If `serde_json` or `Database::set_setting` fails, no log, no toast, and no retry; in-memory state changes while persisted state does not, so a restart silently reverts the user's choice. Two of these sites (`ignore_concealed`, `auto_detect_secrets`) change privacy behavior, making silent loss a security-relevant inconsistency.
5. **Expected behavior**: The failure is logged with context and surfaced (at minimum as a warning toast), or the UI rolls back the in-memory change.
6. **Recommendation**: Add one `ClipboardApp::persist_settings()` helper that calls `save()`, logs via `logging::log_message("ERROR", ...)` on failure, and calls `show_toast()`; replace all ten `let _ =` sites with it.

### Medium

**EH-3 — In-transaction prune failure is swallowed while the transaction commits**
1. **Failure path**: `Database::insert`, `insert_secret`, or `insert_image` with `max_history > 0` hits a SQLite error during the prune step.
2. **Severity**: Medium
3. **Evidence** — `src/db.rs:304-317`:
   ```rust
       fn prune_in_tx(tx: &rusqlite::Transaction<'_>, max_limit: usize) {
           if max_limit > 0 {
               let limit_i64 = i64::try_from(max_limit).unwrap_or(i64::MAX);
               if let Err(e) = tx.execute(
                   "DELETE FROM clipboard_history
                    WHERE id NOT IN (
                        SELECT id FROM clipboard_history ORDER BY id DESC LIMIT ?1
                    )",
                   params![limit_i64],
               ) {
                   eprintln!("⚠️ Failed to prune clipboard history: {e}");
               }
           }
       }
   ```
   and `src/db.rs:369-371`:
   ```rust
           // 3. Automatically prune older entries beyond max_history
           Self::prune_in_tx(&tx, max_limit);
           tx.commit()?;
   ```
4. **Problem**: The insert commits successfully while the history cap silently fails; `prune_history()` returns `Result<usize, rusqlite::Error>` (`src/db.rs:263`) elsewhere, so the contract is inconsistent.
5. **Expected behavior**: A prune failure is either propagated (rolling back the insert) or explicitly recorded as a non-fatal condition with a log and a retry path.
6. **Recommendation**: Change `prune_in_tx` to return `Result<(), rusqlite::Error>` and use `?` at the three call sites (`src/db.rs:370`, `:410`, `:468`).

**EH-4 — Schema migration failures are logged to stderr but never surfaced**
1. **Failure path**: `Database::open` on an older schema where an `ALTER TABLE` fails for a reason other than "duplicate column name".
2. **Severity**: Medium
3. **Evidence** — `src/db.rs:177-182`:
   ```rust
               if let Err(e) = conn.execute(sql, []) {
                   let msg = e.to_string();
                   if !msg.contains("duplicate column name") {
                       eprintln!("⚠️ Schema migration failed for `{sql}`: {e}");
                   }
               }
   ```
   The caller adds no context: `src/main.rs:107` `let (db, key_storage_mode) = open_encrypted_database(&data_dir)?;`
4. **Problem**: String matching on the error message is fragile, and a real migration failure leaves missing columns while `Database::open` still returns `Ok`. Later queries fail with confusing `rusqlite` errors far from the cause.
5. **Expected behavior**: Only the expected duplicate-column condition is ignored; other migration failures abort startup with an actionable message and context.
6. **Recommendation**: Inspect the SQLite extended error code (or use `IF NOT EXISTS`-style migration statements) and propagate unexpected errors with `.context()` at `Database::open`.

**EH-5 — Inconsistent poison recovery on the macOS repaint-context mutex silently loses state**
1. **Failure path**: Any panic while `REPAINT_CTX` (`src/platform/macos.rs:11`) is locked; later Dock-reopen or repaint requests.
2. **Severity**: Medium
3. **Evidence** — `src/platform/macos.rs:13-16`:
   ```rust
   pub(super) fn set_repaint_context(ctx: &eframe::egui::Context) {
       if let Ok(mut lock) = REPAINT_CTX.lock() {
           *lock = Some(ctx.clone());
       }
   }
   ```
   `src/platform/macos.rs:19-25`:
   ```rust
   fn request_repaint() {
       if let Ok(lock) = REPAINT_CTX.lock() {
           if let Some(ctx) = lock.as_ref() {
               ctx.request_repaint();
           }
       }
   }
   ```
   Contrast with `src/platform/macos.rs:202-204`:
   ```rust
       let mut lock = REPAINT_CTX
           .lock()
           .unwrap_or_else(std::sync::PoisonError::into_inner);
   ```
4. **Problem**: After poisoning, the context is never stored and repaints silently stop, while teardown uses `into_inner`. `docs/CONCURRENCY_AND_PERF_LESSONS.md:26` requires "Recover from poisoned mutexes consistently". The codebase already demonstrates the correct pattern in `src/db.rs:195-208`, `src/platform/windows.rs:51-70`, `src/logging.rs:26-34`, `src/crypto.rs:233-246`.
5. **Expected behavior**: Poison is recovered deterministically at all three sites, with a diagnostic log.
6. **Recommendation**: Use `unwrap_or_else(std::sync::PoisonError::into_inner)` in `set_repaint_context` and `request_repaint`.

**EH-6 — Startup history prune result discarded without a reason**
1. **Failure path**: Application startup with a corrupt/locked DB during `prune_history`.
2. **Severity**: Medium
3. **Evidence** — `src/main.rs:115`:
   ```rust
       let _ = db.prune_history(settings.max_history);
   ```
4. **Problem**: No logging and no reason comment; the error-handling prompt §2 requires an explicit reason for a discarded `Result`. Oversized history persists silently on a failure that may indicate a real DB problem.
5. **Expected behavior**: Log a warning with the error so the condition is diagnosable from `app.log`.
6. **Recommendation**: `if let Err(e) = db.prune_history(settings.max_history) { logging::log_message("WARN", &format!("Startup history prune failed: {e}")); }`.

### Low

**EH-7 — Failed row deletion leaves the UI claiming a removal that did not happen**
1. **Failure path**: User deletes a history item; `Database::delete` returns `Err`.
2. **Severity**: Low
3. **Evidence** — `src/app/mod.rs:735-739`:
   ```rust
               if let Some(item) = self.items.get(item_idx) {
                   let id = item.id;
                   let _ = self.db.delete(id);
                   self.texture_cache.remove(id);
                   self.refresh_items();
   ```
4. **Problem**: The cache is evicted and the list refreshed regardless of the DB outcome, so a failed delete looks successful until the next restart.
5. **Expected behavior**: Surface the failure and avoid evicting the texture cache when the row is still present.
6. **Recommendation**: Match on `self.db.delete(id)` and toast on `Err`; only remove the cached texture on success.

**EH-8 — Coalesced notification `try_send` results discarded without a recorded reason**
1. **Failure path**: Background monitor produces a notification while the bounded notification channel is full or disconnected.
2. **Severity**: Low
3. **Evidence** — `src/monitor.rs:165-169`:
   ```rust
           Ok(Some(id)) => {
               log_captured_text(id, &text);
               let _ = notify_tx.try_send(());
               ctx.request_repaint();
           }
   ```
   and `src/monitor.rs:219-226`, including `let _ = notify_tx.try_send(());` at `:225`. `notify_tx` is a `SyncSender<()>` (`src/monitor.rs:41`).
4. **Problem**: Discarding is actually correct for a coalescing bounded channel (`docs/CONCURRENCY_AND_PERF_LESSONS.md:36` recommends `sync_channel(1)` + `try_send(())` for coalesced wakeups), but the prompt §2 requires an explicit reason and none is written.
5. **Expected behavior**: Reason is discoverable at the call site.
6. **Recommendation**: Add `// Best-effort coalesced wakeup; a full or closed channel needs no action.` at both sites.

**EH-9 — DPAPI failures collapsed into `String` at the FFI boundary**
1. **Failure path**: `dpapi_protect`/`dpapi_unprotect` fail on Windows.
2. **Severity**: Low
3. **Evidence** — `src/platform/windows.rs:671` and `:701-702`:
   ```rust
   pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
   ```
   ```rust
       if ok == 0 || out_blob.pbData.is_null() {
           return Err("Windows DPAPI CryptProtectData failed".to_string());
       }
   ```
   `src/platform/windows.rs:715` declares `Result<zeroize::Zeroizing<Vec<u8>>, String>` and `:747-748` returns another string error. The caller erases it further: `src/crypto.rs:176-177` `crate::platform::windows::dpapi_unprotect(&ciphertext).map_err(std::io::Error::other)?;`.
4. **Problem**: The OS status and which stage failed are lost; callers cannot distinguish a transient keyring/DPAPI failure from a malformed key file. The status codes themselves *are* checked (`ok == 0`, null checks), so this is a type-design gap, not a missing check.
5. **Expected behavior**: A typed error preserving the failing operation and status, with context added at the crypto boundary.
6. **Recommendation**: Define a small `thiserror` enum for DPAPI operations and map it into `io::Error` with `std::io::Error::new(ErrorKind::Other, err)` at `src/crypto.rs:176`.

## Quick Wins
1. **EH-1**: Make `Database::clear()` return an error when VACUUM/WAL truncation fails and branch the success toast at `src/app/mod.rs:1286` (~2 h) — highest security value.
2. **EH-2**: Introduce `persist_settings()` and replace the ten identical `let _ = self.settings.save(&self.db)` lines (~2 h).
3. **EH-5**: Use `PoisonError::into_inner` in `src/platform/macos.rs:14` and `:20` (~0.5 h).
4. **EH-8**: Add one-line reasons to the two `try_send` discards in `src/monitor.rs:168` and `:225` (~0.25 h).
5. **EH-6**: Replace `let _ = db.prune_history(...)` at `src/main.rs:115` with a logged warning (~0.25 h).

## Recommended Actions
1. **EH-1 and EH-2 together**: Centralize UI-facing persistence results (settings + clear) in `ClipboardApp` so every mutating action has one logging/toast path; this closes the silent-failure class in one change.
2. **EH-3 and EH-4**: Make SQLite error propagation consistent — propagate unexpected migration/prune errors, keep only explicitly expected conditions ignored — then add focused tests for a simulated prune failure and a non-duplicate-column migration failure.
3. **EH-5**: Unify poison recovery across `macos.rs`, `db.rs`, `windows.rs`, `logging.rs` and the `mock_keyring` to one documented policy.
4. **EH-9**: Replace `String` errors at the DPAPI boundary as part of the typed-error work in `TECH_DEBT_REVIEW_2026-11-21.md` (TD-3).
5. **EH-7 and EH-8**: Fold into the next UI/monitor cleanup PR.

## Coverage
- **1. Error Type Design** — Finding EH-9. `Result<_, Box<dyn std::error::Error>>` at `src/db.rs:127`, `src/settings.rs:565`, `src/hotkey.rs:87`, `src/tray.rs:63`, `src/media.rs:11,30` and `src/main.rs:75,257` is cross-referenced with TD-3 in the tech-debt report rather than duplicated.
- **2. Propagation and Context** — Findings EH-1, EH-3, EH-4, EH-6. Positive evidence: `src/main.rs:34-40` handles the top-level `run()` error with stderr, `logging::log_message`, a Windows error dialog, and `std::process::exit(1)`.
- **3. Panic and Abort Discipline** — No findings in this category from the reviewed files. `src/code.rs:422` begins `#[cfg(test)] mod tests`, so the flagged `src/code.rs:431` (`let formatted = format_json(json).expect("Prettify failed");`) is test code, not a production panic path. No production `unwrap`/`expect` was found in the non-test paths read for this report; overflow-sensitive integer conversions at FFI boundaries use checked conversions (e.g. `src/platform/windows.rs:672-675`, `:718-721`).
- **4. OS/FFI Boundaries** — No separate findings; DPAPI type design is EH-9 and repository-wide `SAFETY` documentation is covered in `UNSAFE_FFI_REVIEW_2026-11-21.md`. Positive evidence: key material fail-closed when a DB already exists (`src/crypto.rs:101-104`), OS status checked before dependent calls (`src/platform/windows.rs:701`, `:747`), transient clipboard failures retried (`src/monitor.rs:121-137`), clipboard-occupied handled as retryable (`src/monitor.rs:148`, `:188`), and DPAPI plaintext zeroized before `LocalFree` (`src/platform/windows.rs:752-757`).
- **5. SQLite / Transaction Errors** — Findings EH-1, EH-3, EH-4. Positive evidence: multi-statement writes use one transaction (`src/db.rs:333`, `:383`, `:432`), `LIKE` wildcards escaped with `ESCAPE '\'` (`src/db.rs:491-498`, `:520`), `PRAGMA secure_delete = ON` (`src/db.rs:149`), and `lock_conn`/`lock_transient` recover poison explicitly (`src/db.rs:195-208`).
- **6. Synchronization and Poisoning** — Finding EH-5. Positive evidence: consistent `PoisonError::into_inner` recovery in `src/db.rs:196-207`, `src/platform/windows.rs:51-70`, `src/crypto.rs:233-246`, and panic-safe `try_lock` in `src/logging.rs:26-34`.
- **7. User-Facing Errors** — Findings EH-1, EH-2, EH-7. Positive evidence: actionable missing-key recovery message in `src/crypto.rs:144-163` and fail-closed behavior at `src/crypto.rs:102-104`; no clipboard contents or key material are included in any quoted error path in this report.
