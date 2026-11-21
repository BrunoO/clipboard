# Unsafe & FFI Review - 2026-11-21

## Executive Summary
- **Health Score**: 7/10
- **Critical Issues**: 0
- **High Issues**: 1
- **Total Findings**: 6
- **Estimated Remediation Effort**: 20 hours
- **Evidence Commands Run**: `grep -RInE 'unsafe|extern "system"|extern "C"|#\[link|#!?\[allow\(unsafe_code\)' src` (parent-captured `unsafe-inventory.log`, 95 lines, read verbatim); `grep -RIn "SAFETY" src/platform` → `No matches found`; `grep -RIn "#[link" src/platform` → 3 matches in `macos.rs`; `grep -RIn "extern \"system\"" src/platform` → 2 matches in `windows.rs`; direct source reads of `Cargo.toml`, `src/platform/mod.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/platform/linux.rs`, `src/crypto.rs`. No cargo/clippy/test/Miri command was run for this report — build and static-analysis results are out of scope and belong to the separate static-analysis report; verification steps below are recommendations only.

## Findings

**Unsafe inventory (verbatim grep results)**:

| File | Construct | Purpose | Safe wrapper benefit |
|---|---|---|---|
| `src/platform/macos.rs` | `#![allow(unsafe_code)]` (L1); `unsafe`/`autoreleasepool(‖ unsafe)` at L37, 47, 62, 75, 132, 193, 232, 236, 261, 283, 307, 332, 363, 374, 412 | AppKit/Objective-C messaging, CG keyboard events, NSPasteboard types, AX permission | All exposed through `pub(super)`/`pub(crate)` safe functions; no raw pointer crosses into `platform::*` |
| `src/platform/windows.rs` | `#![allow(unsafe_code)]` (L1); `unsafe extern "system"` callbacks at L111, 298; `unsafe` blocks at L138, 156, 186, 198, 220, 273, 329, 357, 384, 431, 439, 458, 469, 474, 482, 495, 516, 530, 560, 589, 628, 649, 690, 706, 736, 752 | Win32 focus/HWND capture, clipboard formats, UIPI integrity check, DPAPI | All exposed through safe `pub(super)`/`pub(crate)` functions; `windows-sys` types are converted to `Option`/`bool`/`Result` |
| `src/platform/mod.rs` | none — safe facade | OS-agnostic dispatch and focus-paste worker | N/A (0% unsafe) |
| `src/platform/linux.rs` | none — `x11rb` safe API | X11 session, atoms, paste | N/A (0% unsafe) |

Containment is confirmed: the `grep -RIn "unsafe" src` output lists matches only in `src/platform/macos.rs` and `src/platform/windows.rs`, and `Cargo.toml:87-88` reads:
```toml
[lints.rust]
unsafe_code = "deny"
```

### Critical
No findings in this category.

### High

**UF-1 — No `// SAFETY:` justification on any unsafe block in either FFI backend**
1. **Boundary**: `src/platform/macos.rs` and `src/platform/windows.rs`, all unsafe blocks.
2. **Severity**: High
3. **Evidence** — verbatim command output for `grep -RIn "SAFETY" src/platform`:
   ```
   No matches found
   ```
   Representative undocumented blocks, `src/platform/macos.rs:332-333`:
   ```rust
       unsafe {
           let source = CGEventSourceCreate(SOURCE_STATE_COMBINED_SESSION);
   ```
   `src/platform/windows.rs:690-699`:
   ```rust
       let ok = unsafe {
           CryptProtectData(
               &raw const in_blob,
               std::ptr::null(),
               &raw const entropy_blob,
               std::ptr::null(),
               std::ptr::null(),
               CRYPTPROTECT_UI_FORBIDDEN,
               &raw mut out_blob,
           )
       };
   ```
4. **Violated or unverified precondition**: The unsafe-ffi prompt §2 requires each unsafe block to be judged against its own `// SAFETY:` comment explaining nullness, alignment, aliasing, mutability, initialization, lifetime and thread affinity. None exists, so none of those preconditions is recorded at any of the roughly 40 sites. Adjacent prose comments (e.g. `src/platform/macos.rs:33-35`, `:583-585`, `:635-637`) document *why* the call exists but not *why the unsafe contract holds*.
5. **Impact**: [Inferred] Loss of auditability and high regression risk on OS-API upgrades; no UB is claimed here because no specific violated precondition was demonstrated.
6. **Recommendation**: Add a `// SAFETY:` comment immediately above every `unsafe fn`/`unsafe` block, starting with the ones that dereference raw pointers or cast buffer layouts (UF-3, UF-4, UF-5). Make it a review checklist item for `src/platform/`.
7. **Verification**: Re-run this grep to require zero undocumented blocks; Miri only for OS-independent helpers; target-specific clippy per OS (parent-owned).

