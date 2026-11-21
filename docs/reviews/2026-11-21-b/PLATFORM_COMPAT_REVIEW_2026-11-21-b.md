# Platform Compatibility Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 8/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 3 (0 Critical, 0 High, 1 Medium, 2 Low)
- **Estimated Remediation Effort**: 2.5 hours
- **Evidence Commands Run**:
  - `grep -RInE '#\[cfg|cfg_attr|target_os|unix\)' src Cargo.toml deny.toml build.rs`
  - `grep -RInE '#\[cfg|cfg_attr|target_os|unix\)' src --exclude-dir=platform`
  - `rustup target list --installed` → `x86_64-unknown-linux-gnu`
  - `cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked` → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 12.11s`, `LINUX_TARGET_EXIT=0`
  - `cargo tree --locked -d --depth 1` → duplicate versions on Linux host (`x11rb` `0.13.2` + `0.14.0`, `smithay-client-toolkit` `0.19.2` + `0.20.0`, `calloop` `0.13.0` + `0.14.5`, `rustix` `0.38.44` + `1.1.6`, `syn` `2.0.119` + `3.0.10`, etc.)
  - `grep -n -A1 '^name = "windows-sys"\|^name = "windows"' Cargo.lock` → `windows` `0.62.2` (single version), `windows-sys` `0.52.0`, `0.59.0`, `0.61.2`

Compared to the `2026-11-21` review, four previously reported issues have been fixed:
1. `Cargo.toml:29-32` moved `winres = "0.1"` to `[build-dependencies]` and standardized all target sections (`Cargo.toml:34,49,52`) on `cfg(target_os = "...")`.
2. `build.rs:4` gates Windows resource emission solely on `std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")` without a host `#[cfg(windows)]` guard.
3. `src/platform/mod.rs:302-329,376-382` and `src/platform/linux.rs:240-243` now explicitly define `ConcealedMarkerSupport::DetectOnly` on Linux, invoke `linux::mark_clipboard_concealed()`, surface the limitation string in Settings (`src/app/settings_view.rs:471-478`), and test the contract (`src/platform/mod.rs:670-686,726-732`).
4. `src/platform/macos.rs:286-288` replaced the unchecked `std::process::id() as i32` cast with `let Ok(our_pid) = i32::try_from(std::process::id()) else { return; };`.

The remaining findings are one Medium architectural invariant violation (residual platform `#[cfg]` attributes outside `src/platform/`) and two Low items (missing macOS release-packaging job and duplicate `x11rb` versions on Linux).

### Target Matrix (Observed Support)

| Target Triple | Declared in `deny.toml` | Local `cargo check --target` | CI Host Coverage (`.github/workflows/ci.yml`) | Release Packaging (`.github/workflows/release.yml`) |
|---|---|---|---|---|
| `x86_64-unknown-linux-gnu` | `deny.toml:5` | **Pass** (`LINUX_TARGET_EXIT=0`) | `ubuntu-latest` (`ci.yml:19,45,118`) | `release-linux` on `ubuntu-22.04` (`release.yml:176-269`) |
| `x86_64-apple-darwin` | `deny.toml:6` | **Skipped** (target not installed locally) | `macos-latest` (`ci.yml:45,76`) | No macOS release job (Low Finding L-1) |
| `aarch64-apple-darwin` | `deny.toml:7` | **Skipped** (target not installed locally) | `macos-latest` (`ci.yml:45,76`) | No macOS release job (Low Finding L-1) |
| `x86_64-pc-windows-msvc` | `deny.toml:8` | **Skipped** (target not installed locally) | `windows-latest` (`ci.yml:45`) | `release-windows` on `windows-latest` (`release.yml:22-175`) |

## Findings

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

