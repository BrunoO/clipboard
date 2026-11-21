# Technical Debt Review - 2026-11-21

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 7
- **Estimated Remediation Effort**: 16 hours
- **Evidence Commands Run**: `grep -RInE 'TODO|FIXME|HACK|XXX|#\[allow|#!\[allow' src Cargo.toml .github` (parent-captured inventory at `/private/var/folders/04/j6_9v0hn5mz2syff3s6sdwp80000gp/T/opencode/review-evidence/debt-inventory.log`, read verbatim); `wc -l src/*.rs src/platform/*.rs` (same log, 12,673 total lines); `git ls-files Cargo.lock .github/workflows`; `grep -RIn -- '--locked' .github`; direct source reads of `Cargo.toml`, `src/main.rs`, `src/app/mod.rs`, `src/settings.rs`, `src/platform/mod.rs`, `src/tray.rs`, `src/db.rs`, `src/crypto.rs`, `src/platform/windows.rs`. No cargo/clippy/test command was run for this report — build and static-analysis results are out of scope and belong to the separate static-analysis report. Parent graph evidence: tier=Verify, project `Users-brunoorsier-dev-clipboard`, generation `2026-11-21T07:10:32Z`, `index_status ready`, 1284 nodes/4578 edges; 37 paths coverage-checked.

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**TD-1 — Clippy `too_many_lines` suppression hides a long startup orchestrator**
1. **Title**: `run()` is a multi-phase setup function protected by a blanket lint allow.
2. **Severity**: Medium
3. **Evidence** — `src/main.rs:74-75`:
   ```rust
   #[allow(clippy::too_many_lines)]
   fn run() -> Result<(), Box<dyn std::error::Error>> {
   ```
   The phase comments confirm multiple unrelated responsibilities, e.g. `src/main.rs:78` `// 1. Resolve application data directory (cross-platform: macOS, Windows, Linux)`, `src/main.rs:85` `// 1b. Enforce single-instance execution so two monitors don't race on the database`, `src/main.rs:106` `// 2. Open or initialize the SQLCipher encrypted database (key is zeroized on scope exit)`, `src/main.rs:111` `// 5. Load persistent application settings`.
4. **Why it is debt**: `run()` owns directory setup, locking, DB/key handling, settings, viewport construction and eframe launch; the allow prevents the lint from flagging further growth. Onboarding cost and blast radius of any startup change are high.
5. **Recommendation**: Extract named helpers (`acquire_instance`, `build_viewport`, `configure_database`) and drop the allow once each helper is under the threshold.
6. **Effort**: M (~3 hours).