### Medium

**UF-2 — Manual nul-terminated byte strings on the macOS concealed-clipboard marker path**
1. **Boundary**: `src/platform/macos.rs:421-427`, macOS.
2. **Severity**: Medium
3. **Evidence**:
   ```rust
           let nil_obj: *mut Object = std::ptr::null_mut();
           for marker in [
               b"org.nspasteboard.ConcealedType\0".as_slice(),
               b"org.nspasteboard.TransientType\0".as_slice(),
           ] {
               let type_str: *mut Object =
                   msg_send![class!(NSString), stringWithUTF8String: marker.as_ptr()];
   ```
   The same file already uses the safe literal form elsewhere: `src/platform/macos.rs:154` `stringWithUTF8String: c"NSApplicationDidBecomeActiveNotification".as_ptr()`, `:169` and `:239`.
4. **Violated or unverified precondition**: `docs/MULTIPLATFORM_LESSONS.md:58` and `AGENTS.md:39` require avoiding "manual nul-terminated byte strings (prefer `c\"...\"`)" in target-gated code. `marker.as_ptr()` is passed to an API that reads until NUL; the terminator is manual and unenforced.
5. **Impact**: [Inferred] A future edit that drops the `\0` produces an out-of-bounds read in `stringWithUTF8String:`.
6. **Recommendation**: Replace with `c"org.nspasteboard.ConcealedType".as_ptr()` and `c"org.nspasteboard.TransientType".as_ptr()`.
7. **Verification**: macOS-target clippy; smoke test that secrets still receive concealed markers.

**UF-3 — DPAPI input blobs cast immutable pointers to mutable**
1. **Boundary**: `src/platform/windows.rs:677-684` (`dpapi_protect`) and `src/platform/windows.rs:723-730` (`dpapi_unprotect`), Windows.
2. **Severity**: Medium
3. **Evidence**:
   ```rust
       let in_blob = CRYPT_INTEGER_BLOB {
           cbData: in_len,
           pbData: plaintext.as_ptr().cast_mut(),
       };
       let entropy_blob = CRYPT_INTEGER_BLOB {
           cbData: entropy_len,
           pbData: DPAPI_ENTROPY.as_ptr().cast_mut(),
       };
   ```
   The same pattern is used for `ciphertext` and `DPAPI_ENTROPY` at `:723-730`. `DPAPI_ENTROPY` is declared `const DPAPI_ENTROPY: &[u8] = b"CipherClipboard-DPAPI-v1";` at `:669`.
4. **Violated or unverified precondition**: `pbData` is `*mut u8`, so casting `&[u8]` and a `&'static [u8]` to `*mut u8` removes compiler enforcement that the callee treats `pDataIn`/`pEntropy` as input-only. The input-only contract is not documented at either call site (see UF-1).
5. **Impact**: [Inferred] Undefined behavior if the OS API writes through those pointers; no observation of such a write was made.
6. **Recommendation**: Add a `// SAFETY:` note stating that `CryptProtectData`/`CryptUnprotectData` do not modify `pDataIn` or `pEntropy`, or copy both inputs into owned mutable `Vec<u8>` buffers before building the blobs.
7. **Verification**: Windows smoke test for key-file protect/unprotect round trip; target-specific clippy (`borrow_as_ptr`, unused mut).

**UF-4 — Raw `LPARAM` reconstructed into `&mut SearchCtx` without an invariant note**
1. **Boundary**: `src/platform/windows.rs:298-300` and `:325-330`, Windows.
2. **Severity**: Medium
3. **Evidence**:
   ```rust
       unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
           let ctx = &mut *(lparam as *mut SearchCtx);
   ```
   ```rust
       let mut ctx = SearchCtx {
           expected_pid,
           found: std::ptr::null_mut(),
       };
       unsafe {
           EnumWindows(Some(enum_proc), std::ptr::addr_of_mut!(ctx) as LPARAM);
   ```