#### M-1 — Residual platform `#[cfg(target_os = ...)]`, `#[cfg(unix)]`, and `cfg!(target_os = ...)` attributes remain outside `src/platform/`
- **Platform / boundary**: Cross-platform architecture invariant (`AGENTS.md` / `docs/MULTIPLATFORM_LESSONS.md:10`) across `src/main.rs`, `src/app/mod.rs`, `src/settings.rs`, `src/theme.rs`, `src/logging.rs`, `src/tray.rs`, and `src/crypto.rs`.
- **Severity**: Medium
- **Evidence**: Normative rule `docs/MULTIPLATFORM_LESSONS.md:10` and `AGENTS.md`:
  > **Confine platform `#[cfg(target_os = ...)]` / `#[cfg(unix)]` attributes to `src/platform/` (`mod.rs`, `fs_security.rs`, `linux.rs`, `macos.rs`, `windows.rs`).** Do not scatter OS-conditional `#[cfg]` blocks across application, UI, settings, hotkey, theme, or crypto modules; instead, expose OS-agnostic capability helpers, constants, or no-op fallback types from `src/platform/`.

  Running `grep -RInE '#\[cfg|cfg_attr|target_os|unix\)' src --exclude-dir=platform` found OS-conditional attributes and `cfg!(target_os = ...)` checks in seven non-platform files:
  - `src/main.rs:37-38,65-70,90-91,128-134`:
    ```rust
    // src/main.rs:37-38
    #[cfg(target_os = "windows")]
    platform::windows::show_error_dialog(&format!("CipherClipboard failed to start:\n\n{e}"));
    // src/main.rs:90-91
    #[cfg(target_os = "windows")]
    platform::windows::activate_existing_instance(pid);
    // src/main.rs:128-134
    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }
    ```
  - `src/app/mod.rs:189,1517-1522,1617-1621,1714-1717`:
    ```rust
    // src/app/mod.rs:1517-1522
    #[cfg(target_os = "macos")]
    fn on_exit(&mut self) {
        if self.is_quitting {
            self.shutdown_and_exit();
        }
    }
    // src/app/mod.rs:1714-1717
    #[cfg(target_os = "macos")]
    if modifiers.mac_cmd || modifiers.command {
        parts.push("Cmd");
    }
    ```
  - `src/settings.rs:58,69,80,470`:
    ```rust
    // src/settings.rs:80
    let base_mod = if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    };
    // src/settings.rs:470
    const fn default_hide_on_blur() -> bool {
        !cfg!(target_os = "windows")
    }
    ```
  - `src/theme.rs:99-113` (`platform_select` using `#[cfg(target_os = "macos")]`, `#[cfg(target_os = "windows")]`, `#[cfg(not(any(target_os = "macos", target_os = "windows")))]`).
  - `src/logging.rs:67-71`:
    ```rust
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    ```
  - `src/tray.rs:5,18,34-57,59,149,154,161` (`#[cfg(target_os = "windows")]` / `#[cfg(not(target_os = "windows"))]`).
  - `src/crypto.rs:354,362` (unit test assertions gated on `#[cfg(unix)]` and `#[cfg(target_os = "windows")]`).
- **Compatibility issue**: While `src/hotkey.rs`, `src/app/settings_view.rs`, and `src/platform/fs_security.rs` were refactored to route through `src/platform/`, these seven modules still contain direct `#[cfg(target_os = ...)]`, `#[cfg(unix)]`, or `cfg!(target_os = ...)` predicates outside `src/platform/`.
- **Impact**: Platform branching remains partially split between `src/platform/` and application/UI modules, violating the repository's strict confinement rule in `AGENTS.md` and `docs/MULTIPLATFORM_LESSONS.md:10`.
- **Recommendation**:
  - Move or expose OS-agnostic helpers in `src/platform/mod.rs` (e.g., `platform::configure_native_viewport(viewport)`, `platform::show_startup_error_dialog(msg)`, `platform::show_panic_error_dialog(msg)`, `platform::activate_existing_instance(pid)`, `platform::exit_after_cleanup_if_needed()`, `platform::has_meta_modifier(modifiers)`, `platform::default_hide_on_blur()`, `platform::primary_hotkey_modifier()`, `platform::platform_select(...)`, and `platform::fs_security::open_private_log_file(path)`), and relocate the Windows `TrayService` implementation details under `src/platform/` (or document `src/tray.rs` and `src/theme.rs::platform_select` as permitted exceptions in `AGENTS.md`).

### Low

