# CipherClipboard 📋🔒

A fast, encrypted cross-platform clipboard manager written in Rust.

CipherClipboard runs quietly in the background, instantly captures copied text and images, and stores everything in an AES-256 encrypted database secured primarily by your operating system's native keyring (with a restricted local-key fallback when the keyring is unavailable).

### Key Features
* **Full-Database Encryption at Rest:** Zero-knowledge 256-bit AES encryption powered by **SQLCipher** (`rusqlite` bundled build).
* **Native Keyring Security (with fallback):** Automatic key generation and storage in macOS Keychain, Windows Credential Manager, or Linux Secret Service via [`keyring`](https://github.com/hwchen/keyring-rs), with a restricted local `.encryption_key` fallback if the keyring is unavailable.
* **Smart Secret Detection & Masking:** Automatically detects and masks API tokens, private keys, JWTs, database URLs, and high-entropy passwords with toggleable visibility.
* **Transient & Password Manager Protection:** Automatically respects conceal flags from password managers (1Password, Bitwarden, KeePass, Apple Keychain).
* **Cross-Platform Immediate-Mode GUI:** Responsive interface built with [`egui`](https://github.com/emilk/egui) / [`eframe`](https://github.com/emilk/egui) with Direct3D 12 / WGPU acceleration on Windows and high WCAG contrast themes (Dark, Light, System).
* **Developer Utilities:** In-line syntax highlighting for 11 languages, interactive CSS/Hex/RGB/HSL color palette extraction, and built-in cryptographically secure password & passphrase generator.
* **Global Hotkey & Fast Recall:** Configurable global summon shortcut (`Ctrl+Shift+V` / `Cmd+Shift+V`) with single-instance window restoration and optional auto-paste.
* **Lossless PNG Image Storage:** Efficient PNG encoding for clipboard images via [`image`](https://crates.io/crates/image).

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
│   ├── tray.rs           # Windows system tray icon & context menu lifecycle
│   ├── secret.rs         # Heuristic detection & masking for credentials and API keys
│   ├── code.rs           # Multi-language syntax detection & highlighting sections
│   ├── color.rs          # Hex/RGB/HSL color parsing & swatch previews
│   ├── password.rs       # Cryptographically secure password and passphrase generator
│   ├── platform/         # OS-level window focus capture, restoration & auto-paste
│   │   ├── mod.rs
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
* Install **OpenSSL** (required by the bundled SQLCipher build, which links against `libcrypto`):
  ```powershell
  choco install openssl
  ```
  Then point the build at the Chocolatey install (CI uses `scripts/windows/setup-openssl-ci.ps1` to derive the same values):
  ```powershell
  $env:OPENSSL_DIR = "C:\Program Files\OpenSSL-Win64"
  $env:OPENSSL_LIB_DIR = "$env:OPENSSL_DIR\lib\VC\x64\MD"
  $env:OPENSSL_INCLUDE_DIR = "$env:OPENSSL_DIR\include"
  cargo build
  ```
  Use the `MD` import library — Rust's MSVC target links the dynamic CRT. Do not flatten the `MD`/`MT` lib directories into one. (Alternatively, use vcpkg and set these variables to its installed directories.)

  `bundled-sqlcipher` links `libcrypto` **dynamically** on Windows, so ship the matching `libcrypto-3-x64.dll` next to the executable (the release workflow pins and stages it). `OPENSSL_STATIC=1` is an `openssl-sys` setting and has **no effect** with this feature set — do not remove the DLL line from `scripts/windows/setup.iss`, or the exe will fail at startup with a missing-DLL error.

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
* **Windows `Alt+Space` Hotkey Preset:** Selecting the optional `Alt+Space` global shortcut on Windows overrides the standard system-menu accelerator (`Alt+Space`) in other applications. Prefer the default `Ctrl+Shift+V` unless you explicitly want `Alt+Space`.

---

## License

This project is licensed under the [MIT License](LICENSE).