4. **Violated or unverified precondition**: The cast assumes the pointer is non-null, correctly aligned, points to an initialized `SearchCtx`, and is uniquely borrowed for the duration of the synchronous `EnumWindows` callback. The code satisfies those conditions (stack-local `ctx`, `addr_of_mut!`, synchronous enumeration), but they are not documented; `enum_proc` is an `extern "system"` callback that could in principle be invoked from another thread.
5. **Impact**: [Inferred] Aliasing/data-race UB if the callback were ever stored or invoked concurrently; no current concurrent invocation was found.
6. **Recommendation**: Add a `// SAFETY:` comment covering the synchronous-callback, stack-local, unique-borrow contract; keep `addr_of_mut!` rather than `&raw mut` only if the reason is recorded.
7. **Verification**: Windows smoke test for second-instance activation; clippy on Windows target.

**UF-5 — `TOKEN_MANDATORY_LABEL` cast performs no `ret_len` bounds validation**
1. **Boundary**: `src/platform/windows.rs:601-612`, Windows UIPI check.
2. **Severity**: Medium
3. **Evidence**:
   ```rust
               let mut buf = TokenIntegrityBuf([0u8; 256]);
               let mut ret_len = 0u32;
               let ok = GetTokenInformation(
                   token,
                   TokenIntegrityLevel,
                   buf.0.as_mut_ptr().cast::<std::ffi::c_void>(),
                   u32::try_from(buf.0.len()).unwrap_or(256),
                   &raw mut ret_len,
               );
               if ok != 0 {
                   let info = &*std::ptr::addr_of!(buf).cast::<TOKEN_MANDATORY_LABEL>();
   ```
   Alignment and initialization are explicitly handled at `src/platform/windows.rs:586-587` and `:601`:
   ```rust
       #[repr(C, align(8))]
       struct TokenIntegrityBuf([u8; 256]);
   ```
   `ret_len` is populated but never read after the call.
4. **Violated or unverified precondition**: The cast assumes at least `size_of::<TOKEN_MANDATORY_LABEL>()` bytes were written with the documented inline-SID layout. The zero-initialized, `align(8)` buffer satisfies initialization/alignment, but the written-length/layout assumption is neither checked nor documented. The null check at `:612` (`if !info.Label.Sid.is_null()`) mitigates the most obvious bad value.
5. **Impact**: [Inferred] Reading an invalid SID pointer or sub-authority count if the OS returns a short buffer; the fail-closed comparison at `:638` would then use an unvalidated integrity level.
6. **Recommendation**: Before the cast, require `ret_len >= size_of::<TOKEN_MANDATORY_LABEL>()` and `ret_len <= buf.0.len()`; return `None` otherwise so `target_is_elevated()` fails closed.
7. **Verification**: Windows CI UIPI smoke test against a medium- and high-integrity target.

### Low

**UF-6 — Unchecked `u32 as i32` cast on the macOS PID capture path**
1. **Boundary**: `src/platform/macos.rs:270-274`, macOS.
2. **Severity**: Low
3. **Evidence**:
   ```rust
           let pid: i32 = msg_send![frontmost, processIdentifier];
           let our_pid = std::process::id() as i32;
   ```
4. **Violated or unverified precondition**: `std::process::id()` returns `u32`; the `as i32` cast can wrap/sign-change in principle. macOS PIDs are bounded well below `i32::MAX`, so the invariant is practically satisfied but undocumented. The unsafe-ffi prompt §3 prefers checked conversions at FFI boundaries; contrast `src/platform/windows.rs:459` `(seq != 0).then(|| u64::from(seq))` and `:672` `u32::try_from(...)`.
5. **Impact**: [Inferred] Sign change for a PID above `i32::MAX`; not realistic on macOS.
6. **Recommendation**: `let our_pid = i32::try_from(std::process::id()).unwrap_or(0);` and record the assumption in a comment.
7. **Verification**: Source review only; no runtime reproduction claimed.

## Quick Wins
1. **UF-2**: Replace the two `b"...\0"` literals at `src/platform/macos.rs:423-424` with `c"..."` literals (~0.5 h).
2. **UF-6**: Use `i32::try_from` at `src/platform/macos.rs:271` (~0.25 h).
3. **UF-3**: Add the input-only `// SAFETY:` note to both DPAPI call sites (~0.5 h) — immediate documentation value before the fuller UF-1 sweep.
4. **UF-5**: Add the two `ret_len` bounds checks at `src/platform/windows.rs:610` (~1 h), preserving the fail-closed behavior at `:638`.

