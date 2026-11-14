# CipherClipboard 📋🔒

A lightweight, encrypted cross-platform clipboard manager written in Rust.

Designed as a clean, idiomatic project for learning Rust, featuring:
* **Immediate-mode GUI** using [`eframe`](https://github.com/emilk/egui) / [`egui`](https://github.com/emilk/egui).
* **Cross-platform clipboard access** for both text and images via [`arboard`](https://github.com/1Password/arboard).
* **Global OS hotkey summoning** (`Cmd+Shift+V` / `Ctrl+Shift+V`) via [`global-hotkey`](https://github.com/tauri-apps/global-hotkey).
* **Full-database encryption** at rest using **SQLCipher** (via [`rusqlite`](https://github.com/rusqlite/rusqlite) with `bundled-sqlcipher`).
* **Secure key storage** via OS Keyring ([`keyring`](https://github.com/hwchen/keyring-rs): macOS Keychain, Windows Credential Manager, Linux Secret Service).
* **Lossless PNG compression** for clipboard images via [`image`](https://crates.io/crates/image).

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
├── src/
│   ├── main.rs           # App setup, directory discovery, eframe launcher
│   ├── app.rs            # egui UI layout, search, item rendering
│   ├── db.rs             # rusqlite + SQLCipher encrypted database wrapper
│   ├── crypto.rs         # OS Keyring integration & key generation
│   ├── hotkey.rs         # Global OS shortcut listener (Cmd+Shift+V / Ctrl+Shift+V)
│   ├── media.rs          # Image encoding/decoding (RGBA <-> PNG)
│   └── monitor.rs        # Background thread monitoring OS clipboard
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
On Linux, `arboard` and `keyring` require X11/Wayland and DBus libraries:
```bash
# Debian / Ubuntu:
sudo apt-get install libx11-dev libxcb1-dev libdbus-1-dev
```

### 3. Windows
* Install the **C++ Build Tools** via the Visual Studio Installer (needed for MSVC C compiler for SQLCipher).

---

## Running the Application

In your terminal:

```bash
cargo run
```

On first launch:
1. It automatically generates a secure 256-bit encryption key and registers it with your **OS Keyring** (under service `cipherclipboard`).
2. Creates an encrypted database `clipboard_history.db` in `~/Library/Application Support/cipherclipboard/` (or `%LOCALAPPDATA%\cipherclipboard\` on Windows).
3. Opens the UI and starts monitoring copied text in the background.

---

## Known Limitations

* **Monochrome Emoji & Symbol Rendering:** `egui` (`0.29`) rasterizes font glyphs via `ab_glyph` using monochrome vector outlines and does not support color emoji tables (`sbix`, `COLR`/`CPAL`, or `CBDT`). As a result, emojis and symbols in copied text render in the active theme's monochrome text color rather than full color. Invisible Unicode variation selectors (`U+FE0E`, `U+FE0F`) and zero-width joiners are stripped from UI previews to avoid missing-glyph (`□`) boxes, while the original text is preserved intact when copied back to the clipboard.

---

## Educational Next Steps to Practice Rust

1. **Global Hotkey:** Add the [`global-hotkey`](https://crates.io/crates/global-hotkey) crate to summon/hide the window with a shortcut (e.g. `Cmd+Shift+V`).
2. **Password Manager Filter:** Inspect clipboard metadata (or regex patterns) to ignore entries marked as passwords.
3. **Data Expiration:** Add a TTL (Time-To-Live) feature that automatically purges clipboard items older than $N$ hours.
