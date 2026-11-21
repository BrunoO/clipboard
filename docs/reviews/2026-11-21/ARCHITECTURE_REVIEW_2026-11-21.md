# Architecture Review - 2026-11-21

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 4
- **Estimated Remediation Effort**: 10 hours
- **Evidence Commands Run**: `git ls-files Cargo.lock .github/workflows`; `grep -RIn -- '--locked' .github`; `grep -RIn "platform_select" src/theme.rs`; direct source reads of `Cargo.toml`, `src/main.rs`, `src/app/mod.rs`, `src/platform/mod.rs`, `src/platform/macos.rs`, `src/platform/linux.rs`, `src/db.rs`, `src/settings.rs`. No cargo/clippy/test command was run for this report — build and static-analysis results are out of scope and belong to the separate static-analysis report. Parent graph evidence (tier=Verify, project `Users-brunoorsier-dev-clipboard`, generation `2026-11-21T07:10:32Z`, `index_status ready`, 1284 nodes/4578 edges, 37 paths coverage-checked) is cited where used.

## Findings

**Observed architecture (graph + source only)**: Single binary target `CipherClipboard` (`Cargo.toml:12-14`), 17 private modules declared at `src/main.rs:4-20`; entry points `build.rs::main` and `src/main.rs::main`. Package edges from the parent graph: `app→theme` (82 calls), `app→password` (45), `app→db` (33), `app→settings` (25), `app→platform` (18), `app→hotkey` (16), `db→settings` (8), `db→crypto` (8). Hotspots: `ClipboardApp::new` (fan-in 44), `PassphraseSeparator::label` (32), `theme::text_secondary` (23), `Database::open` (22), `crypto::get` (16), `Database::get_recent` (14), `ClipboardApp::show_toast` (14). The platform facade exposes OS-agnostic names (`is_app_active_state`, `clipboard_change_token`, `mark_clipboard_concealed`, `restore_focus_and_paste`) with safe stubs at `src/platform/mod.rs:55-58`, `:102-105`, `:124-127`, `:164-167`. `eframe::App` placement is correct: `logic()` at `src/app/mod.rs:1388-1407` owns close/tray/hotkey/activation/monitor/auto-clear handling, while `ui()` at `src/app/mod.rs:1409-1444` owns keyboard/pointer input (`handle_quit_shortcut`, `handle_escape_key`, `handle_keyboard_navigation` at lines 1421-1423) and rendering.

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**A-1 — Process-global paste worker has no shutdown or join path**
1. **Area**: `src/platform/mod.rs`, concurrency ownership / shutdown.
2. **Severity**: Medium
3. **Evidence** — `src/platform/mod.rs:250-260` and `src/platform/mod.rs:290-303`:
   ```rust
   fn paste_worker_loop(rx: &std::sync::mpsc::Receiver<PasteRequest>) {
       let mut pending: Option<PasteRequest> = None;
       loop {
           let mut req = if let Some(r) = pending.take() {
               r
           } else {
               let Ok(r) = rx.recv() else {
                   break;
               };
   ```
   ```rust
   fn paste_worker_tx() -> &'static std::sync::mpsc::Sender<PasteRequest> {
       static TX: std::sync::OnceLock<std::sync::mpsc::Sender<PasteRequest>> =
           std::sync::OnceLock::new();
       TX.get_or_init(|| {
           let (tx, rx) = std::sync::mpsc::channel::<PasteRequest>();
           if let Err(e) = std::thread::Builder::new()
               .name("focus-paste-worker".to_string())
               .spawn(move || paste_worker_loop(&rx))
           {
               log_message("ERROR", &format!("Failed to spawn focus-paste worker: {e}"));
           }
           tx
       })
   }
   ```
4. **Architectural impact**: The `OnceLock` owns the only `Sender` for the process lifetime, so `rx.recv()` never observes `Disconnected` and the loop cannot exit through its own break path. No `JoinHandle` is stored, so there is no deterministic join. The thread *is* named (`focus-paste-worker`) and coalescing is correct, but `docs/CONCURRENCY_AND_PERF_LESSONS.md:10` requires "Name every spawned thread and join owned workers on shutdown." [Inferred] Any in-flight focus/paste OS work may be interrupted at process exit instead of draining.
5. **Recommendation**: Store a `OnceLock<Worker>` holding both sender and `JoinHandle`, expose `shutdown()` that drops the sender and joins, and call it from `ClipboardApp::drop` / `shutdown_and_exit`.
6. **Migration risk**: Calls to `restore_focus_and_paste` after shutdown become no-ops; any caller relying on the current infinite-lifetime singleton must be audited.

