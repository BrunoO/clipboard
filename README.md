# CipherClipboard 📋🔒

A fast, encrypted cross-platform clipboard manager written in Rust.

CipherClipboard runs quietly in the background, instantly captures copied text and images, and stores everything in an AES-256 encrypted database secured by your operating system's native keyring.

### Key Features
* **Full-Database Encryption at Rest:** Zero-knowledge 256-bit AES encryption powered by **SQLCipher** (`rusqlite` bundled build).
* **Native Keyring Security:** Automatic key generation and storage in macOS Keychain, Windows Credential Manager, or Linux Secret Service via [`keyring`](https://github.com/hwchen/keyring-rs).
* **Smart Secret Detection & Masking:** Automatically detects and masks API tokens, private keys, JWTs, database URLs, and high-entropy passwords with toggleable visibility.
* **Transient & Password Manager Protection:** Automatically respects conceal flags from password managers (1Password, Bitwarden, KeePass, Apple Keychain).
* **Cross-Platform Immediate-Mode GUI:** Responsive interface built with [`egui`](https://github.com/emilk/egui) / [`eframe`](https://github.com/emilk/egui) with Direct3D 12 / WGPU acceleration on Windows and high WCAG contrast themes (Dark, Light, System).
* **Developer Utilities:** In-line syntax highlighting for 7+ languages, interactive CSS/Hex/RGB/HSL color palette extraction, and built-in cryptographically secure password & passphrase generator.
* **Global Hotkey & Fast Recall:** Configurable global summon shortcut (`Ctrl+Shift+V` / `Cmd+Shift+V`) with single-instance window restoration and optional auto-paste.
* **Lossless Image Compression:** Efficient PNG compression for clipboard images via [`image`](https://crates.io/crates/image).

---

## Architecture Overview

```
                      ┌──────────────────────────────────────┐
                      │          OS Keyring (keyring)        │
                      │   (macOS Keychain / Win Credential)  │
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
├── scripts/              # Local CI pre-flight guardrails & release packaging
├── src/
│   ├── main.rs           # Entry point, single-instance coordination & eframe launcher
│   ├── app.rs            # egui UI layout, search, filters & state management
│   ├── db.rs             # rusqlite + SQLCipher encrypted database wrapper & migrations
│   ├── crypto.rs         # OS Keyring integration & cryptographic key generation
│   ├── hotkey.rs         # Global shortcut registration & listener thread
│   ├── secret.rs         # Heuristic detection & masking for credentials and API keys
│   ├── code.rs           # Multi-language syntax detection & highlighting sections
│   ├── color.rs          # Hex/RGB/HSL color parsing & swatch previews
│   ├── password.rs       # Cryptographically secure password and passphrase generator
│   ├── platform.rs       # OS-level window focus capture, restoration & auto-paste
│   ├── media.rs          # Image encoding/decoding (RGBA <-> PNG)
│   ├── monitor.rs        # Background clipboard polling & change detection
│   ├── settings.rs       # Persistent user preferences & keybinding configuration
│   ├── theme.rs          # Dark/Light palettes & WCAG-compliant styling
│   └── instance.rs       # Cross-platform single-instance mutex & named pipe lock
└── README.md
```

---

## Prerequisites & Building

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

On first launch:
1. It automatically generates a secure 256-bit encryption key and registers it with your **OS Keyring** (under service `cipherclipboard`).
2. Creates an encrypted database `clipboard_history.db` in `~/Library/Application Support/cipherclipboard/` (or `%LOCALAPPDATA%\cipherclipboard\` on Windows).
3. Opens the UI and starts monitoring copied text in the background.

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

---

## Known Limitations

* **Monochrome Emoji & Symbol Rendering:** `egui` (`0.29`) rasterizes font glyphs via `ab_glyph` using monochrome vector outlines and does not support color emoji tables (`sbix`, `COLR`/`CPAL`, or `CBDT`). As a result, emojis and symbols in copied text render in the active theme's monochrome text color rather than full color. Invisible Unicode variation selectors (`U+FE0E`, `U+FE0F`) and zero-width joiners are stripped from UI previews to avoid missing-glyph (`□`) boxes, while the original text is preserved intact when copied back to the clipboard.

---

## License

This project is licensed under the [MIT License](LICENSE).
