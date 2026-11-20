# Windows Testing Checklist — CipherClipboard

Manual smoke-test checklist for Windows. Run before each release to cover GUI,
shell, and hardware behaviors that headless CI cannot exercise.

## Build & install

- [ ] Fresh machine with only **Visual Studio C++ Build Tools** + **OpenSSL** (`choco install openssl`, `OPENSSL_DIR` set) builds successfully (`cargo build --release`).
- [ ] Release binary runs without a console (uses `windows_subsystem = "windows"`).
- [ ] `CipherClipboard.exe` file properties show the CipherClipboard icon, product name and version metadata (an empty/generic resource means the resource compiler failed).
- [ ] **Clean Windows image with no VC++ redistributable:** installer installs the bundled `vc_redist.x64.exe` and the app starts.
- [ ] **Clean Windows image:** portable ZIP's `vc_redist.x64.exe` run once, then `CipherClipboard.exe` starts.
- [ ] Installer (`setup.iss`) copies the pinned `libcrypto-3-x64.dll` next to the exe and the app starts.
- [ ] First launch creates `%LOCALAPPDATA%\cipherclipboard\` and registers the key in Windows Credential Manager.

## Hotkeys

- [ ] `Ctrl+Shift+V` (Universal) registers and triggers on both US QWERTY and French AZERTY.
- [ ] `Ctrl+ù` (AZERTY) registers and triggers on French/Belgian AZERTY.
- [ ] `Ctrl+ù` does **not** misregister as `²` on AZERTY.
- [ ] On US QWERTY, the quote key records as `Quote`, not `ù`.
- [ ] Custom hotkey recorder captures modifier + key and round-trips (`parse` → `format`).
- [ ] Changing hotkey at runtime updates registration (old hotkey stops working, new one works).
- [ ] If registration fails, the UI shows the OS error string.
- [ ] `Alt+Space` preset is documented as shadowing the system menu accelerator.

## Window / tray / focus

- [ ] Hide window via hotkey or X button; restore via tray icon left-click and double-click.
- [ ] Tray context menu: Show, Settings, Quit all work.
- [ ] If tray init fails, window minimizes to taskbar instead of becoming unreachable.
- [ ] Global hotkey still summons the window after it is hidden.
- [ ] `AttachThreadInput`/`SetForegroundWindow` path restores focus reliably.
- [ ] Hide-on-blur behavior matches expectation (default `false` on Windows).

## Clipboard & monitoring

- [ ] Copy text, image, and file paths; all are captured.
- [ ] Windows paths (`C:\…`, `C:/…`, `\\server\share`, `src\lib\main.rs`) are **not** flagged as secrets.
- [ ] Passwords containing a single backslash are still detected as secrets.
- [ ] App started at session logon (clipboard briefly unavailable) still captures after retry.
- [ ] Copy while disconnected from an RDP session, then reconnect: capture resumes.
- [ ] Concealed/transient clipboard content is ignored when `ignore_concealed` is enabled.

## Auto-paste & focus

- [ ] Auto-paste (`Ctrl+V`) works into a normal (non-elevated) target app.
- [ ] Auto-paste into scan-code-sensitive targets (e.g. Windows Terminal) works — confirms `wScan` is populated.
- [ ] Auto-paste into an elevated (Administrator) target app is documented as blocked by UIPI.
- [ ] Focus returns to the previous window after paste.
- [ ] Switching input language in another app (without opening our Settings) makes `Ctrl+ù` register for the new layout automatically — confirms the layout probe reads the target thread.

## Crypto / storage

- [ ] Key file permissions are restricted (best-effort; verify on non-Entra machine).
- [ ] Database opens and history persists across restarts.
- [ ] Secret auto-clear timer fires after configured delay.
- [ ] No SQLite lock errors under an AV scanner.

## Build / CI guardrails

- [ ] `cargo build`, `cargo test`, `cargo clippy --all-targets` are green on `windows-latest`.
- [ ] `cargo deny check` passes (including OpenSSL license).
- [ ] No new `unsafe` without review.

## Notes

- Release builds use `windows_subsystem = "windows"`: `println!`/`eprintln!` output is discarded. Use UI toasts or a log file for diagnostics.
- Prefer testing on both US QWERTY and French/Belgian AZERTY layouts.