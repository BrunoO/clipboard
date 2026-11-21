# Technical Debt Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 6
- **Estimated Remediation Effort**: 14 hours
- **Evidence Commands Run**: `cargo metadata --no-deps --format-version 1`; `cargo tree -d`; `grep -RInE 'TODO|FIXME|HACK|XXX|#\[allow|#!\[allow' src Cargo.toml .github 2>/dev/null`; `wc -l src/*.rs src/app/*.rs src/platform/*.rs`; `grep -RIn 'cfg(' src --exclude-dir=platform`; direct source reads of `Cargo.toml`, `build.rs`, `src/main.rs`, `src/app/mod.rs`, `src/app/history_view.rs`, `src/app/settings_view.rs`, `src/app/generator_view.rs`, `src/db.rs`, `src/crypto.rs`, `src/settings.rs`, `src/hotkey.rs`, `src/tray.rs`, `src/logging.rs`, `src/instance.rs`, `src/media.rs`, `src/monitor.rs`, `src/password.rs`, `src/secret.rs`, `src/theme.rs`, `src/color.rs`, `src/code.rs`, `src/platform/mod.rs`, `src/platform/fs_security.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/platform/linux.rs`, `AGENTS.md`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`, `docs/CONCURRENCY_AND_PERF_LESSONS.md`.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**TD-1 — Mixed-concern god struct `ClipboardApp` with boolean flag sprawl**
1. **Title**: `ClipboardApp` aggregates database handles, inter-thread channels, tray/hotkey services, monitor lifecycle, and 20+ UI state fields under `#[allow(clippy::struct_excessive_bools)]` and `#[allow(clippy::too_many_arguments)]`.
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:173-216` and `src/app/mod.rs:219-232`:
   ```rust
   #[allow(clippy::struct_excessive_bools)]
   pub(crate) struct ClipboardApp {
       db: Database,
       key_storage_mode: crate::crypto::KeyStorageMode,
       search_query: String,
       content_filter: ContentFilter,
       items: Vec<ClipboardItem>,
       filter_counts: FilterCounts,
       clipboard_writer: Option<Clipboard>,
       toast: Option<Toast>,
       pending_toast: Option<(String, ToastSeverity)>,
       last_query_cached: String,
       notify_rx: Receiver<()>,
       hotkey_rx: Receiver<()>,
       hotkey_service: Option<HotkeyService>,
       tray_rx: Receiver<TrayAction>,
       #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
       tray_service: Option<TrayService>,
       texture_cache: ImageTextureCache,
       is_visible: bool,
       has_been_focused: bool,
       was_inactive_since_hidden: bool,
       selected_index: usize,
       focus_search_input: bool,
       last_toggle_instant: Option<Instant>,
       settings: AppSettings,
       active_view: ActiveView,
       password_gen: crate::password::PasswordGeneratorState,
       revealed_secrets: HashSet<i64>,
       pending_clipboard_clear: Option<(zeroize::Zeroizing<String>, Instant)>,
       show_clear_confirm_dialog: bool,
       scroll_to_selected: bool,
       current_is_dark: Option<bool>,
       title_bar_dark_mode: Option<bool>,
       monitor_shutdown_tx: Sender<()>,
       monitor_handle: Option<std::thread::JoinHandle<()>>,
       recording_custom_hotkey: bool,
       pending_custom_hotkey: String,
       is_quitting: bool,
       show_keyring_explanation: bool,
       last_drag_or_move_instant: Option<Instant>,
       last_outer_rect: Option<egui::Rect>,
       last_hide_instant: Option<Instant>,
   }
   ```
   ```rust
       #[allow(clippy::too_many_arguments)]
       pub(crate) fn new(
           _cc: &eframe::CreationContext<'_>,
           db: Database,
           key_storage_mode: crate::crypto::KeyStorageMode,
           notify_rx: Receiver<()>,
           hotkey_rx: Receiver<()>,
           hotkey_service: Option<HotkeyService>,
           tray_rx: Receiver<TrayAction>,
           tray_service: Option<TrayService>,
           settings: AppSettings,
           monitor_shutdown_tx: Sender<()>,
           monitor_handle: Option<std::thread::JoinHandle<()>>,
       ) -> Self {
   ```
4. **Why it is debt**: `src/app/mod.rs` is 2,149 lines (out of 14,329 total lines across `src/`) and acts as a monolithic integration point where view state, OS window-dismissal state, background thread ownership, and database access all live on a single struct. Suppressing `struct_excessive_bools` and `too_many_arguments` hides the growing state surface and makes isolated unit testing of view logic harder.
5. **Recommendation**: Group related fields into cohesive sub-structs (`WindowFocusState`, `AppChannels`, `BackgroundServices`) and pass a single `AppServices` bundle into `ClipboardApp::new`.
6. **Effort**: L (~5 hours) — mechanical field grouping across `src/app/*.rs`.

**TD-2 — Residual OS-conditional `#[cfg(target_os = ...)]` and `#[cfg(unix)]` blocks outside `src/platform/`**
1. **Title**: `src/tray.rs`, `src/main.rs`, `src/app/mod.rs`, `src/logging.rs`, and `src/theme.rs` still contain OS-conditional `#[cfg]` gates despite the `AGENTS.md` / `MULTIPLATFORM_LESSONS.md` confinement rule.
2. **Severity**: Medium
3. **Evidence** — `AGENTS.md:17` and `docs/MULTIPLATFORM_LESSONS.md:10`:
   ```markdown
   - **Confine platform `#[cfg(target_os = ...)]` / `#[cfg(unix)]` attributes to `src/platform/` (`mod.rs`, `fs_security.rs`, `linux.rs`, `macos.rs`, `windows.rs`).** Do not scatter OS-conditional `#[cfg]` blocks across application, UI, settings, hotkey, theme, or crypto modules; instead, expose OS-agnostic capability helpers, constants, or no-op fallback types from `src/platform/`.
   ```
   Verbatim output of `grep -RIn 'cfg(' src --exclude-dir=platform` (filtering out `#[cfg(test)]` and `#[cfg(debug_assertions)]`):
   ```text
   src/main.rs:37:        #[cfg(target_os = "windows")]
   src/main.rs:65:        #[cfg(all(target_os = "windows", not(debug_assertions)))]
   src/main.rs:90:            #[cfg(target_os = "windows")]
   src/main.rs:128:    #[cfg(target_os = "macos")]
   src/theme.rs:101:    #[cfg(target_os = "macos")]
   src/theme.rs:105:    #[cfg(target_os = "windows")]
   src/theme.rs:109:    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
   src/crypto.rs:354:        #[cfg(unix)]
   src/crypto.rs:362:        #[cfg(target_os = "windows")]
   src/logging.rs:67:    #[cfg(unix)]
   src/app/mod.rs:1517:    #[cfg(target_os = "macos")]
   src/app/mod.rs:1617:    #[cfg(target_os = "macos")]
   src/app/mod.rs:1714:    #[cfg(target_os = "macos")]
   src/tray.rs:18:    #[cfg(target_os = "windows")]
   src/tray.rs:39:        #[cfg(target_os = "windows")]
   src/tray.rs:53:        #[cfg(not(target_os = "windows"))]
   src/tray.rs:59:    #[cfg(target_os = "windows")]
   src/tray.rs:149:    #[cfg(target_os = "windows")]
   src/tray.rs:154:    #[cfg(not(target_os = "windows"))]
   src/tray.rs:161:#[cfg(target_os = "windows")]
   ```
   (Note: `src/theme.rs:101-109` is the `const fn platform_select` helper explicitly prescribed by `docs/MULTIPLATFORM_LESSONS.md:15`, and `src/crypto.rs:354,362` are inside `#[cfg(test)] mod tests`).
4. **Why it is debt**: While recent refactorings (`4ac812f`..`91ed772`) moved `fs_security`, `QuoteHotkeyFallback`, and modifier constants into `src/platform/`, leaving `#[cfg(target_os = ...)]` in `src/tray.rs`, `src/main.rs`, `src/app/mod.rs`, and `#[cfg(unix)]` in `src/logging.rs:67` creates ambiguity about whether the `AGENTS.md` rule is strictly enforced.
5. **Recommendation**: Move the `0o600` log file open helper into `platform::fs_security::open_private_append(path)`, expose OS-agnostic facade wrappers in `src/platform/mod.rs` for `show_fatal_error_dialog`, `activate_existing_instance`, `configure_native_viewport`, `exit_after_cleanup_if_required`, and `has_cmd_modifier(modifiers)`, and either move Windows tray backend wiring under `src/platform/` or document `src/tray.rs` and `src/theme.rs::platform_select` as explicit exceptions in `AGENTS.md`.
6. **Effort**: M (~3 hours).

**TD-3 — Stringly / boxed error types across internal module boundaries**
1. **Title**: `Database::open`, `AppSettings::save`, `HotkeyService::new`, ` crypto::get_or_create_encryption_key`, `media::{rgba_to_png, png_to_rgba}`, and `platform::windows::{dpapi_protect, dpapi_unprotect}` return `Box<dyn Error>` or `String` instead of typed errors.
2. **Severity**: Medium
3. **Evidence** — `src/db.rs:189-192`:
   ```rust
       pub(crate) fn open(
           path: &Path,
           encryption_key: &str,
       ) -> Result<Self, Box<dyn std::error::Error>> {
   ```
   `src/settings.rs:551-555`:
   ```rust
       pub(crate) fn save(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
           let json = serde_json::to_string(self)?;
           db.set_setting(Self::SETTINGS_KEY, &json)?;
           Ok(())
       }
   ```
   `src/platform/windows.rs:733` and `src/platform/windows.rs:784`:
   ```rust
   pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
   ```
   ```rust
   pub(crate) fn dpapi_unprotect(ciphertext: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
   ```
   `src/crypto.rs:56-59` and `src/media.rs:11-15`:
   ```rust
   pub(crate) fn get_or_create_encryption_key(
       data_dir: &Path,
       db_exists: bool,
   ) -> Result<(Zeroizing<String>, KeyStorageMode), Box<dyn std::error::Error>> {
   ```
   ```rust
   pub(crate) fn rgba_to_png(
       width: usize,
       height: usize,
       rgba: &[u8],
   ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
   ```
4. **Why it is debt**: Notice how `Database::clear()` was recently upgraded to return a typed `Result<(), ClearError>` (`src/db.rs:105-113`), which immediately allowed `ClipboardApp::render_clear_confirm_modal` (`src/app/mod.rs:1387-1409`) to distinguish `ClearError::Delete` from `ClearError::Compaction`. By contrast, the remaining `Box<dyn Error>` and `String` boundaries erase failure causes and prevent callers from matching on structured variants (e.g., key missing vs. I/O failure vs. SQLCipher key rejection).
5. **Recommendation**: Introduce small domain error enums (`DbOpenError`, `SettingsError`, `CryptoKeyError`, `MediaError`, `DpapiError`) implementing `std::error::Error` with `source()` chains, mirroring `ClearError` in `src/db.rs:105-149`.
6. **Effort**: M (~4 hours).

**TD-4 — Startup orchestrator `run()` and query builder `Database::get_recent()` rely on `#[allow(clippy::too_many_lines)]` without a recorded reason**
1. **Title**: `main::run` and `Database::get_recent` suppress `clippy::too_many_lines` without a `reason = "..."` attribute or decomposition.
2. **Severity**: Medium
3. **Evidence** — `src/main.rs:74-75` (151 lines, lines 75–225):
   ```rust
   #[allow(clippy::too_many_lines)]
   fn run() -> Result<(), Box<dyn std::error::Error>> {
   ```
   `src/db.rs:609-614` (118 lines, lines 610–727):
   ```rust
       #[allow(clippy::too_many_lines)]
       pub(crate) fn get_recent(
           &self,
           limit: usize,
           filter: Option<&str>,
       ) -> Result<Vec<ClipboardItem>, rusqlite::Error> {
   ```
   Also `src/settings.rs:163` (`ParsedHotkey::parse`) and `src/settings.rs:309` (`ParsedHotkey::format`):
   ```rust
       #[allow(clippy::too_many_lines, clippy::match_same_arms)]
       pub(crate) fn parse(s: &str) -> Option<Self> {
   ```
   ```rust
       #[allow(clippy::too_many_lines)]
       pub(crate) fn format(&self) -> String {
   ```
4. **Why it is debt**: `main::run()` mixes 8 distinct startup phases (data directory resolution, single-instance lock, SQLCipher init, settings load + startup prune, viewport configuration, channel creation, font/theme registration, and worker spawning), and `Database::get_recent()` embeds three query branches plus legacy-row classification in one function. Meanwhile, `src/app/mod.rs:1722-1725` already demonstrates the project's preferred pattern (`#[allow(clippy::too_many_lines, reason = "...")]`) for exhaustive key tables.
5. **Recommendation**: Split `main::run()` into `acquire_single_instance()`, `build_native_options()`, and `init_egui_context()`; extract `fetch_raw_recent_rows()` from `Database::get_recent()`; and add `reason = "exhaustive key-code mapping table"` to `ParsedHotkey::parse` and `ParsedHotkey::format`.
6. **Effort**: S (~1.5 hours).

### Low

**TD-5 — Broad `#![allow(clippy::missing_const_for_fn)]` and `#![allow(unexpected_cfgs)]` crate/module attributes lack inline rationale**
1. **Title**: Module-level lint suppressions in `src/platform/mod.rs`, `src/main.rs`, and `src/platform/macos.rs` have no `reason` or comment explaining why they are needed.
2. **Severity**: Low
3. **Evidence** — `src/platform/mod.rs:8`:
   ```rust
   #![allow(clippy::missing_const_for_fn)]
   ```
   while lines 97-103 and 149-155 in the same file redundantly carry per-function `expect(clippy::missing_const_for_fn, reason = ...)` attributes that can never fire while line 8 is active:
   ```rust
   #[cfg_attr(
       not(target_os = "windows"),
       expect(
           clippy::missing_const_for_fn,
           reason = "non-Windows stub keeps the cross-platform facade uniform"
       )
   )]
   pub(crate) fn quote_key_is_ugrave() -> bool {
   ```
   And `src/main.rs:2` / `src/platform/macos.rs:2`:
   ```rust
   #![allow(unexpected_cfgs)]
   ```
4. **Why it is debt**: Having `#![allow(clippy::missing_const_for_fn)]` at the top of `src/platform/mod.rs` disables `missing_const_for_fn` across the entire 765-line module and renders the narrower `expect(..., reason = ...)` attributes on lines 99 and 151 inert on Linux/macOS. Similarly, `#![allow(unexpected_cfgs)]` in `src/main.rs:2` is unmotivated now that `objc::msg_send!` / `sel!` macro expansions are confined to `src/platform/macos.rs`.
5. **Recommendation**: Remove `#![allow(unexpected_cfgs)]` from `src/main.rs:2` (keep it only in `src/platform/macos.rs:2` with a `reason = "objc crate macros check cargo-clippy cfg"`), and either remove `#![allow(clippy::missing_const_for_fn)]` from `src/platform/mod.rs:8` in favor of targeted `#[allow(clippy::missing_const_for_fn, reason = "...")]` on platform-dispatch functions or remove the redundant `cfg_attr(..., expect(...))` on lines 97–103 and 149–155.
6. **Effort**: S (~0.5 hours).

**TD-6 — Duplicate startup history configuration and pruning in `main::run` and `ClipboardApp::new`**
1. **Title**: `db.set_max_history`, `db.set_ignore_concealed`, `db.set_preserve_secrets_on_prune`, and `db.prune_history(settings.max_history)` are executed twice during startup.
2. **Severity**: Low
3. **Evidence** — `src/main.rs:112-118`:
   ```rust
       let settings = AppSettings::load(&db);
       db.set_max_history(settings.max_history);
       db.set_ignore_concealed(settings.ignore_concealed);
       db.set_preserve_secrets_on_prune(settings.preserve_secrets_on_prune);
       if let Err(e) = db.prune_history(settings.max_history) {
           logging::log_message("WARN", &format!("Startup history prune failed: {e}"));
       }
   ```
   and moments later inside `ClipboardApp::new` at `src/app/mod.rs:233-239`:
   ```rust
           db.set_max_history(settings.max_history);
           db.set_ignore_concealed(settings.ignore_concealed);
           db.set_auto_detect_secrets(settings.auto_detect_secrets);
           db.set_preserve_secrets_on_prune(settings.preserve_secrets_on_prune);
           if let Err(e) = db.prune_history(settings.max_history) {
               eprintln!("⚠️ Failed to prune history at startup: {e}");
           }
   ```
4. **Why it is debt**: `main::run` forgets `db.set_auto_detect_secrets(settings.auto_detect_secrets)` before spawning the clipboard monitor at `src/main.rs:201`, whereas `ClipboardApp::new` sets all four flags and runs a second `DELETE` prune query (logging to `eprintln!` instead of `logging::log_message`).
5. **Recommendation**: Centralize runtime DB-flag synchronization into a single `db.apply_settings(&settings)` method (or inside `ClipboardApp::new`), and run the startup `prune_history` once with `logging::log_message("WARN", ...)`.
6. **Effort**: S (~0.25 hours).

## Quick Wins
1. **TD-6**: Deduplicate the startup `db.set_*` and `db.prune_history` sequence between `src/main.rs:112-118` and `src/app/mod.rs:233-239` (~0.25 h).
2. **TD-5**: Remove `#![allow(unexpected_cfgs)]` from `src/main.rs:2` and reconcile `#![allow(clippy::missing_const_for_fn)]` with the per-fn `expect` attributes in `src/platform/mod.rs` (~0.5 h).
3. **TD-4**: Add `reason = "..."` to `ParsedHotkey::{parse, format}` allows and extract `fetch_raw_recent_rows` from `Database::get_recent` (~1.5 h).
4. **TD-2 (logging + app)**: Move the Unix `0o600` log file open logic in `src/logging.rs:67-71` into `src/platform/fs_security.rs` and replace `#[cfg(target_os = "macos")]` in `src/app/mod.rs:1714` with a `platform` helper (~1 h).

## Recommended Actions
1. **TD-6 & TD-5**: Clean up the duplicate startup prune and redundant/unexplained lint attributes first (zero behavioral risk).
2. **TD-2**: Finish confining remaining `#[cfg(target_os = ...)]` / `#[cfg(unix)]` blocks in `src/logging.rs`, `src/app/mod.rs`, `src/main.rs`, and `src/tray.rs` behind `src/platform/` helpers so `AGENTS.md` is 100% satisfied by mechanical grep.
3. **TD-4**: Decompose `main::run()` and `Database::get_recent()` so `#[allow(clippy::too_many_lines)]` can be removed from non-table functions.
4. **TD-3**: Replace `Box<dyn Error>` / `String` error returns at internal module boundaries (`db::open`, `settings::save`, `crypto::get_or_create_encryption_key`, `windows::dpapi_*`) with typed error enums following the `ClearError` pattern.
5. **TD-1**: Incrementally group `ClipboardApp` fields into view-state, focus-state, and background-service sub-structs.

## Coverage
- **1. Debt Markers and Suppressions** — Reviewed all 31 `#[allow]` / `#![allow]` occurrences across `src/`, `Cargo.toml`, and `.github/` (`grep -RInE 'TODO|FIXME|HACK|XXX|#\[allow|#!\[allow' src Cargo.toml .github`). Zero `TODO`, `FIXME`, `HACK`, or `XXX` markers exist in the repository. Numeric cast suppressions in `src/color.rs:66,268,283,295,299,340`, `src/theme.rs:405`, `src/secret.rs:374`, `src/password.rs:272`, and `src/app/generator_view.rs:139` were inspected and are paired with explicit range clamps/checks. Findings TD-1, TD-4, TD-5.
- **2. Module and Ownership Cohesion** — Reviewed all 25 `.rs` files (14,329 total lines). `Database(Arc<DatabaseInner>)` (`src/db.rs:151-162`) cleanly consolidates shared connection and atomic settings behind a single `Arc`. No `unsafe impl Send` or `unsafe impl Sync` exists anywhere in the crate. Finding TD-1.
- **3. API and Type Debt** — Reviewed function signatures across all modules. No `&String`, `&Vec<T>`, or `&Box<T>` parameters found. Finding TD-3.
- **4. Error and Panic Debt** — Reviewed `unwrap()`, `expect()`, `panic!()`, and `let _ =` sites across `src/`. Production code has zero `unwrap()`/`expect()` calls (`src/crypto.rs:225` is inside `#[cfg(test)] pub(crate) mod mock_keyring`; `build.rs:16-21` intentionally panics at build time if Windows icon/manifest compilation fails). Detailed error propagation findings are covered in `ERROR_HANDLING_REVIEW_2026-11-21-b.md`; finding TD-6 notes the duplicated startup prune error handling.
- **5. Cargo and Feature Debt** — Reviewed `Cargo.toml` and `cargo tree -d` output. The previous `cfg(windows)` vs `cfg(target_os = "windows")` mismatch in `Cargo.toml` was resolved (`winres = "0.1"` is now an unconditional `[build-dependencies]` entry at `Cargo.toml:29-32` with a clear cross-compilation comment, and all target sections use `cfg(target_os = "...")`). Transitive duplicates reported by `cargo tree -d` (`hashbrown 0.16/0.17`, `rustix 0.38/1.1`, `smithay-client-toolkit 0.19/0.20`, `syn 2/3`, `thiserror 1/2`, `x11rb 0.13/0.14`) come from upstream `eframe`/`arboard`/`keyring`/`global-hotkey` dependency graphs and do not cause trait-bound friction in crate code.
- **6. Platform and Testing Debt** — Reviewed platform boundaries and test suites. `unsafe` is strictly confined to `src/platform/macos.rs` and `src/platform/windows.rs`. Finding TD-2 covers remaining `#[cfg]` blocks outside `src/platform/`.
- **7. Documentation Debt** — Reviewed doc comments and normative lesson files (`AGENTS.md`, `docs/*LESSONS.md`). `# Errors` and `# Safety` sections are present on fallible and `unsafe fn` helpers. Finding TD-2 notes the code/rule gap on platform `#[cfg]` confinement.