#### L-1 — No macOS release packaging job in `.github/workflows/release.yml` despite macOS bundle metadata in `Cargo.toml`
- **Platform / boundary**: macOS release distribution (`.github/workflows/release.yml` vs `Cargo.toml:17-27`).
- **Severity**: Low
- **Evidence**: `Cargo.toml:17-27` defines `[package.metadata.bundle]` (`icon = ["assets/icon.icns"]`, `identifier = "com.cipherclipboard.app"`) and `[package.metadata.bundle.macos]` (`NSKeychainUsageDescription`), and `deny.toml:6-7` lists `x86_64-apple-darwin` and `aarch64-apple-darwin`. However, `.github/workflows/release.yml` defines only:
  ```yaml
  # .github/workflows/release.yml:22
    release-windows:
      name: Build & Package Windows (x64)
  # .github/workflows/release.yml:176
    release-linux:
      name: Build & Package Linux (x64)
  ```
- **Compatibility issue**: macOS is built and smoke-tested in CI (`.github/workflows/ci.yml:45,74-114`), and `assets/icon.icns` is tracked in git, but tag releases do not build or attach macOS binaries/bundles.
- **Impact**: macOS users cannot download prebuilt release artifacts (`x86_64-apple-darwin` or `aarch64-apple-darwin`) from GitHub Releases without building from source.
- **Recommendation**: Either add a `release-macos` job to `.github/workflows/release.yml` (building `--release --locked` and packaging the `.app` bundle using the metadata in `Cargo.toml:17-27`), or document in `README.md` that macOS releases are currently source-only.

#### L-2 — Duplicate `x11rb` versions (`0.13.2` and `0.14.0`) in the Linux dependency graph
- **Platform / boundary**: Linux (`Cargo.toml:52-53` vs transitive Linux GUI/clipboard/hotkey dependencies).
- **Severity**: Low
- **Evidence**: `Cargo.toml:52-53` declares:
  ```toml
  [target.'cfg(target_os = "linux")'.dependencies]
  x11rb = { version = "0.14", features = ["xtest"] }
  ```
  Running `cargo tree --locked -d --depth 1` outputs:
  ```text
  x11rb v0.13.2
  ├── arboard v3.7.0
  ├── global-hotkey v0.10.0 (*)
  └── winit v0.30.13 (*)

  x11rb v0.14.0
  └── cipherclipboard v0.1.7 (/home/antigravity/BrunoO/clipboard)

  x11rb-protocol v0.13.2
  └── x11rb v0.13.2 (*)

  x11rb-protocol v0.14.0
  └── x11rb v0.14.0 (*)
  ```
- **Compatibility issue**: Both `x11rb 0.13.2` and `x11rb 0.14.0` (plus `x11rb-protocol`) are compiled on Linux because `arboard`, `global-hotkey`, and `winit` depend on `0.13.2` while `cipherclipboard` directly depends on `0.14`. (Each maintains its own X11 socket connection, so types do not cross the crate boundary.)
- **Impact**: Redundant compilation of `x11rb` and `x11rb-protocol` on Linux builds and an unrecorded duplicate in `deny.toml:45-48` (which currently documents only the `windows-sys` duplicate baseline).
- **Recommendation**: Either align the direct `x11rb` dependency in `Cargo.toml:53` with `0.13` (if `0.14`-only APIs are not required in `src/platform/linux.rs`) or document `x11rb 0.13.2`/`0.14.0` alongside `windows-sys` in the `deny.toml:45-48` duplicate-version baseline comment.

## Quick Wins
1. Expose `platform::open_private_append_file(path)` in `src/platform/fs_security.rs` to eliminate the `#[cfg(unix)]` block in `src/logging.rs:67-71`.
2. Move `default_hide_on_blur()`, `PRIMARY_HOTKEY_MODIFIER`, and `configure_native_viewport()` into `src/platform/mod.rs` to remove `#[cfg]` / `cfg!` from `src/settings.rs` and `src/main.rs`.
3. Document `x11rb 0.13.2` / `0.14.0` in `deny.toml:45-48` or align `Cargo.toml:53` to `x11rb = "0.13"` so only one `x11rb` version is compiled on Linux.

