# Windows Compatibility Review — CipherClipboard

**Scope:** Full review of `src/` (10,405 lines), `build.rs`, CI/release workflows, and installer
script for defects that would only surface on Windows. The codebase was previously exercised on
macOS (primary) and lightly on Linux; the Windows CI test job is `continue-on-error: true`.

**Baseline:** 61/61 unit tests pass on macOS before and after these changes
(`cargo test`, `cargo clippy --all-targets` clean).

---

## Summary of Findings

| # | Severity | Area | Finding | Action |
|---|----------|------|---------|--------|
| 1 | 🔴 High | `secret.rs` | Windows paths classified as *Secrets* (masked + auto-cleared) | **Fixed** |
| 2 | 🟠 Medium | `crypto.rs` | Windows ACL hardening fails silently on domain/Entra-joined machines | **Fixed** |
| 3 | 🟠 Medium | `monitor.rs` | Clipboard capture permanently disabled if `Clipboard::new()` fails once | **Fixed** |
| 4 | 🟡 Medium | Docs / build | Local Windows build instructions miss the OpenSSL requirement | **Fixed (README)** |
| 5 | 🟡 Low | UX / docs | Windows limitations: no tray icon, `Alt+Space` shadowing, UIPI auto-paste | **Documented** |

---

## 🔴 1. Windows paths flagged as Secrets (`src/secret.rs`)

`is_high_entropy_secret()` excludes Unix paths (`/…`, `./…`, `~/…`) and Windows paths of the
exact form `C:\…`, but misses the forms that are common on Windows:

- **Drive-letter paths with forward slashes** — `C:/Users/bruno/...` (PowerShell, Git Bash, and
  many tools emit these).
- **UNC shares** — `\\fileserver\share\reports\q3.xlsx`.
- **Relative backslash paths** — `src\lib\main.rs`, `dir\sub1\file.txt`.

Concretely, `dir\sub1\file.txt` (17 chars, lower + digit + symbol classes, Shannon entropy
≈ 3.73) passes the `has_symbol && class_count >= 3 && entropy >= 3.5` rule and is stored as a
**Secret**: masked as bullets in the UI, and silently cleared from the clipboard 30 s after the
user pastes it. The same copy on macOS (`dir/sub1/file.txt`) is… also not excluded, but
forward-slash paths only get flagged when they are ≥ 24 chars; the backslash spelling trips the
rule far more often. Net effect on Windows: ordinary file paths appear as locked "Secret"
entries and paste-back auto-clears them.

**Fix:** new `looks_like_path()` helper covering drive letters with *either* separator, UNC
prefixes, and backslash-relative paths (requires a plausible `name.ext` last segment or ≥ 2
backslashes, so passwords containing a single backslash are still detected). Covered by new
tests.

*Trade-off, documented in code:* a password that happens to contain a backslash **and** look
like `name.ext` (or contain two backslashes) will no longer be auto-masked. Paths are far more
common in clipboard traffic than backslash-carrying passwords, and users can re-mark items as
Secret manually; the reverse (paths masked + auto-cleared) is a worse failure mode.

## 🟠 2. Silent ACL-hardening failure on domain/Entra machines (`src/crypto.rs`)

`set_restricted_permissions()` (Windows branch) grants ACLs via `icacls … /grant:r {USERNAME}:(F)`
using the bare `USERNAME` environment variable. This fails on:

- **Domain-joined machines** where a local account and a domain account share the name —
  ambiguous, may resolve to the wrong principal or fail with "no mapping between account names
  and SIDs".
- **Microsoft Entra (Azure AD) joined machines**, where bare SAM names frequently do not
  resolve at all.

The command result is discarded, so the key file silently keeps the broad inherited ACLs from
`%LOCALAPPDATA%` — the hardening exists only on paper for those users.

**Fix:** prefer the unambiguous `USERDOMAIN\USERNAME` form (falls back to bare `USERNAME`).
Still best-effort with `CREATE_NO_WINDOW` so no console flashes.

## 🟠 3. One failed `Clipboard::new()` disables capture until restart (`src/monitor.rs`)

The background monitor thread created its `arboard::Clipboard` once at startup and `return`ed
on error. On Windows the clipboard can be briefly unavailable — at session logon (auto-start),
while disconnected from an RDP session, or during session switches. If the app happened to
start at such a moment, it would run forever with **no clipboard capture** and no retry.

**Fix:** retry loop (2 s interval, aborts on shutdown signal) instead of permanent early-exit.
No behavior change on macOS/Linux in the happy path.

## 🟡 4. Windows build docs miss the OpenSSL requirement

