# CipherClipboard 📋🔒

A fast, encrypted cross-platform clipboard manager written in Rust.

> [!NOTE]
> **Platform testing status:** Currently, only the **macOS** and **Windows** builds have been reasonably tested in real-world use. The **Linux** build compiles and passes CI checks, but has not yet been tested on a live Linux desktop.

CipherClipboard runs quietly in the background, instantly captures copied text and images, and stores everything in an AES-256 encrypted database secured primarily by your operating system's native keyring (with a restricted local-key fallback when the keyring is unavailable).

### Key Features
* **Full-Database Encryption at Rest:** Zero-knowledge 256-bit AES encryption powered by **SQLCipher** (`rusqlite` bundled build).
* **Native Keyring Security (with fallback):** Automatic key generation and storage in macOS Keychain, Windows Credential Manager, or Linux Secret Service via [`keyring`](https://github.com/hwchen/keyring-rs), with a restricted local `.encryption_key` fallback if the keyring is unavailable.
* **Smart Secret Detection & Masking:** Automatically detects and masks API tokens, private keys, JWTs, database URLs, and high-entropy passwords with toggleable visibility.
* **Encrypted Annotations, Pinning & Direct Secret Entry:** Add labels/notes to any entry, pin important snippets/links/images/secrets to keep them out of pruning and bulk clear (independently from Secret masking), and save secrets directly without touching the OS clipboard.
* **Transient & Password Manager Protection:** Automatically respects conceal flags from password managers (1Password, Bitwarden, KeePass, Apple Keychain).
* **Cross-Platform Immediate-Mode GUI:** Responsive interface built with [`egui`](https://github.com/emilk/egui) / [`eframe`](https://github.com/emilk/egui) with Direct3D 12 / WGPU acceleration on Windows and high WCAG contrast themes (Dark, Light, System).
* **Developer Utilities:** In-line syntax highlighting for 11 languages, interactive CSS/Hex/RGB/HSL color palette extraction, and built-in cryptographically secure password & passphrase generator.
* **Text Transformations & AI Prompt Prep:** One-click card menu transformations (with live hover previews) for cleanup, case/line operations, JSON/Base64/URL tools, and **AI & Prompt** presets that sanitize and format code, logs, or spreadsheets before pasting into Microsoft 365 Copilot or AI coding agents.
* **Global Shortcuts & Fast Recall:** Unified keycap-pill shortcut recorders for **Toggle Window** (`Cmd+Shift+V` / `Ctrl+Shift+V`) with single-instance restoration and optional auto-paste, plus **Paste Next (Paste Stack)** (`Cmd+Option+V` / `Ctrl+Alt+V`) to paste multiple copied entries sequentially without opening the window.
* **Paste Stack (LIFO / FIFO):** Paste copied text, secrets, or images one by one in **LIFO ↓** (newest-first, default) or **FIFO ↑** order by placing the directional `NEXT` marker on any older entry (hover button, right-click menu, or `Cmd/Ctrl+Shift+M`). Any new copy automatically resets back to LIFO.
* **Lossless PNG Image Storage:** Efficient PNG encoding for clipboard images via [`image`](https://crates.io/crates/image).

---

## Text Transformations & AI Prompt Prep

Open the `⋯` menu on any text clipboard card to copy a transformed version without modifying the stored original. Hovering over any transformation shows a description and a live preview of the output (hidden for masked secrets).

* **AI & Prompt** *(designed for Copilot 365 & AI coding agents)*:
  * **Prepare Code for AI:** Dedents common indentation, replaces local home paths (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`) with `<HOME>`, redacts PEM keys, tokens, URL credentials, and sensitive assignments, and wraps the snippet in an auto-tagged Markdown code fence.
  * **Prepare Log for AI:** Strips ANSI escape codes, replaces local home paths, redacts secrets and non-loopback IPv4 addresses, collapses repeated consecutive lines (`... (repeated Nx)`), and wraps the result in `<log>...</log>`.
  * **Prepare Table for AI:** Converts copied TSV (Excel/Sheets/SQL), CSV, or semicolon-separated rows into a clean Markdown pipe table.
  * **Add Line Numbers:** Prefixes each line with right-aligned 1-based line numbers (`1: ...`) so you can reference exact lines in an AI chat.
* **Cleanup:** Strip formatting (HTML, Markdown, ANSI, zero-width chars), unwrap hard line breaks, collapse whitespace.
* **Case & Lines:** `UPPERCASE`, `lowercase`, `Title Case`, `camelCase`, `snake_case`, `kebab-case`, sort, deduplicate, remove empty lines, join with commas.
* **Developer & URL:** Prettify/minify JSON, Base64 encode/decode, OS-aware **Quote Path for Shell** (POSIX single quotes on macOS/Linux, double quotes on Windows), and strip URL tracking parameters (`utm_*`, `fbclid`, `gclid`, etc.).

---

## Paste Stack ("Paste Next")

Press the global **Paste Next** shortcut (`Cmd+Option+V` on macOS, `Ctrl+Alt+V` on Windows/Linux — customizable or clearable in **Settings → Global Shortcuts**) to paste history entries sequentially into your focused app without showing the CipherClipboard window:

* **LIFO ↓ (default):** Starts on your newest entry (`↓ NEXT`) and steps downward to older entries.
* **FIFO ↑:** Place the marker on any older entry (1-click hover arrow on a card, right-click → **Paste next from here**, or `Cmd/Ctrl+Shift+M` on the selected card) to paste upward (`↑ NEXT`) back to the newest entry, then automatically return to **LIFO ↓**.
* **Automatic reset:** Any new captured copy resets to **LIFO ↓** on that new entry; Paste Next's own writes and ignored concealed copies never reset the stack.

---

## Architecture Overview

```
                      ┌──────────────────────────────────────┐
                      │   OS Keyring or local key fallback   │
                      │   (Keychain / Credential / Secret)   │
                      └──────────────────┬───────────────────┘
                                         │ 256-bit Key
                                         ▼
┌──────────────────────┐      ┌──────────────────────────────┐
│  Background Monitor  │      │     SQLCipher Database       │
│      (arboard)       ├─────►│  (rusqlite, 256-bit AES)     │
│ Polls text & images  │      │  Table: clipboard_history    │
└──────────┬───────────┘      └──────────────┬───────────────┘
           │                                 │
           │ ctx.request_repaint()           │ Queries / Filter
           ▼                                 ▼
┌────────────────────────────────────────────────────────────┐
│                  eframe / egui GUI Window                  │
│  - Instant search & history filter (text & images)         │
│  - Interactive thumbnail preview & one-click copy back     │
│  - Single item delete & Clear All                          │
└────────────────────────────────────────────────────────────┘
```

---

## Project Structure

```
cipherclipboard/
├── Cargo.toml            # Project manifest & dependencies
├── deny.toml             # Supply-chain security & license policy
├── docs/                 # Bug analyses, platform notes and dated review reports
│   ├── TESTING.md                # Test inventory and skipped-tooling notes
│   ├── architecture/             # Architecture and module relationship documentation
│   ├── windows/
│   │   ├── WINDOWS_REVIEW.md     # Windows readiness review notes
│   │   └── WINDOWS_TESTING.md    # Windows manual testing checklist
│   └── reviews/                  # Dated comprehensive review reports
├── scripts/              # Local CI pre-flight guardrails & release packaging
├── src/
│   ├── main.rs           # Entry point, panic hook, single-instance lock & eframe launcher
│   ├── app/              # egui UI layout, history view, password generator & settings
│   │   ├── mod.rs
│   │   ├── history_view.rs
│   │   ├── generator_view.rs
│   │   └── settings_view.rs
│   ├── db.rs             # rusqlite + SQLCipher encrypted database wrapper & migrations
│   ├── crypto.rs         # OS Keyring integration & cryptographic key generation
│   ├── hotkey.rs         # Global shortcut registration & listener thread
│   ├── icons.rs          # Embedded application & system tray icons
│   ├── logging.rs        # In-memory diagnostics ring-buffer & file logging
│   ├── tray.rs           # Windows system tray icon & context menu lifecycle
│   ├── secret.rs         # Heuristic detection & masking for credentials and API keys
│   ├── transform.rs      # Deterministic text transformations & AI prompt preparation presets
│   ├── code.rs           # Multi-language syntax detection & highlighting sections
│   ├── color.rs          # Hex/RGB/HSL color parsing & swatch previews
│   ├── password.rs       # Cryptographically secure password and passphrase generator
│   ├── platform/         # OS-level window focus capture, restoration & auto-paste
│   │   ├── mod.rs
│   │   ├── fs_security.rs# Filesystem ACL hardening, permissions & DPAPI fallback
│   │   ├── macos.rs
│   │   ├── windows.rs
│   │   └── linux.rs
│   ├── media.rs          # Image encoding/decoding (RGBA <-> PNG)
│   ├── monitor.rs        # Background clipboard polling & change detection
│   ├── settings.rs       # Persistent user preferences & keybinding configuration
│   ├── theme.rs          # Dark/Light palettes & WCAG-compliant styling
│   └── instance.rs       # Cross-platform single-instance advisory lock
└── README.md
```

---

## Prerequisites & Building

### Rust toolchain

The minimum supported Rust version (MSRV) is **1.95**, declared as `rust-version` in `Cargo.toml`. Build and test with a stable toolchain at or above that version; CI uses current stable and resolves dependencies from the committed `Cargo.lock` with `--locked`. Newer stable toolchains are supported, so there is no `rust-toolchain.toml` pin — use `rustup override set stable` locally if you want to match CI explicitly.

### 1. macOS (Primary Environment)
Ensure you have Xcode Command Line Tools installed (needed to compile the bundled SQLCipher C code):
```bash
xcode-select --install
```

### 2. Linux (Debian / Ubuntu / Fedora)
On Linux, `arboard` and `keyring` require X11/Wayland and DBus libraries, and the bundled `sqlcipher` build requires OpenSSL development headers:
```bash
# Debian / Ubuntu:
sudo apt-get install libx11-dev libxcb1-dev libdbus-1-dev libssl-dev pkg-config

# Fedora:
sudo dnf install libX11-devel libxcb-devel dbus-devel openssl-devel pkgconf-pkg-config
```

### 3. Windows
* Install the **C++ Build Tools** via the Visual Studio Installer (needed for MSVC C compiler for SQLCipher).
* Install **Strawberry Perl** (`winget install StrawberryPerl.StrawberryPerl`), which Windows uses to build the vendored OpenSSL (`bundled-sqlcipher-vendored-openssl`) statically into `CipherClipboard.exe` — no external OpenSSL installation or runtime `libcrypto-*.dll` is needed.
  *(If you also have Git Bash or MSYS2 Perl on `PATH`, set `$env:OPENSSL_SRC_PERL = "C:\Strawberry\perl\bin\perl.exe"` so OpenSSL Configure uses native Windows Perl.)*

---

## Running the Application

In your terminal:

```bash
cargo run
```

### Windows release downloads

The installer runs the bundled **Visual C++ redistributable (x64)** when the runtime is missing (a no-op when it is already installed). The portable ZIP contains `vc_redist.x64.exe` too — run it once before launching `CipherClipboard.exe` on a fresh Windows install. The executable is MSVC-linked (SQLCipher + OpenSSL), so it cannot start without the Universal CRT.

**Uninstalling on Windows:** Uninstalling removes the application binaries and shortcuts only; your encrypted database, settings, and log (`%LOCALAPPDATA%\cipherclipboard\`) as well as the encryption key in **Windows Credential Manager** (under `cipherclipboard`) are preserved so upgrades and reinstalls never wipe your history. To perform a complete removal, delete `%LOCALAPPDATA%\cipherclipboard\` and remove the `cipherclipboard` generic credential in Windows Credential Manager.

On first launch:
1. It automatically generates a secure 256-bit encryption key and stores it in your **OS Keyring** (under service `cipherclipboard`), falling back to a restricted local `.encryption_key` file if the keyring is unavailable.
2. Creates an encrypted database `clipboard_history.db` in your app data directory: `~/Library/Application Support/cipherclipboard/` (macOS), `%LOCALAPPDATA%\cipherclipboard\` (Windows), or `~/.local/share/cipherclipboard/` (Linux).
3. Opens the UI and starts monitoring copied text and images in the background.

#### Key loss and recovery

The key normally lives in your OS Keyring under service `cipherclipboard` (macOS Keychain, Windows Credential Manager, or Linux Secret Service). If the keyring is unavailable at first launch, CipherClipboard creates a restricted local `.encryption_key` fallback in the app data directory and shows a degraded-state notice.

If an encrypted database already exists but its key cannot be found, startup **fails closed**: CipherClipboard will never generate a replacement key, because that would make your existing history undecryptable. First try unlocking the OS keyring (for example, unlock the macOS login keychain) and restarting. If you previously used the fallback, restore `.encryption_key` from a backup. There is no other way to recover the encrypted history.

Deleting the app data directory — `~/Library/Application Support/cipherclipboard/` (macOS), `%LOCALAPPDATA%\cipherclipboard\` (Windows), or `~/.local/share/cipherclipboard/` (Linux) — removes the database and fallback key and **permanently destroys your history**. Do this only when you intentionally want to start fresh.

#### Backup & Restore

Use **Settings → Export Encrypted Backup…** to create a consistent encrypted copy of `clipboard_history.db`. The backup stays encrypted with the same SQLCipher key, so restoring it on another machine requires the original encryption key.

**Restore:** use **Settings → Restore Backup…** to select an encrypted backup. CipherClipboard validates the backup with the current key, stages it, and applies it on next restart (creating a pre-restore backup of the current DB first). Manual restore is also possible: quit CipherClipboard, replace `clipboard_history.db` in the app data directory, ensure the matching encryption key is available, then restart.

**Locate/copy the encryption key (sensitive — anyone with it can decrypt your DB):**

- **Service:** `cipherclipboard`
- **Account:** `db_encryption_key`

| Platform | Where to find it | CLI (prints secret; use carefully) |
|---|---|---|
| macOS | Keychain Access → search `cipherclipboard` / `db_encryption_key` | `security find-generic-password -s cipherclipboard -a db_encryption_key -w` |
| Windows | Credential Manager → Windows Credentials → `cipherclipboard` / `db_encryption_key` | No built-in safe export; view in Credential Manager or recreate the entry on the target machine |
| Linux | Seahorse/GNOME Passwords and Keys, or your Secret Service backend | `secret-tool lookup service cipherclipboard username db_encryption_key` |

If CipherClipboard used the local-key fallback, back up the restricted `.encryption_key` file from the app data directory together with the DB. Treat both the backup and key as secrets.

---

## Development & CI Guardrails

To ensure all contributions satisfy continuous integration standards (formatting, zero Clippy warnings, license compliance via `cargo-deny`, and passing test suite):

1. **Install the Git Pre-Push Hook (Recommended):**
   ```bash
   ./scripts/install-hooks.sh
   ```
   This ensures that any `git push` automatically runs pre-flight verifications before sending commits to remote.

2. **Run Local Pre-Flight Checks Manually:**
   ```bash
   ./scripts/preflight.sh
   ```

See [`docs/TESTING.md`](docs/TESTING.md) for the current test inventory and the explicitly skipped property-based, fuzz, and Miri tooling.

---

## Known Limitations

* **Monochrome Emoji & Symbol Rendering:** `egui` (`0.37`) rasterizes font glyphs via `ab_glyph` using monochrome vector outlines and does not support color emoji tables (`sbix`, `COLR`/`CPAL`, or `CBDT`). As a result, emojis and symbols in copied text render in the active theme's monochrome text color rather than full color. Invisible Unicode variation selectors (`U+FE0E`, `U+FE0F`) and zero-width joiners are stripped from UI previews to avoid missing-glyph (`□`) boxes, while the original text is preserved intact when copied back to the clipboard.
* **Windows UIPI & Elevated Target Windows:** Windows User Interface Privilege Isolation (UIPI) blocks standard-privilege applications from injecting synthetic keystrokes (`Ctrl+V`) into elevated (Administrator) windows. CipherClipboard detects higher-integrity target windows, copies the item to the clipboard, and skips auto-paste—press `Ctrl+V` manually in the elevated window.
* **Windows `Alt+Space` Global Shortcut:** Binding `Alt+Space` as a global shortcut on Windows overrides the standard system-menu accelerator in other applications. Prefer the default `Ctrl+Shift+V` / `Ctrl+Alt+V` unless you explicitly want `Alt+Space`.
* **Linux Outgoing Concealed Markers:** On Linux (X11/Wayland), incoming concealed markers (`x-kde-passwordManagerHint` in `TARGETS`) are detected from password managers, but outgoing concealed markers cannot be attached by the underlying clipboard provider (`ConcealedMarkerSupport::DetectOnly`). Copied secrets are masked and scheduled for auto-clear, but third-party clipboard monitors without heuristic detection may observe the copy.

---

## License

This project is licensed under the [MIT License](LICENSE).
