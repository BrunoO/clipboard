# Unsafe & FFI Review - 2026-11-21-b

## Executive Summary
- **Health Score**: 9/10
- **Critical Issues**: 0
- **High Issues**: 0
- **Total Findings**: 2
- **Estimated Remediation Effort**: 2 hours
- **Evidence Commands Run**: `grep -RInE 'unsafe|extern "system"|extern "C"|#\[link|#!?\[allow\(unsafe_code\)' src`; `grep -RInE '\bas\b|addr_of|&raw|repr\(|Cell<\*mut|static mut' src/platform`; `grep -RIn "SAFETY" src/platform`; `cargo clippy --all-targets --all-features -- -D warnings`; direct source reads of `Cargo.toml`, `src/platform/mod.rs`, `src/platform/fs_security.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/platform/linux.rs`, `docs/SECURITY_LESSONS.md`, `docs/MULTIPLATFORM_LESSONS.md`.

## Findings

**Unsafe Inventory (verified via `grep -RInE 'unsafe|extern "system"|extern "C"|#\[link|#!?\[allow\(unsafe_code\)' src`):**

| File | Construct | Purpose | Safe Wrapper Benefit |
|---|---|---|---|
| `src/platform/macos.rs` | `#![allow(unsafe_code)]` (L1); `extern "C" fn on_app_reopen` (L122); `#[link(..., kind = "framework")] extern "C"` blocks (L220-241); 14 `unsafe` / `autoreleasepool(\|\| unsafe)` blocks (L46, 57, 73, 87, 145, 207, 245, 250, 276, 301, 326, 352, 384, 396, 435) — all 14 preceded by `// SAFETY:` comments | AppKit/Objective-C messaging, CG keyboard events, `NSPasteboard` change counts & concealed markers, Accessibility trust check | Exposed as safe `pub(super)` / `pub(crate)` functions returning `bool`, `Option<u64>`, or `()`; zero raw pointers cross module boundaries |
| `src/platform/windows.rs` | `#![allow(unsafe_code)]` (L1); 2 `unsafe extern "system" fn enum_proc` callbacks (L116, 310); 4 internal `unsafe fn` helpers with `# Safety` doc sections (L79, 528, 600, 636); 25 `unsafe` blocks (L144, 163, 194, 207, 230, 284, 342, 371, 399, 447, 456, 476, 489, 497, 508, 550, 565, 685, 707, 753, 770, 806, 823) — all preceded by `// SAFETY:` comments | Win32 window enumeration/focus, `SendInput` paste, clipboard privacy format registration/inspection, UIPI token integrity query, DWM dark title bar, DPAPI key protection | Exposed as safe `pub(super)` / `pub(crate)` functions returning `bool`, `Option<u64>`, or `Result<_, String>`; all Win32 handles closed and DPAPI buffers zeroized before `LocalFree` |
| `src/platform/mod.rs` | None (`unsafe_code = "deny"` inherited) | Cross-platform facade and `PasteWorker` | 100% safe Rust |
| `src/platform/fs_security.rs` | None (`unsafe_code = "deny"` inherited) | Key file permissions, `icacls` invocation, DPAPI delegation | 100% safe Rust |
| `src/platform/linux.rs` | None (`unsafe_code = "deny"` inherited) | X11 session, atom interning, selection inspection, `QuoteHotkeyFallback` via `x11rb` | 100% safe Rust |

**Note on Resolved Prior Findings**: All 6 findings from the earlier `2026-11-21` Unsafe & FFI review (`UF-1` `// SAFETY:` comments on all unsafe blocks, `UF-2` `c"..."` literals in `macos.rs:446-447`, `UF-3` DPAPI input-only `// SAFETY:` documentation in `windows.rs:752,805`, `UF-4` `EnumWindows` callback aliasing `// SAFETY:` documentation in `windows.rs:115,309,341`, `UF-5` `TOKEN_MANDATORY_LABEL` `ret_len` bounds validation in `windows.rs:658-666`, and `UF-6` checked `i32::try_from(std::process::id())` in `macos.rs:286`) were verified in source as completely fixed.

### Critical
No findings in this category.

### High
No findings in this category.

### Medium