`rusqlite` with `bundled-sqlcipher` compiles SQLCipher from source, which links against
OpenSSL (`libcrypto`). CI (`ci.yml`) and the release workflow install OpenSSL via Chocolatey and
set `OPENSSL_DIR`, but the README told Windows developers to install only the C++ Build Tools —
a local `cargo build` would fail with an `openssl-sys`/linker error.

**Fix:** README now documents `choco install openssl` + `OPENSSL_DIR` (and the vcpkg
alternative), matching what CI does.

## 🟡 5. Windows UX limitations (documented, not code-fixed in this pass)

- **No system tray icon.** Hide-on-blur and the X button hide the window; a hidden window has
  no taskbar button, so without the global hotkey the running app is unreachable. On macOS the
  Dock icon + reopen observer covers this. Proper fix = `tray-icon` + `muda` integration with
  Show/Quit menu — deferred (needs new dependencies and real Windows testing, risky to add
  blind from macOS).
- **`Alt+Space` preset** shadows the Windows system-menu accelerator globally for *all*
  applications while registered. The other presets are safe defaults; keeping the preset with a
  docs warning.
- **Auto-paste (simulated `Ctrl+V` via `SendInput`) is blocked by UIPI** when the target
  application runs elevated (as Administrator) and CipherClipboard does not. Windows has no
  per-app permission prompt for this (unlike macOS Accessibility); document that the target app
  must run at the same or lower integrity level.
- Release builds use `windows_subsystem = "windows"`: no console, `println!` output is
  discarded (by design; Rust std discards writes to missing stdio handles — no panic).

---

## Verified OK — checked, no bug (so nobody re-litigates these)

- **CRLF handling:** clipboard text is stored untrimmed (fidelity preserved), and every
  classifier trims first (`parse_color`, `detect_code_language`, `format_json`,
  `is_likely_secret`, `is_valid_url`, `is_shell_command`); `extract_colors` treats `\r` as a
  boundary character. egui 0.29 itself renders `\r` as zero-width
  (`epaint::text::font::invisible_char`). CRLF content from Windows sources displays and
  classifies correctly.
- **Keyboard shortcuts on Windows:** egui-winit maps the `command` modifier to Ctrl on
  non-macOS, so `Ctrl+1..9` copy slots, `Ctrl+Backspace` delete, and `Ctrl+Q` quit all work.
- **`simulate_paste()` INPUT struct** (`platform.rs`): `repr(C)` layout verified — 40 bytes on
  x64 and 28 on x86, matching the `MOUSEINPUT`-sized union with const assertions.
- **Single-instance lock** (`instance.rs`): `File::try_lock()` works on Windows; the `.pid`
  sidecar covers read-while-locked; stale PID only reported when the lock is actually held.
- **`println!` in GUI-subsystem release builds** cannot panic (std discards missing stdio).
- **SQLite WAL:** `busy_timeout(5 s)` absorbs transient locks from AV scanners; WAL on exotic
  network-redirected `%LOCALAPPDATA%` degrades gracefully.
- **Unicode paths:** `dirs::data_local_dir()` → `%LOCALAPPDATA%` and rusqlite handle non-ASCII
  user names.
- **Fonts:** `C:\Windows\Fonts\segoeui.ttf` probe always resolves on Windows.
- **Keyring:** keyring 4.2 → Windows Credential Manager; a 64-char key fits comfortably.
- **`build.rs` resource embedding:** release builds run natively on `windows-latest`, where the
  `#[cfg(windows)]` host gate is satisfied. (Only affects hypothetical cross-compilation.)
- **Window summon/focus:** `order_front_main_window()` implements the
  `AttachThreadInput`/`SetForegroundWindow` workaround required for hotkey-initiated focus
  changes on Windows; it is called from the UI thread and complements egui's `ViewportCommand`.

---

## Deferred / recommendations

1. **System tray icon** (`tray-icon` + `muda`): the single biggest functional gap on Windows —
   with no tray, closing/hiding the window orphans the process. Worth doing as a feature with
   access to a Windows machine.
2. **Windows CI gates:** consider removing `continue-on-error` for the `windows-latest` test
   job once the team has one Windows run green, so regressions actually block.
3. **Installer:** `setup.iss` copies `libcrypto-*.dll` next to the exe — keep this in sync if
   the build ever switches to static OpenSSL (`OPENSSL_STATIC=1`), which would remove the DLL
   dependency entirely.
4. **icacls on Entra-only machines:** even `USERDOMAIN\USERNAME` can fail there; the fully
   robust variant grants by SID (`*S-1-…`) which would require querying `whoami /user` or the
   `windows` crate. Current best-effort behavior is acceptable for a local-only key file.
