# Architecture Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 3
- **Estimated Remediation Effort**: 8 hours
- **Evidence Commands Run**: `cargo metadata --no-deps --format-version 1`; `find src -maxdepth 2 -name '*.rs' -print | sort`; `grep -RInE '^pub |^pub\(crate\) |thread::Builder|Arc<|Mutex<|RwLock<|thread_local!|impl eframe::App|fn logic\(|fn ui\(' src`; `grep -RIn 'cfg(' src --exclude-dir=platform`; direct source reads of `Cargo.toml`, `build.rs`, `src/main.rs`, `src/app/mod.rs`, `src/app/history_view.rs`, `src/app/settings_view.rs`, `src/app/generator_view.rs`, `src/platform/mod.rs`, `src/platform/fs_security.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/platform/linux.rs`, `src/db.rs`, `src/crypto.rs`, `src/settings.rs`, `src/hotkey.rs`, `src/tray.rs`, `src/monitor.rs`, `src/logging.rs`, `src/instance.rs`, `src/theme.rs`.

## Findings

**Observed Architecture (from `Cargo.toml`, `mod`/`use` declarations, and source reads):**
- **Crate & Target Structure**: Single binary-only package `cipherclipboard` (`Cargo.toml:1-16`, `[[bin]] name = "CipherClipboard"`, `path = "src/main.rs"`, `edition = "2021"`, `rust-version = "1.95"`). No `[lib]` target exists.
- **Top-Level Modules (`src/main.rs:4-20`)**: 17 private modules: `app` (`mod.rs`, `history_view.rs`, `settings_view.rs`, `generator_view.rs`), `code`, `color`, `crypto`, `db`, `hotkey`, `icons`, `instance`, `logging`, `media`, `monitor`, `password`, `platform` (`mod.rs`, `fs_security.rs`, `linux.rs`, `macos.rs`, `windows.rs`), `secret`, `settings`, `theme`, `tray`. All cross-module items use `pub(crate)` or `pub(super)`; `unreachable_pub = "warn"` is enforced in `Cargo.toml:93`.
- **Platform Facade (`src/platform/mod.rs`)**: Uses a unified `use {linux, macos, windows} as backend;` alias (`src/platform/mod.rs:20-25`), exposes `pub(crate) mod fs_security` (`:12`) and `QuoteHotkeyFallback` (`:28-54`), and owns the coalescing `PasteWorker` (`:572-633`) with deterministic `shutdown_paste_worker()` (`:629-633`).
- **Concurrency & Shutdown Topology**:
  - `clipboard-monitor` thread (`src/monitor.rs:45-100`) — joined in `ClipboardApp::shutdown_resources()` (`src/app/mod.rs:1601-1605`).
  - `hotkey-listener` thread (`src/hotkey.rs:97-121`) + Linux `x11-quote-hotkey` fallback thread (`src/platform/linux.rs:383-386`) — joined via `HotkeyService::shutdown()` (`src/hotkey.rs:170-179`), called from `ClipboardApp::shutdown_resources()` (`src/app/mod.rs:1596-1598`) and `HotkeyService::drop` (`src/hotkey.rs:183-190`).
  - `focus-paste-worker` thread (`src/platform/mod.rs:615-617`) — joined via `platform::shutdown_paste_worker()` (`src/platform/mod.rs:589-607`, `:629-633`), called from `ClipboardApp::shutdown_resources()` (`src/app/mod.rs:1607`).
  - Unified teardown: both `ClipboardApp::drop` (`src/app/mod.rs:1624-1628`) and macOS `ClipboardApp::shutdown_and_exit` (`src/app/mod.rs:1618-1621`) invoke `self.shutdown_resources()` (`src/app/mod.rs:1593-1609`).

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**A-1 — Platform `#[cfg(target_os = ...)]` and `#[cfg(unix)]` boundaries leak outside `src/platform/`**
1. **Area**: Platform Facade (`src/tray.rs`, `src/main.rs`, `src/app/mod.rs`, `src/logging.rs`).
2. **Severity**: Medium
3. **Evidence** — `AGENTS.md:17` and `docs/MULTIPLATFORM_LESSONS.md:10` state:
   ```markdown
   - **Confine platform `#[cfg(target_os = ...)]` / `#[cfg(unix)]` attributes to `src/platform/` (`mod.rs`, `fs_security.rs`, `linux.rs`, `macos.rs`, `windows.rs`).** Do not scatter OS-conditional `#[cfg]` blocks across application, UI, settings, hotkey, theme, or crypto modules; instead, expose OS-agnostic capability helpers, constants, or no-op fallback types from `src/platform/`.
   ```
   Observed OS-conditional attributes outside `src/platform/` (excluding `src/theme.rs:101-109`, where `const fn platform_select` is explicitly mandated by `docs/MULTIPLATFORM_LESSONS.md:15`, and `#[cfg(test)]` assertions):
   - `src/tray.rs:5,18,35,39,53,59,149,154,161` — e.g. `src/tray.rs:17-20`:
     ```rust
     pub(crate) struct TrayService {
         #[cfg(target_os = "windows")]
         tray_icon: tray_icon::TrayIcon,
     }
     ```
   - `src/logging.rs:67-71`:
     ```rust
         #[cfg(unix)]
         {
             use std::os::unix::fs::OpenOptionsExt;
             opts.mode(0o600);
         }
     ```
   - `src/app/mod.rs:189,1517,1617,1714` — e.g. `src/app/mod.rs:1714-1717`:
     ```rust
         #[cfg(target_os = "macos")]
         if modifiers.mac_cmd || modifiers.command {
             parts.push("Cmd");
         }
     ```
   - `src/main.rs:37,65,90,128` — e.g. `src/main.rs:128-134`:
     ```rust
         #[cfg(target_os = "macos")]
         {
             viewport = viewport
                 .with_fullsize_content_view(true)
                 .with_titlebar_shown(false)
                 .with_title_shown(false);
         }
     ```