**UF-1 — `integrity_level` does not validate SID sub-authority offset against `ret_len` before dereferencing `*rid`**
1. **Boundary**: `src/platform/windows.rs:658-677`, Windows UIPI mandatory integrity check (`target_is_elevated` -> `integrity_level`).
2. **Severity**: Medium
3. **Evidence** — `src/platform/windows.rs:658-677`:
   ```rust
               let min_len =
                   u32::try_from(std::mem::size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(u32::MAX);
               if ok != 0 && (ret_len < min_len || ret_len > buffer_len) {
                   crate::logging::log_message(
                       "WARN",
                       &format!("Unexpected TOKEN_MANDATORY_LABEL length {ret_len}; skipping UIPI elevation check"),
                   );
               }
               if ok != 0 && ret_len >= min_len && ret_len <= buffer_len {
                   let info = &*std::ptr::addr_of!(buf).cast::<TOKEN_MANDATORY_LABEL>();
                   if !info.Label.Sid.is_null() {
                       let count = GetSidSubAuthorityCount(info.Label.Sid);
                       if !count.is_null() && *count > 0 {
                           let rid = GetSidSubAuthority(info.Label.Sid, u32::from(*count - 1));
                           if !rid.is_null() {
                               level = Some(*rid);
                           }
                       }
                   }
               }
   ```
4. **Violated or unverified precondition**: While `ret_len >= size_of::<TOKEN_MANDATORY_LABEL>()` and `ret_len <= buffer_len` (256 bytes) are now checked, `TOKEN_MANDATORY_LABEL` is only the 16-byte `SID_AND_ATTRIBUTES` header (`info.Label.Sid` points to the inline `SID` structure immediately following the header inside `buf`). `GetSidSubAuthorityCount` reads byte offset 1 of the `SID`, and `GetSidSubAuthority(sid, *count - 1)` computes a pointer `8 + 4 * (*count - 1)` bytes past the start of the `SID` without checking that `Sid` lies within `buf[..ret_len]` or requires `IsValidSid(info.Label.Sid) != 0`.
5. **Impact**: `[Inferred]` Under normal Win32 kernel behavior, `GetTokenInformation(TokenIntegrityLevel)` writes a well-formed 1-sub-authority SID (28 bytes total, well within the 256-byte zeroed `TokenIntegrityBuf`). However, calling `IsValidSid(info.Label.Sid) != 0` before `GetSidSubAuthorityCount`/`GetSidSubAuthority` is the standard Win32 defense-in-depth contract to ensure the inline SID header and sub-authority count are valid.
6. **Recommendation**: Call `IsValidSid(info.Label.Sid) != 0` (from `windows_sys::Win32::Security::IsValidSid`) before calling `GetSidSubAuthorityCount(info.Label.Sid)`.
7. **Verification**: Compile and run Windows CI test suite and UIPI smoke check.

### Low

**UF-2 — `setup_reopen_observer` returns early inside `Once::call_once` before releasing `observer` if `defaultCenter` is unexpectedly null**
1. **Boundary**: `src/platform/macos.rs:155-160`, macOS AppKit notification observer setup.
2. **Severity**: Low
3. **Evidence** — `src/platform/macos.rs:155-160`:
   ```rust
               let observer_cls = class!(CipherReopenObserver);
               let observer: *mut Object = msg_send![observer_cls, new];
               let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
               if center.is_null() || observer.is_null() {
                   return;
               }
   ```
4. **Violated or unverified precondition**: `[observer_cls new]` returns a `+1` retained Objective-C instance (unlike autoreleased convenience constructors). If `observer` is non-null and `center` is null, the early `return` skips `REOPEN_OBSERVER.with(|cell| cell.set(observer))` without sending `[observer release]`, leaking the `+1` allocation.
5. **Impact**: `[Inferred]` Negligible in practice (`[NSNotificationCenter defaultCenter]` is never null in a running macOS process and `INIT.call_once` runs at most once per process), but releasing `observer` on the error path makes the ownership balance (`new` <-> `release`) locally airtight.
6. **Recommendation**: Check `if center.is_null()` before calling `msg_send![observer_cls, new]`, or send `let _: () = msg_send![observer, release];` before returning when `center.is_null() && !observer.is_null()`.
7. **Verification**: macOS `cargo clippy --all-targets --all-features -- -D warnings` and macOS quit smoke test.

## Quick Wins
1. **UF-2**: Move `let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];` above `let observer: *mut Object = msg_send![observer_cls, new];` in `src/platform/macos.rs:155-160` (~0.25 h).
2. **UF-1**: Add `IsValidSid(info.Label.Sid) != 0` check before `GetSidSubAuthorityCount` in `src/platform/windows.rs:668` (~0.5 h).

## Recommended Actions
1. **UF-1**: Add `IsValidSid` validation in `src/platform/windows.rs:668` to complete the `TOKEN_MANDATORY_LABEL` inline-SID safety contract.
2. **UF-2**: Reorder `defaultCenter` lookup before `[CipherReopenObserver new]` in `src/platform/macos.rs:156-160` so `+1` retained allocation cannot leak on early return.
3. Preserve all current FFI invariants: `unsafe_code = "deny"` in `Cargo.toml:91`, 100% `// SAFETY:` coverage across `macos.rs` and `windows.rs`, `is_main_thread()` guards, `autoreleasepool` wrappers, `canBecomeMainWindow` filtering, `thread_local! { Cell<*mut Object> }`, and DPAPI `Zeroizing` + `slice.zeroize()` before `LocalFree`.

