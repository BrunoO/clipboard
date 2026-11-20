# Multiplatform Lessons Learned

Distilled from developing, hardening, and shipping CipherClipboard across **Windows**, **macOS**, and **Linux (X11 & Wayland)**. Future contributors and coding agents should treat these as concrete invariants when adding OS-specific features, FFI bindings, or cross-platform UI/packaging changes.

---

## 1. Organizing Platform-Specific Rust Code

- **Use OS submodules (`src/platform/{mod,macos,windows,linux}.rs`), not a single monolith.** Keep `src/platform/mod.rs` as a thin, 100% safe facade (`unsafe_code = "deny"`), and confine `#![allow(unsafe_code)]` strictly to the FFI backend files that actually need it (`src/platform/macos.rs` and `src/platform/windows.rs`). Safe backends (`src/platform/linux.rs` using `x11rb`) and platform-agnostic utilities (`src/logging.rs`) must never live under `#![allow(unsafe_code)]`.
- **Separate cross-platform capabilities from genuinely single-OS APIs.**
  - Give cross-platform capabilities OS-agnostic names in `platform::*` with clean no-op/default stubs where unsupported (e.g., `platform::set_title_bar_dark_mode(is_dark)`, `platform::quote_key_is_ugrave()`, `platform::clipboard_change_token()`).
  - Expose genuinely single-OS APIs directly through target-gated submodules (`platform::windows::dpapi_protect`, `platform::windows::target_is_elevated`, `platform::windows::show_error_dialog`, `platform::macos::is_trusted`) instead of adding `windows_*` pass-through wrappers with `#[cfg_attr(not(target_os = "..."), allow(dead_code))]`.
- **Standardize `#[cfg]` predicates across the crate.** Use `#[cfg(target_os = "windows")]` / `"macos"` / `"linux"` for OS-specific code, and reserve `#[cfg(unix)]` strictly for POSIX APIs (e.g., `PermissionsExt::from_mode(0o600)`). Never mix `#[cfg(windows)]` and `#[cfg(target_os = "windows")]` in the same file.
- **Collapse repetitive inline `#[cfg]` branches with `const fn` helpers.** In `src/theme.rs`, `const fn platform_select<T: Copy>(macos: T, windows: T, other: T) -> T` replaces dozens of 3-way `#[cfg(target_os = ...)]` blocks while keeping every palette and geometry helper `const`.
- **Make pure platform logic testable on every OS.** Gate pure helpers like `resolve_windows_icacls_path` or font-candidate builders with `#[cfg(any(target_os = "windows", test))]` so their unit tests run on Linux and macOS CI as well as Windows.
- **Keep cross-platform tests layout- and host-agnostic.** Never hardcode assumptions about the CI runner's OS configuration (e.g., Windows CI runners use US QWERTY, so AZERTY hotkey tests must branch on `platform::quote_key_is_ugrave()` rather than unconditionally asserting `Code::Backquote` under `#[cfg(target_os = "windows")]`).

---

## 2. Windows-Specific Lessons

- **System tray is mandatory when hiding the window.** On Windows, setting `Visible(false)` removes the window from the taskbar and `Alt+Tab`. Always keep a live tray icon (`src/tray.rs`) and fall back to `Minimized(true)` if tray initialization fails or Explorer restarts so users are never locked out.
- **Respect UIPI (User Interface Privilege Isolation) on auto-paste.** Windows silently drops `SendInput` keystrokes sent from a medium-integrity process to an elevated (Administrator) window.
  - Check `PREVIOUS_HWND` (the captured paste target), *not* `GetForegroundWindow()` (which is still CipherClipboard during `hide_window` because `ViewportCommand::Visible(false)` is queued until end-of-frame).
  - A non-elevated process receives `ERROR_ACCESS_DENIED` when calling `OpenProcessToken(..., TOKEN_QUERY)` on an elevated process — treat `ERROR_ACCESS_DENIED` as elevated (fail closed) and skip synthetic `Ctrl+V`.
  - Defer the user-facing warning toast (`pending_toast`) until the next `show_window()` so it isn't lost on a window that is immediately hiding.
- **Restoring minimized windows safely.** Check `IsIconic(hwnd)` before calling `ShowWindow`: send `SW_RESTORE` only when minimized and `SW_SHOW` otherwise (unconditional `SW_RESTORE` un-maximizes a maximized window). Attach `AttachThreadInput` to the *current foreground* thread (not our own thread) and only detach if `AttachThreadInput(..., 1)` succeeded.
- **Keyboard layout awareness for global hotkeys.** `global-hotkey` maps `Code::Quote` to `VK_OEM_7` assuming US QWERTY, whereas on French/Belgian AZERTY the physical `ù` key produces `VK_OEM_3` (`Code::Backquote`). Query the active layout via `GetKeyboardLayout` + `MapVirtualKeyExW` + `ToUnicodeEx` on scan code `0x0028` instead of hardcoding a Windows-wide override, and provide a "Re-detect keyboard layout" action in Settings.
- **Link native Win32 libraries explicitly.** Always annotate `extern "system"` blocks with `#[link(name = "kernel32")]`, `#[link(name = "advapi32")]`, `#[link(name = "dwmapi")]`, `#[link(name = "crypt32")]`, etc., rather than relying on transitive crates to link them.
- **Watch for duplicate `windows` crate versions in `Cargo.lock`.** When upgrading `eframe`/`wgpu`, `gpu-allocator` (which accepts a range like `>=0.53, <=0.62`) can remain pinned to an older `windows` version in `Cargo.lock` while `wgpu-hal` pulls `windows 0.62`, causing trait-bound errors on `D3D12_RESOURCE_DESC` / `ID3D12Heap` that only surface on `windows-latest` CI. Run `cargo update -p gpu-allocator` to unify them.
- **Protect fallback key files and avoid console flashes.** Encrypt fallback `.encryption_key` files with user-scoped DPAPI (`CRYPTPROTECT_UI_FORBIDDEN`), resolve `icacls.exe` from `%SystemRoot%\System32` (never bare `PATH`), and pass `CREATE_NO_WINDOW` (`0x0800_0000`).
- **Handle transient clipboard unavailability.** On Windows (logon auto-start, RDP reconnect, lock screen), `Clipboard::new()` can fail briefly. Both the background monitor and the UI `clipboard_writer` (`ensure_clipboard_writer`) must lazily retry instead of permanently disabling clipboard support.