**TD-2 — Mixed-concern god struct with a bool-suppression**
1. **Title**: `ClipboardApp` aggregates DB, channels, tray, monitor lifecycle and a large set of UI flags.
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:144-146` and `src/app/mod.rs:163-186`:
   ```rust
   #[allow(clippy::struct_excessive_bools)]
   pub(crate) struct ClipboardApp {
       db: Database,
   ```
   ```rust
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
   ```
   Parent graph evidence (same generation as above) reports `ClipboardApp::new` fan-in 44 and package edges `app→theme` (82 calls), `app→password` (45), `app→db` (33), `app→settings` (25), `app→platform` (18), `app→hotkey` (16).
4. **Why it is debt**: A single type is the integration point for nearly every leaf module; `#[allow(clippy::struct_excessive_bools)]` suppresses the signal that the state should be split. It also blocks isolated unit testing of view logic.
5. **Recommendation**: Group fields into `UiState`, `BackgroundHandles` and `Channels` sub-structs; represent mutually exclusive modal flags with an enum.
6. **Effort**: L (~6 hours).

**TD-3 — Opaque error types at internal boundaries**
1. **Title**: Library-like modules return `Box<dyn Error>`/`String` rather than matchable typed errors.
2. **Severity**: Medium
3. **Evidence** — `src/db.rs:124-127`:
   ```rust
       pub(crate) fn open(
           path: &Path,
           encryption_key: &str,
       ) -> Result<Self, Box<dyn std::error::Error>> {
   ```
   `src/settings.rs:565-568`:
   ```rust
       pub(crate) fn save(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
           let json = serde_json::to_string(self)?;
           db.set_setting(Self::SETTINGS_KEY, &json)?;
           Ok(())
       }
   ```
   `src/platform/windows.rs:671` and `src/platform/windows.rs:715`:
   ```rust
   pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
   ```
   ```rust
   pub(crate) fn dpapi_unprotect(ciphertext: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
   ```
4. **Why it is debt**: Callers can only string-match or erase the error; this is one reason the UI sites in `src/app/settings_view.rs` discard `save()` results instead of reacting to a specific failure. The crate is binary-only, so the cost is contained but still present at the db/crypto boundary.
5. **Recommendation**: Introduce a small `thiserror`-based enum for `Database::open`/`AppSettings::save`/DPAPI and convert to `anyhow` with context only in `main::run`.
6. **Effort**: L (~6 hours).

**TD-4 — Inconsistent `cfg` predicate in the Cargo manifest**
1. **Title**: `cfg(windows)` and `cfg(target_os = "windows")` are mixed in `Cargo.toml`.
2. **Severity**: Medium
3. **Evidence** — `Cargo.toml:28-31`:
   ```toml
   [target.'cfg(windows)'.build-dependencies]
   winres = "0.1"

   [target.'cfg(target_os = "windows")'.dependencies]
   ```
4. **Why it is debt**: `docs/MULTIPLATFORM_LESSONS.md:13` requires "Standardize `#[cfg]` predicates across the crate. Use `#[cfg(target_os = "windows")]` ... Never mix `#[cfg(windows)]` and `#[cfg(target_os = "windows")]` in the same file." The manifest is the same configuration surface.
5. **Recommendation**: Change line 28 to `[target.'cfg(target_os = "windows")'.build-dependencies]`.
6. **Effort**: S (~0.5 hours).

### Low

**TD-5 — Module-wide clippy allow without a recorded reason**
1. **Title**: `#![allow(clippy::missing_const_for_fn)]` disables an entire lint class in the safe facade.
2. **Severity**: Low
3. **Evidence** — `src/platform/mod.rs:1`:
   ```rust
   #![allow(clippy::missing_const_for_fn)]
   ```
4. **Why it is debt**: Tech-debt prompt §1 requires each suppression that weakens a lint to have narrow scope and a reason. This is module-wide and undocumented; it is harmless today (the facade is thin) but hides future drift.
5. **Recommendation**: Remove the crate-level allow and annotate the few functions that genuinely cannot be `const`, with a reason.
6. **Effort**: S (~0.5 hours).

**TD-6 — Unexplained `dead_code` allow on a cross-platform helper**
1. **Title**: `load_tray_icon_rgba` carries `#[allow(dead_code)]` without a reason.
2. **Severity**: Low
3. **Evidence** — `src/tray.rs:173-174`:
   ```rust
   #[allow(dead_code)]
   pub(crate) fn load_tray_icon_rgba() -> Result<(Vec<u8>, u32, u32), image::ImageError> {
   ```
   The helper is used on Windows at `src/tray.rs:67` (`let (rgba, width, height) = load_tray_icon_rgba()?;`); the allow exists for non-Windows builds.
4. **Why it is debt**: Narrow but undocumented; a reader cannot tell whether the function is abandoned or intentionally Windows-only.
5. **Recommendation**: Gate the helper with `#[cfg(any(target_os = "windows", test))]` (the pattern already used at `src/crypto.rs:370`) and delete the allow.
6. **Effort**: S (~0.25 hours).

**TD-7 — Large exhaustive match shielded by `too_many_lines`**
1. **Title**: `egui_key_to_hotkey_name` needs a lint suppression for a pure mapping table.
2. **Severity**: Low
3. **Evidence** — `src/app/mod.rs:1574-1576`:
   ```rust
   #[allow(clippy::too_many_lines)]
   fn egui_key_to_hotkey_name(key: egui::Key) -> Option<&'static str> {
       let name = match key {
   ```
4. **Why it is debt**: Legitimate exhaustive mapping, but an unexplained allow indistinguishable from TD-1 weakens the value of the lint as a size signal.
5. **Recommendation**: Keep the allow but add a one-line reason (`// exhaustive egui::Key mapping`), or move the mapping to a static table.
6. **Effort**: S (~0.5 hours).

## Quick Wins
1. **TD-4**: Change `Cargo.toml:28` from `cfg(windows)` to `cfg(target_os = "windows")` (~0.5 h, no source impact).
2. **TD-5/TD-7**: Add one-line reason comments to the module-wide `src/platform/mod.rs:1` and `src/app/mod.rs:1574` allows, or narrow them (~1 h).
3. **TD-6**: Replace `#[allow(dead_code)]` on `src/tray.rs:173` with a `#[cfg(any(target_os = "windows", test))]` gate (~0.25 h).
4. **TD-3 (first step)**: Give `dpapi_protect`/`dpapi_unprotect` a typed error instead of `String`; they are only called from `src/crypto.rs:176` and `:334` (~2 h).

## Recommended Actions
1. **TD-2 (highest payoff)**: Split `ClipboardApp` state into view/background/channel groups before adding any new settings fields; do this first because every later UI change touches this struct.
2. **TD-1**: Extract startup phases from `run()` into named helpers; re-run the project's verification suite (handled by the parent) afterwards.
3. **TD-3**: Replace boxed/stringly errors at `db`, `settings`, and DPAPI boundaries with typed enums; keep `anyhow` only at `main::run`. Coordinate with the error-handling review so ignored `Result`s are fixed in the same pass.
4. **TD-4/TD-5/TD-6/TD-7**: Batch the manifest predicate fix and lint-suppression cleanup into one small PR.

## Coverage
- **1. Debt Markers and Suppressions** — Reviewed. The captured `debt-inventory.log` (lines 1–79) contains file names, line counts and `#[allow]` hits only; no `TODO`, `FIXME`, `HACK` or `XXX` lines were captured. Findings TD-1, TD-2, TD-5, TD-6, TD-7. Narrow cast allows at `src/color.rs:268,283,295,299` were read and are paired with explicit range checks (`if (0.0..=100.0).contains(&pct)` at `src/color.rs:271`, `if (0.0..=255.0).contains(&val)` at `src/color.rs:282`), so they are intentional invariants, not findings.
- **2. Module and Ownership Cohesion** — Finding TD-2. All 17 modules are declared in `src/main.rs:4-20`; no `unsafe impl Send/Sync` found outside the documented `thread_local!` justification at `src/platform/macos.rs:118-124`.
- **3. API and Type Debt** — Finding TD-3. No additional findings were recorded from the signatures actually read in `db.rs`, `settings.rs`, `media.rs`, `tray.rs`, `hotkey.rs` and `platform/windows.rs`.
- **4. Error and Panic Debt** — Duplicates are intentionally not recorded here; see `ERROR_HANDLING_REVIEW_2026-11-21.md`. `src/code.rs:431` (`expect("Prettify failed")`) was verified to sit inside `#[cfg(test)] mod tests` beginning at `src/code.rs:422`, so it is not production panic debt.
- **5. Cargo and Feature Debt** — Finding TD-4. Positive evidence: `Cargo.lock` is tracked (`git ls-files Cargo.lock .github/workflows` output includes `Cargo.lock`) and release builds use it (`.github/workflows/release.yml:70` `run: cargo build --release --locked`, and `:218` likewise).
- **6. Platform and Testing Debt** — No findings in this category. `unsafe` is confined to `src/platform/macos.rs` and `src/platform/windows.rs` per the captured `unsafe-inventory.log`; `src/platform/mod.rs` is a safe facade.
- **7. Documentation Debt** — No findings in this category in the code read for this review; normative invariants in `AGENTS.md`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md` and `docs/CONCURRENCY_AND_PERF_LESSONS.md` matched the reviewed implementations.