## Recommended Actions
1. **Complete Platform `#[cfg]` Confinement (M-1)**: Route the remaining OS-specific branches in `src/main.rs`, `src/app/mod.rs`, `src/settings.rs`, and `src/logging.rs` through `src/platform/mod.rs` and `src/platform/fs_security.rs`, and update `AGENTS.md` / `docs/MULTIPLATFORM_LESSONS.md` if `src/tray.rs` and `src/theme.rs::platform_select` are intended to remain in place.
2. **Clarify or Automate macOS Release Packaging (L-1)**: Add a `release-macos` job to `.github/workflows/release.yml` or document the macOS distribution model in `README.md`.
3. **Track Linux Duplicate Crates (L-2)**: Evaluate unifying `x11rb` to `0.13.2` or record the split in `deny.toml:45-48`.

## Coverage

- **1. Configuration Consistency**:
  - `Cargo.toml:34,49,52` consistently uses `cfg(target_os = "windows")`, `cfg(target_os = "macos")`, and `cfg(target_os = "linux")`; `winres = "0.1"` is a host `[build-dependencies]` entry (`Cargo.toml:29-32`).
  - `src/platform/fs_security.rs:75,107,115,119,240` uses `#[cfg(unix)]` strictly for POSIX `OpenOptionsExt::mode` / `PermissionsExt::from_mode`.
  - Coordinated GUI trio verified in `Cargo.toml:56-58` and `Cargo.lock`: `eframe 0.37.0`, `egui 0.37.0`, `egui_extras 0.37.0`, and `egui_material_icons 0.9.0`.
  - Residual `#[cfg]` outside `src/platform/` reported in **M-1**.
- **2. Safe Facade Discipline**:
  - `src/platform/mod.rs`, `src/platform/fs_security.rs`, and `src/platform/linux.rs` contain zero `unsafe` blocks (`unsafe_code = "deny"`).
  - `#![allow(unsafe_code)]` appears solely in `src/platform/macos.rs:1` and `src/platform/windows.rs:1`.
  - Single-OS APIs (`platform::windows::dpapi_protect`, `platform::windows::show_error_dialog`, `platform::windows::activate_existing_instance`, `platform::macos::is_trusted`) are exposed without redundant pass-through wrappers.
  - Pure Windows helpers (`windows_font_candidates` at `src/platform/mod.rs:218`, `WINDOWS_CONCEALED_DETECT_FORMATS` / `WINDOWS_CONCEALED_EMIT_FORMATS` / `windows_concealed_detected` / `uipi_should_skip_paste` at `src/platform/mod.rs:332-371`, and `resolve_windows_icacls_path` / `windows_icacls_grant` at `src/platform/fs_security.rs:134,146`) are gated with `#[cfg(any(target_os = "windows", test))]` and executed in the 97-test suite on Linux. No findings in this category.
- **3. Windows**:
  - Live tray check (`TrayService::is_live` at `src/tray.rs:150-152`) and fallback to `ViewportCommand::Minimized(true)` (`src/platform/mod.rs:396-411`).
  - UIPI check uses captured `PREVIOUS_WINDOW` (`src/platform/windows.rs:600-617`), fails closed when `integrity_level(target_pid)` is `None` or higher than `self_level` (`src/platform/mod.rs:359-371`, `src/platform/windows.rs:686-696`), and returns a user-facing warning toast (`src/platform/mod.rs:424-436`).
  - Minimized window restoration checks `IsIconic` before choosing `SW_RESTORE` vs `SW_SHOW` (`src/platform/windows.rs:164-168,348-352`), attaches to the foreground thread (`AttachThreadInput`), and detaches only when attached (`src/platform/windows.rs:179-188,256-265`).
  - Dynamic keyboard layout detection uses `GetKeyboardLayout` + `MapVirtualKeyExW` + `ToUnicodeEx` (`src/platform/windows.rs:79-112,563-593`) with a "Re-detect keyboard layout" button in Settings (`src/app/settings_view.rs:116-118,159-174`).
  - Win32 bindings use `windows-sys = "0.61"` (`Cargo.toml:36-47`); `windows` crate has only one version in `Cargo.lock` (`0.62.2` at `Cargo.lock:5300-5301`).
  - Fallback key files use DPAPI (`CRYPTPROTECT_UI_FORBIDDEN` at `src/platform/windows.rs:760`), resolve `icacls.exe` from `%SystemRoot%\System32` (`src/platform/fs_security.rs:135-144,198-199`), and pass `CREATE_NO_WINDOW` (`0x0800_0000` at `src/platform/fs_security.rs:165,203`).
  - Clipboard transient errors are retried (`init_clipboard_with_retry` at `src/monitor.rs:121-137`) and format IDs are cached with `AtomicU32` (`src/platform/windows.rs:437-452`). No findings in this category.