**A-2 — Duplicated two-phase shutdown sequence in macOS exit and `Drop`**
1. **Area**: `src/app/mod.rs`, lifecycle / ownership.
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:1454-1466` and `src/app/mod.rs:1469-1479`:
   ```rust
       #[cfg(target_os = "macos")]
       fn shutdown_and_exit(&mut self) -> ! {
           self.clear_pending_secret_from_clipboard();
           if let Some(hotkey) = self.hotkey_service.as_mut() {
               hotkey.shutdown();
           }
           let _ = self.monitor_shutdown_tx.send(());
           if let Some(handle) = self.monitor_handle.take() {
               let _ = handle.join();
           }
           crate::platform::teardown_reopen_observer();
           std::process::exit(0);
       }
   ```
   ```rust
   impl Drop for ClipboardApp {
       fn drop(&mut self) {
           self.clear_pending_secret_from_clipboard();
           if let Some(hotkey) = self.hotkey_service.as_mut() {
               hotkey.shutdown();
           }
           let _ = self.monitor_shutdown_tx.send(());
           if let Some(handle) = self.monitor_handle.take() {
               let _ = handle.join();
           }
   ```
4. **Architectural impact**: Two hand-maintained copies of the same cleanup ordering (secret clear → hotkey stop → monitor signal → monitor join); the macOS copy adds observer teardown. `let _ = handle.join();` also discards worker panics. `docs/SECURITY_LESSONS.md:38` treats this exact ordering as a crash-avoidance invariant, so drift is a correctness risk, not just duplication.
5. **Recommendation**: Extract `fn shutdown_resources(&mut self)` (idempotent via `Option::take`), call it from both `Drop` and `shutdown_and_exit`, and keep the `process::exit(0)` only in the macOS path; log join failures.
6. **Migration risk**: Must preserve `process::exit(0)` after cleanup on macOS (documented in `docs/MULTIPLATFORM_LESSONS.md:43`); must not be called twice in one process.

**A-3 — `ClipboardApp` is a high-fan-in integration hub across nearly every package**
1. **Area**: Module boundaries / package layering.
2. **Severity**: Medium
3. **Evidence**: Parent graph evidence (tier=Verify, generation `2026-11-21T07:10:32Z`) reports `ClipboardApp::new` fan-in 44 plus the edges `app→theme` (82), `app→password` (45), `app→db` (33), `app→settings` (25), `app→platform` (18), `app→hotkey` (16). Source confirmation at `src/app/mod.rs:144-187`: the struct directly owns `db: Database`, `settings: AppSettings`, `hotkey_service: Option<HotkeyService>`, `tray_rx`/`notify_rx`/`hotkey_rx` receivers, `texture_cache`, `monitor_shutdown_tx` and `monitor_handle`.
4. **Architectural impact**: The `app` package is the single point where UI, DB, settings, password generation, theming and platform FFI meet. This is intentional for a small desktop app and is not a documented-invariant violation, but it concentrates coupling and makes the `app` layer the default place to add features. [Inferred] Testability and parallel work suffer as the struct grows.
5. **Recommendation**: Introduce thin view-model/controller types under `src/app/` (`HistoryController`, `SettingsController`) that own DB+settings interaction; keep `ClipboardApp` as composition root. Do not change platform layering.
6. **Migration risk**: Large mechanical refactor of field access; must preserve the `logic()`/`ui()` split described above.

### Low

**A-4 — Target-specific build dependency uses a different `cfg` predicate than dependencies**
1. **Area**: Crate and target structure (`Cargo.toml`).
2. **Severity**: Low
3. **Evidence** — `Cargo.toml:28-31`:
   ```toml
   [target.'cfg(windows)'.build-dependencies]
   winres = "0.1"

   [target.'cfg(target_os = "windows")'.dependencies]
   ```
4. **Architectural impact**: Two predicates for the same OS in one manifest; `docs/MULTIPLATFORM_LESSONS.md:13` requires a single standardized predicate. No functional mismatch was observed from the manifest alone.
5. **Recommendation**: Normalize line 28 to `cfg(target_os = "windows")`.
6. **Migration risk**: None expected; verify the Windows build (handled by parent CI).

## Quick Wins
1. **A-4**: Normalize the `cfg(windows)` build-dependency predicate in `Cargo.toml:28` (~0.5 h).
2. **A-2**: Extract the shared `shutdown_resources` helper so the two teardown paths cannot drift (~2 h).
3. **A-1**: Add a `Drop`/`shutdown` hook to the paste worker by moving the sender out of the `OnceLock` into a owned worker struct (~3 h).
4. Preserve and document the already-correct `logic()`/`ui()` split with a short comment at `src/app/mod.rs:1384-1388` (~0.25 h) — the comment already exists; keep it as the canonical rationale.

## Recommended Actions
1. **A-2 first**: Unify shutdown paths before touching concurrency, because both the monitor and paste worker lifecycle changes depend on it.
2. **A-1 second**: Give the paste worker an explicit shutdown/join and call it from the unified teardown; add a test that the worker exits on sender drop.
3. **A-3 third**: Split `ClipboardApp` into controller sub-types incrementally, starting with settings persistence (which also addresses the ignored `save()` results in the error-handling report).
4. **A-4 last**: Fold the manifest predicate fix into the next platform-related PR.

## Coverage
- **1. Crate and Target Structure** — Finding A-4. Positive evidence: `Cargo.lock` is tracked (`git ls-files Cargo.lock .github/workflows` includes `Cargo.lock`) and release workflows use `--locked` (`.github/workflows/release.yml:70` and `:218`, verbatim `run: cargo build --release --locked`). Package is binary-only: `Cargo.toml:12-14` declares `[[bin]]` named `CipherClipboard`; no `[lib]` target exists.
- **2. Module Boundaries** — Finding A-3. No literal import cycles observed; `src/main.rs:4-20` declares all modules private.
- **3. Platform Facade** — No findings in this category. `src/platform/mod.rs` contains no `unsafe` (parent unsafe inventory confirms `unsafe` appears only in `macos.rs` and `windows.rs`); OS-agnostic names and safe stubs observed at `src/platform/mod.rs:55-58`, `:69-72`, `:102-105`, `:124-127`, `:164-167`. Single-OS APIs are target-gated, e.g. `src/platform/mod.rs:15-17` and `src/platform/windows.rs:669` `DPAPI_ENTROPY` usage from `src/crypto.rs:334`. `src/theme.rs:143` defines `const fn platform_select<T: Copy>(macos: T, windows: T, other: T) -> T` with 11 call sites (grep output), matching `docs/MULTIPLATFORM_LESSONS.md:14`.
- **4. Concurrency Ownership** — Findings A-1 and A-2. Positive evidence: monitor thread is named and joined (`src/monitor.rs:45-47`, `src/app/mod.rs:1475-1477`); Disconnected handled on monitor shutdown (`src/monitor.rs:59`) and X11 fallback (`src/hotkey.rs:421`); coupled Win32 window state is one `Mutex<Option<CapturedWindow>>` (`src/platform/windows.rs:49`).
- **5. UI Event-Loop Architecture** — No findings in this category. `logic()`/`ui()` placement verified at `src/app/mod.rs:1388-1407` / `:1409-1444`; repaint is deadline-based (`src/app/mod.rs:1396-1398`) and the brief post-hide tick is explicitly bounded (`src/app/mod.rs:1404-1406`).
- **6. Error and Data Flow** — No findings in this category. Multi-statement writes use one transaction (`src/db.rs:333`, `:383`, `:432`); LIKE wildcards escaped and secrets excluded (`src/db.rs:491-498`, `:520`); classification runs outside the DB mutex (`src/db.rs:326-332`, `:500-541`).
- **7. API Guidelines (public library surface)** — No public library API surface found. `Cargo.toml:12-14` defines only a `[[bin]]` and `src/main.rs:4-20` declares all modules private, so no SemVer analysis applies.