4. **Architectural impact**: The recent extraction of `src/platform/fs_security.rs` and `QuoteHotkeyFallback` successfully eliminated `#[cfg]` from `crypto.rs`, `hotkey.rs`, and `settings.rs`. However, `tray.rs`, `logging.rs`, `main.rs`, and `app/mod.rs` still bypass the `src/platform/` facade for OS-conditional compilation.
5. **Recommendation**:
   - Move the `0o600` append-file open helper from `src/logging.rs:65-72` into `src/platform/fs_security.rs`.
   - Add `platform::configure_viewport(viewport)`, `platform::activate_existing_instance(pid)`, `platform::show_startup_error_dialog(msg)`, `platform::show_panic_error_dialog(msg)`, and `platform::has_mac_cmd_modifier(modifiers)` to `src/platform/mod.rs`.
   - Either move the Windows `tray_icon` implementation behind `src/platform/` (paralleling `QuoteHotkeyFallback`) or document `src/tray.rs` and `src/theme.rs::platform_select` as explicit permitted sites in `AGENTS.md`.
6. **Migration risk**: Low — pure movement of target-gated blocks behind existing module facade boundaries.

**A-2 — `ClipboardApp` remains a high-coupling state container across UI, DB, and background services**
1. **Area**: Module Boundaries (`src/app/mod.rs`, `src/app/history_view.rs`, `src/app/settings_view.rs`, `src/app/generator_view.rs`).
2. **Severity**: Medium
3. **Evidence** — `src/app/mod.rs:173-216`:
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
   ...
   ```
   All view submodules (`src/app/history_view.rs:29`, `src/app/settings_view.rs:6`, `src/app/generator_view.rs:5`) are implemented as `impl ClipboardApp` blocks operating directly on all 38 fields of `ClipboardApp`.
4. **Architectural impact**: Because `history_view`, `settings_view`, and `generator_view` are split via partial `impl ClipboardApp` blocks rather than encapsulated sub-components, every view method has unrestricted mutable access to all application channels, worker handles, and unrelated view flags.
5. **Recommendation**: Group `ClipboardApp` fields into domain structs (`WindowLifecycleState`, `HistoryViewState`, `BackgroundWorkers`) and pass only the required state slices into `history_view`, `settings_view`, and `generator_view`.
6. **Migration risk**: Medium — mechanical refactoring across `src/app/*.rs`; must preserve the `eframe::App::logic()` vs `App::ui()` execution split (`src/app/mod.rs:1528-1584`).

### Low

**A-3 — `logging::log_dir()` duplicates application data directory resolution without the legacy `rusty-clipboard` fallback used by `main::get_app_data_dir()`**
1. **Area**: Module Boundaries / Data Flow (`src/logging.rs` vs `src/main.rs`).
2. **Severity**: Low
3. **Evidence** — `src/main.rs:273-286`:
   ```rust
   fn get_app_data_dir() -> PathBuf {
       dirs::data_local_dir().map_or_else(
           || PathBuf::from("./data"),
           |p| {
               let new_dir = p.join("cipherclipboard");
               let legacy_dir = p.join("rusty-clipboard");
               if !new_dir.exists() && legacy_dir.exists() {
                   legacy_dir
               } else {
                   new_dir
               }
           },
       )
   }
   ```
   Contrast with `src/logging.rs:13-22`:
   ```rust
   pub(crate) fn log_dir() -> Option<&'static std::path::Path> {
       LOG_DIR
           .get_or_init(|| {
               let dir = dirs::data_local_dir()?.join("cipherclipboard");
               let _ = std::fs::create_dir_all(&dir);
               crate::crypto::set_restricted_dir_permissions(&dir);
               Some(dir)
           })
           .as_deref()
   }
   ```
4. **Architectural impact**: If a user upgraded from a legacy installation where `~/.local/share/rusty-clipboard` exists and `~/.local/share/cipherclipboard` does not yet exist, any call to `logging::log_message()` (or `main::run()` line 147) creates the empty `cipherclipboard` directory as a side effect, causing subsequent startups to switch `get_app_data_dir()` away from `rusty-clipboard` and strand the legacy database.
5. **Recommendation**: Extract `app_data_dir()` into a single shared authority (e.g., in `src/platform/fs_security.rs` or `src/logging.rs`) and call it from both `main::run()` and `logging::log_dir()`.
6. **Migration risk**: Low — unifying the path resolution preserves legacy directory detection before creating `app.log`.

## Quick Wins
1. **A-3**: Unify `get_app_data_dir()` (`src/main.rs:273`) and `logging::log_dir()` (`src/logging.rs:13`) into one shared function (~0.5 h).
2. **A-1 (logging & app)**: Move the Unix `0o600` file open in `src/logging.rs:67-71` to `src/platform/fs_security.rs` and the macOS modifier check in `src/app/mod.rs:1714` to `src/platform/mod.rs` (~1 h).
3. **A-1 (main)**: Move the 4 `#[cfg(target_os = ...)]` blocks in `src/main.rs:37,65,90,128` behind safe `platform::*` facade helpers (~1 h).

## Recommended Actions
1. **A-3 first**: Centralize data-directory resolution so `logging::log_dir()` and `main::run()` never disagree on legacy vs. current data directories.
2. **A-1 second**: Complete the migration of `#[cfg(target_os = ...)]` and `#[cfg(unix)]` blocks out of `src/logging.rs`, `src/main.rs`, `src/app/mod.rs`, and `src/tray.rs` into `src/platform/`.
3. **A-2 third**: Decompose `ClipboardApp` state into sub-structs for window lifecycle, history filtering, and background worker ownership.

## Coverage
- **1. Crate and Target Structure** — No findings in this category. `Cargo.toml` defines a single binary target `CipherClipboard` (`Cargo.toml:13-15`). Target-specific dependencies (`tray-icon` and `windows-sys` on Windows, `objc` on macOS, `x11rb` on Linux at `Cargo.toml:34-54`) each match target-gated platform modules. `Cargo.lock` is tracked in git and release workflows build with `--locked`.
- **2. Module Boundaries** — Findings A-2 and A-3. All 17 top-level modules are private to the binary crate (`src/main.rs:4-20`), and `unreachable_pub = "warn"` (`Cargo.toml:93`) prevents accidental `pub` leakage.
- **3. Platform Facade** — Finding A-1. `src/platform/mod.rs` and `src/platform/fs_security.rs` are 100% safe Rust and use `use {linux, macos, windows} as backend;` (`src/platform/mod.rs:20-25`) plus uniform fallback types (`QuoteHotkeyFallback` at `src/platform/mod.rs:31-54`).
- **4. Concurrency Ownership** — No findings in this category. Notice that both concurrency findings from the previous review (`A-1` paste worker shutdown/join and `A-2` duplicated two-phase shutdown) have been completely resolved: `PasteWorker` (`src/platform/mod.rs:572-633`) now holds `Mutex<Option<Sender<PasteRequest>>>` and `Mutex<Option<JoinHandle<()>>>`, drops the sender and joins the thread in `shutdown_paste_worker()`, and `ClipboardApp::shutdown_resources()` (`src/app/mod.rs:1593-1609`) is shared by both `Drop` (`:1624-1628`) and macOS `shutdown_and_exit` (`:1618-1621`).
- **5. UI Event-Loop Architecture** — No findings in this category. `impl eframe::App for ClipboardApp` (`src/app/mod.rs:1515-1585`) places all hidden-window/background wake handlers in `logic()` (`:1528-1547`) and all keyboard/pointer shortcuts and rendering in `ui()` (`:1549-1584`), using exact-deadline repaints (`ctx.request_repaint_after(...)` at `:1537`) and `FilterCounts::from_items` precomputation in `refresh_items()` (`:326`).
- **6. Error and Data Flow** — No findings in this category. Clipboard capture (`src/monitor.rs:139-236`) classifies text outside the DB mutex (`src/db.rs:450`), executes deduplication + insert + `prune_in_tx` in a single `conn.transaction()?` (`src/db.rs:456-494`, `:510-538`, `:563-600`), escapes `LIKE` wildcards while excluding `kind != 'secret'` (`src/db.rs:625-655`), and loads image blobs on demand (`src/db.rs:734-745`).
- **7. API Guidelines (public library surface)** — No public library API surface found. `Cargo.toml` defines only a `[[bin]]` target and all modules are private (`pub(crate)`).