- **4. macOS**:
  - `is_main_thread()` guards all `NSApplication` calls (`src/platform/macos.rs:44-50,53,69,83,140,201`).
  - Objective-C calls are wrapped in `objc::rc::autoreleasepool` (`src/platform/macos.rs:87,145,207,250,276,301,326,384,396,435`).
  - `[ns_app windows]` is filtered with `canBecomeMainWindow` (`src/platform/macos.rs:113-116`).
  - `on_app_reopen` sets `REOPEN_REQUESTED` atomic and requests repaint instead of mutating windows re-entrantly (`src/platform/macos.rs:122-128`).
  - Accessibility permission status (`AXIsProcessTrusted`) and Settings deep-link (`x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`) are surfaced (`src/platform/macos.rs:243-272`, `src/app/settings_view.rs:386-414`).
  - Checked PID conversion `i32::try_from(std::process::id())` is used at `src/platform/macos.rs:286`.
  - Quit path runs `shutdown_resources()` then `std::process::exit(0)` in `ClipboardApp::on_exit` (`src/app/mod.rs:1517-1522,1617-1621`) and is verified by `macos-quit-smoke` in `.github/workflows/ci.yml:74-114`.
  - Missing macOS release job reported in **L-1**.
- **5. Linux (X11 & Wayland)**:
  - Pure Wayland degrades gracefully (`is_active()` returns `None` when X11 is unavailable, and `X11_RECONNECT_BACKOFF` waits 5s between connection attempts at `src/platform/linux.rs:14,73-97`).
  - `thread_local! { static X11_STATE: RefCell<X11State> }` maintains a persistent per-thread X11 connection and pipelined interned atoms (`src/platform/linux.rs:16-68`).
  - `QuoteHotkeyFallback` blocks on `rx.recv()` when idle (`grabs.is_empty()`) and releases grabs via `ungrab_all` on `Clear`, `Shutdown`, or channel `Disconnected` (`src/platform/linux.rs:574-606`).
  - `clipboard_change_token()` returns `None` on Linux (`src/platform/mod.rs:281-284`), and `poll_clipboard_text` returns `PollOutcome::Handled` when text is unchanged or oversized so `poll_clipboard_image` is not invoked (`src/monitor.rs:80-90,154-156`).
  - Concealed clipboard markers: incoming `x-kde-passwordManagerHint` is detected (`src/platform/linux.rs:261-319`), and outgoing marker limitation (`ConcealedMarkerSupport::DetectOnly`) is explicitly modeled, tested, and surfaced in UI (`src/platform/mod.rs:303-329`, `src/app/settings_view.rs:471-478`).
  - Duplicate `x11rb` versions reported in **L-2**.
- **6. Cross-Cutting Data and UI**:
  - Path construction uses `Path::join` / `PathBuf`; permissions use `OpenOptionsExt::mode(0o600)` / `PermissionsExt::from_mode(0o700)` on Unix and DPAPI + `icacls.exe` on Windows (`src/platform/fs_security.rs`).
  - Error messages provide OS-appropriate recovery commands (`Remove-Item -Recurse -Force` on Windows vs `rm -rf` on Unix in `src/platform/fs_security.rs:14-34`). No findings in this category.
- **7. CI Verification**:
  - `.github/workflows/ci.yml:38,45,67-72` runs Clippy (`--all-targets --all-features --locked -- -D warnings`) and tests (`--all-targets --locked --verbose`) across `ubuntu-latest`, `macos-latest`, and `windows-latest` with `fail-fast: false` and no `continue-on-error`.
  - `.github/workflows/ci.yml:74-114` runs the `macos-quit-smoke` job. No findings in this category.