## Coverage
- **1. Containment** — No findings in this category. `Cargo.toml:91` sets `unsafe_code = "deny"`. `#![allow(unsafe_code)]` appears only in `src/platform/macos.rs:1` and `src/platform/windows.rs:1`. `src/platform/mod.rs`, `src/platform/fs_security.rs`, `src/platform/linux.rs`, and all non-platform modules are 100% safe Rust.
- **2. Unsafe Contract Quality** — Finding UF-1. Every single `unsafe` block and `unsafe extern "system" fn` in `src/platform/macos.rs` (14 sites) and `src/platform/windows.rs` (25 sites) now has an immediately preceding `// SAFETY:` comment, and all 4 internal `unsafe fn` helpers (`target_keyboard_layout`, `set_zero_dword_format`, `captured_target_pid`, `integrity_level`) carry `# Safety` doc sections.
- **3. Raw Pointers and Layout** — Finding UF-1. `&raw mut`, `&raw const`, and `std::ptr::addr_of!` / `std::ptr::addr_of_mut!` are used consistently throughout `src/platform/windows.rs`. `TokenIntegrityBuf` is `#[repr(C, align(8))]` (`src/platform/windows.rs:626-627`) with `ret_len` bounds checked against `size_of::<TOKEN_MANDATORY_LABEL>()` and `buf.0.len()` (`:658-666`). All C-string literals in `src/platform/macos.rs` use `c"..."` (`:167`, `:182`, `:253`, `:446-447`). Integer conversions across FFI use `try_from` or `.cast_unsigned()`.
- **4. Mac AppKit / Objective-C** — Finding UF-2. All `NSApplication` helpers (`is_active`, `hide_app`, `order_front_main_window`, `setup_reopen_observer`, `teardown_reopen_observer`) are guarded by `is_main_thread()` (`src/platform/macos.rs:44-50`, `:53`, `:69`, `:83`, `:140`, `:201`). Objective-C calls are wrapped in `objc::rc::autoreleasepool`. `[ns_app windows]` is filtered with `canBecomeMainWindow` (`:113-116`). `on_app_reopen` defers window ordering via `REOPEN_REQUESTED.store(true, Ordering::Release)` and `request_repaint()` (`:122-128`). `REOPEN_OBSERVER` is stored in `thread_local! { Cell<*mut Object> }` (`:132-136`).
- **5. Windows FFI** — Finding UF-1. Win32 symbols are linked via `windows-sys` (`Cargo.toml:36-47`). `AttachThreadInput` attach status is checked before detach (`src/platform/windows.rs:179-188`, `:256-265`). DPAPI uses `CRYPTPROTECT_UI_FORBIDDEN` (`:760`, `:813`) and zeroizes the OS buffer before `LocalFree` (`:824-828`). `icacls.exe` is resolved from `%SystemRoot%\System32` and run with `CREATE_NO_WINDOW` in `src/platform/fs_security.rs:165,198-204`. UIPI check uses captured `PREVIOUS_WINDOW` (`src/platform/windows.rs:600-617`, `:686-696`) and fails closed via `uipi_should_skip_paste` (`src/platform/mod.rs:359-371`).
- **6. Linux X11 (safe backend)** — No findings in this category. `src/platform/linux.rs` is 100% safe Rust using `x11rb`. It maintains a thread-local `X11Session` (`src/platform/linux.rs:66-97`) with pipelined atom interning (`:33-46`), a 5-second reconnect backoff on pure Wayland (`:14`, `:83`), `ev.requestor != win` filtering (`:328`), and clean `ungrab_all` on shutdown or channel disconnect (`:468-475`, `:549`, `:595`, `:603`).
- **7. Threading and Reentrancy** — No findings in this category. FFI callbacks (`on_app_reopen` on macOS, `enum_proc` on Windows) do not re-entrantly lock held mutexes; `PasteWorker::shutdown` (`src/platform/mod.rs:589-607`) and `ClipboardApp::shutdown_resources` (`src/app/mod.rs:1593-1609`) release and join all background workers deterministically.
- **8. Tooling** — `cargo clippy --all-targets --all-features -- -D warnings` was executed on the Linux host and exited with code 0 (zero warnings). Miri was not run (`cargo +nightly miri` is not installed in this environment and the target-gated FFI code invokes native OS APIs).