## Recommended Actions
1. **UF-1 (foundation)**: Do a single pass adding `// SAFETY:` comments to all unsafe blocks, prioritizing blocks that dereference raw pointers (`UF-3`, `UF-4`, `UF-5`). Enforce with the grep check used above so future additions must carry a justification.
2. **UF-5 and UF-3 next**: Convert the documented-but-unchecked preconditions into code checks or explicit invariants; these are the two sites where a wrong assumption changes security behavior (UIPI fail-closed and key-file protection).
3. **UF-2 and UF-4**: Remove manual nul terminators and document the callback aliasing contract in the same platform-cleanup PR.
4. **UF-6 last**: Fold the checked cast into the next macOS touch-up.
5. Keep the existing positive invariants intact: main-thread guards (`src/platform/macos.rs:44`, `:59`, `:72`, `:128`, `:188`), `autoreleasepool` around ObjC calls (`:75`, `:132`, `:193`, `:236`, `:261`, `:283`, `:307`, `:363`, `:374`, `:412`), `canBecomeMainWindow` filtering (`:101-104`), deferred reopen notification (`:110-115`), `thread_local! { Cell<*mut Object> }` observer (`:118-124`), explicit `#[link]` frameworks (`:208`, `:214`, `:226`), DPAPI plaintext zeroization before `LocalFree` (`src/platform/windows.rs:752-757`).

## Coverage
- **1. Containment** — No findings. `Cargo.toml:88` `unsafe_code = "deny"`; `#![allow(unsafe_code)]` appears only at `src/platform/macos.rs:1` and `src/platform/windows.rs:1`; `src/platform/mod.rs` line 1 is `#![allow(clippy::missing_const_for_fn)]`, a clippy lint allow, not an unsafe allow. `src/platform/mod.rs:194-199` uses a plain `#[derive(Clone, Copy)] struct PasteRequest` with no raw pointer or OS handle exposed to application code.
- **2. Unsafe Contract Quality** — Finding UF-1 (systemic absence of `// SAFETY:` comments), with UF-3/UF-4/UF-5 as the highest-value specific contracts.
- **3. Raw Pointers and Layout** — Findings UF-3, UF-4, UF-5. Positive evidence: `&raw mut` / `std::ptr::addr_of_mut!` are used throughout Windows code (`src/platform/windows.rs:82`, `:113`, `:141`, `:169`, `:192`, `:202`, `:234`, `:287`, `:301`, `:330`, `:598`, `:608`, `:655`, `:692-698`, `:738-744`) and `cast_unsigned()` is used at `:634` and `:651`; macOS uses `(count as i64).cast_unsigned()` at `src/platform/macos.rs:369`.
- **4. Mac AppKit / Objective-C** — Findings UF-2 and UF-6. All other documented invariants verified in source: main-thread guards, autoreleasepool, `canBecomeMainWindow`, non-reentrant notification handling, `thread_local!` observer, and `std::process::exit(0)` after cleanup at `src/app/mod.rs:1465`.
- **5. Windows FFI** — Findings UF-3, UF-4, UF-5. Positive evidence: `AttachThreadInput` attach guarded before detach (`src/platform/windows.rs:172-181`, `:246-255`); clipboard formats cached with `AtomicU32` (`:463-465`, `:491-493`, `:422-435`); UIPI fail-closed at `:635-639`; DPAPI status checked at `:701` and `:747`; `CRYPTPROTECT_UI_FORBIDDEN` at `:697`, `:743`; `icacls.exe` resolution gated with `#[cfg(any(target_os = "windows", test))]` at `src/crypto.rs:370`. No local `extern "C"`/`#[link]` declarations exist in `windows.rs`; Win32 symbols come from `windows-sys` (imports at `src/platform/windows.rs:9-39`) and the only `extern "system"` items are the two local enumeration callbacks (`:111`, `:298`), so no local library link declaration is required.
- **6. Linux X11 (safe backend)** — No findings in this category. `src/platform/linux.rs:1-10` imports only safe `x11rb` APIs; `X11Session` is created with pipelined atom interning (`:27-57`) and `X11_RECONNECT_BACKOFF` of 5 seconds is declared at `:14`.
- **7. Threading and Reentrancy** — No findings in this category from the FFI files read. Reopen callback only sets an atomic and requests repaint (`src/platform/macos.rs:110-115`); `REPAINT_CTX` lock is held only for a clone/repaint; Win32 enumeration callbacks mutate only stack-local or `Mutex`-protected state.
- **8. Tooling** — No tool results claimed. Recommended verifications are listed per finding; Miri is applicable only to OS-independent helpers and target-specific clippy must be run per OS by the parent's static-analysis phase.
