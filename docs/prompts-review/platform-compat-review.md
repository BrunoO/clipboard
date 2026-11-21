# Rust Platform Compatibility Review Prompt

**Purpose:** Audit macOS, Windows, and Linux compatibility using `#[cfg]`/target-triple evidence and this project's multiplatform invariants.

---

## Prompt

````markdown
You are a Principal Rust cross-platform engineer. Review this crate's platform compatibility and OS integration. Follow the master orchestrator's Global Evidence Rules and treat `docs/MULTIPLATFORM_LESSONS.md` as normative.

## Scope

- Targets: `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` (as declared in `deny.toml`).
- OS modules: `src/platform/mod.rs` (safe facade), `macos.rs` (unsafe allowed), `windows.rs` (unsafe allowed), `linux.rs` (safe).
- Cross-cutting concerns: clipboard, hotkeys, keyring, window/tray lifecycle, synthetic paste, file permissions, packaging, CI.

## Grounding Rules

1. Cite exact `#[cfg]`, `#[cfg_attr]`, target dependency, workflow, or FFI snippet.
2. Do not claim a platform is broken without code/config evidence; `[Inferred]` otherwise.
3. Empty categories get `No findings in this category.`
4. Do not require nonexistent scripts like `find_class_struct_mismatches.py` or `git submodule status`.

## Review Process

### 1. Configuration Consistency
- Standardize on `#[cfg(target_os = "windows")]` / `"macos"` / `"linux"`; use `#[cfg(unix)]` only for POSIX APIs.
- Do not mix `#[cfg(windows)]` and `#[cfg(target_os = "windows")]` in the same file.
- Check `Cargo.toml` target gates match source `#[cfg]` usage and CI matrix.
- Check `deny.toml` target triples still match release targets.
- Check coordinated `eframe`/`egui_extras`/`egui_material_icons` versions.

### 2. Safe Facade Discipline
- `platform/mod.rs` stays 100% safe and thin.
- Cross-platform capabilities have OS-agnostic names and no-op/default stubs on unsupported OSes.
- Single-OS APIs live in `platform::windows::*` / `platform::macos::*`; no redundant pass-through wrappers with `cfg_attr(allow(dead_code))`.
- Pure helpers gated with `#[cfg(any(target_os = "...", test))]` run on all CI hosts.

### 3. Windows
- Tray icon remains live when window hidden; fallback to minimized if tray init fails.
- UIPI check uses captured `PREVIOUS_HWND`, fails closed on access denied, defers warning toast.
- Restore minimized window with `IsIconic`, attach to foreground thread, detach only on success.
- Keyboard layout detected via `GetKeyboardLayout`/`MapVirtualKeyExW`/`ToUnicodeEx`; re-detect action exists.
- Explicit `#[link]` for Win32 libraries; DPAPI fallback, `icacls.exe` from System32, `CREATE_NO_WINDOW`.
- Clipboard transient failures retried; format IDs cached with checked conversions.
- Watch duplicate `windows` crate versions after GUI upgrades.

### 4. macOS
- AppKit calls guarded by `is_main_thread()`; ObjC calls in autoreleasepool.
- `[NSApplication windows]` filtered with `canBecomeMainWindow`.
- No re-entrant window mutation in `NSNotificationCenter` callbacks.
- Accessibility permission status and deep link surfaced.
- Full-size content view/traffic-light padding; Dock reopen restores hidden window.
- Quit path runs cleanup then `process::exit(0)`; smoke test remains green.

### 5. Linux (X11 & Wayland)
- Pure Wayland degrades gracefully; X11 reconnect backoff (5s) instead of per-frame retries.
- `thread_local! X11Session` with persistent connection and interned atoms.
- `QuoteFallback` uses blocking `recv()` when idle and releases grabs on shutdown/disconnect.
- No X11 clipboard change counter; unchanged/oversized text does not fall through to image reads.
- Host/layout-agnostic tests; no US QWERTY assumptions.

### 6. Cross-Cutting Data and UI
- File paths, separators, case sensitivity, line endings, locale, and encodings handled safely.
- Permissions use OS-appropriate APIs (`OpenOptionsExt::mode`, ACLs, DPAPI).
- Clipboard concealed markers honored/emitted on every platform.
- Hotkey and paste behavior consistent; failures explained in UI.
- Error messages use OS-appropriate recovery steps.

### 7. CI Verification
- Linux, macOS, Windows jobs run tests; clippy runs on each target OS.
- No `continue-on-error` on platform jobs.
- Cross-target `cargo check` commands if toolchains installed; otherwise cite CI coverage.
- Platform smoke tests (macOS quit) present and passing.

## Commands (run if available; paste exact output)

```bash
grep -RInE '#\[cfg|cfg_attr|target_os|unix\)' src Cargo.toml deny.toml
cargo check --target x86_64-unknown-linux-gnu --all-targets --all-features --locked
cargo check --target x86_64-apple-darwin --all-targets --all-features --locked
cargo check --target x86_64-pc-windows-msvc --all-targets --all-features --locked
cargo tree -d
```

## Finding Format

1. **Platform / boundary** — OS and file/function.
2. **Severity** — Critical / High / Medium / Low.
3. **Evidence** — `path:line` snippet, workflow lines, or command output.
4. **Compatibility issue** — compile failure, runtime crash, UX mismatch, or missing stub.
5. **Impact** — affected users/platforms and failure mode.
6. **Recommendation** — concrete code/CI/docs change and verification steps.

## Output Sections

- Target matrix (observed support only).
- Findings by severity.
- Quick Wins.
- Recommended Actions.
- Coverage checklist with `No findings in this category.` where empty.
````

---

## Usage

Use as step 14 of the full review, in Pre-Release Review, and after any OS/dependency upgrade.