---

## 3. macOS-Specific Lessons

- **Never call `[NSApplication sharedApplication]` off the main thread.** AppKit asserts main-thread execution and traps with `SIGTRAP` — including inside `cargo test` worker threads! Guard every `NSApplication` helper (`is_active`, `hide_app`, `order_front_main_window`) with `[NSThread isMainThread]` (from Foundation, which is thread-safe).
- **Wrap Objective-C calls in `objc::rc::autoreleasepool`.** Background threads and short-lived FFI helpers without an active pool leak autoreleased `NSString`/`NSArray` objects.
- **Filter `[ns_app windows]` with `canBecomeMainWindow` and avoid re-entrant `NSNotificationCenter` window mutations.** AppKit's window list includes private internal panels (Touch Bar, popovers, tooltips); ordering those front or mutating windows synchronously inside `NSNotificationCenter` callbacks triggers `_NSTouchBarFinderObservation` KVO crashes.
- **Accessibility permission is required for synthetic `Cmd+V`.** Surface permission status (`AXIsProcessTrusted`) and direct Settings deep-link (`x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`) in the macOS-only Settings section.
- **Window chrome & Dock reopen.** macOS uses full-size content view with hidden titlebar (requiring `header_left_padding` for traffic lights) and expects Dock icon clicks (`NSApplicationDidBecomeActiveNotification` / `NSApplicationDidUnhideNotification`) to reopen a hidden window.
- **Quit must exit the process after our own cleanup (`on_exit` → `process::exit(0)`).** After `ViewportCommand::Close`, winit's `NSApplication run` loop keeps pumping; AppKit's next display-cycle flush then removes a Touch Bar KVO observer (`_NSTouchBarFinderObservation`, key path `nextResponder`) from the already torn-down `WinitView` and aborts with an uncaught `NSRangeException`. `ClipboardApp::on_exit` (macOS, only when `is_quitting`) runs the `Drop` cleanup (clear secret, stop hotkeys, join monitor, remove reopen observer) and calls `std::process::exit(0)`. Do **not** try to fix this with window visibility tweaks, `[NSApp hide:]`/activate changes, or delayed `Close`: the crash happens after teardown, so those never worked. Re-verify quit (button, Cmd+Q, tray; window visible and hidden) on every `eframe`/`winit` bump or `quit()`/`hide_window()` change.

---

## 4. Linux-Specific Lessons (X11 & Wayland)

- **Support pure Wayland gracefully alongside X11.** `x11rb::connect(None)` fails on pure Wayland without XWayland. Return `None` from `is_app_active_state()` so `hide_on_blur` falls back to `!is_focused`, and back off X11 reconnect attempts (5s) instead of spamming connection retries on every frame.
- **Keep X11 connections and interned atoms per-thread (`thread_local!`).** Sharing or re-opening X11 connections on every frame or 200ms monitor tick causes lock contention and event-queue interference.
- **Handle missing `global-hotkey` mappings on X11.** When `global-hotkey` fails to register `Code::Quote` on certain X11 layouts, fall back to a dedicated X11 key-grab listener (`QuoteFallback`) that blocks on `rx.recv()` when idle and releases all grabs cleanly on shutdown/disconnect.
- **Clipboard change counters are unavailable on X11/Wayland.** Return `None` from `clipboard_change_token()` and avoid falling through to `get_image()` when clipboard text is unchanged or oversized.

---

## 5. Cross-Platform CI, Clippy, & Ecosystem Upgrades

- **Never assume Linux Clippy covers macOS/Windows `#[cfg]` blocks.** CI runs `cargo clippy --all-targets --all-features -- -D warnings` on Linux, macOS, and Windows. Inside target-gated code, avoid `items_after_statements`, `borrow_as_ptr` (prefer `&raw mut` / `&raw const` / `ptr::addr_of_mut!`), unchecked `as` casts (prefer `u32::try_from`), and manual nul-terminated byte strings (prefer `c"..."`).
- **Upgrade the `egui` ecosystem as a coordinated trio.** `eframe`, `egui_extras`, and `egui_material_icons` must target the *same* `egui`/`epaint` minor version (otherwise font/context types mismatch). Always check `egui_material_icons`'s supported `egui` version before bumping `eframe`.
- **Keep `deny.toml` aligned with transitive dependencies.** Major UI/DB upgrades can both resolve old advisories (triggering `advisory-not-detected` warnings) and introduce new permissive font licenses (e.g., `Ubuntu-font-1.0` in `epaint_default_fonts`).
