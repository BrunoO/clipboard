#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unexpected_cfgs)]

mod app;
mod code;
mod color;
mod crypto;
mod db;
mod hotkey;
mod media;
mod monitor;
mod platform;
mod settings;
mod theme;

use app::ClipboardApp;
use db::Database;
use eframe::egui;
use hotkey::HotkeyService;
use settings::AppSettings;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting CipherClipboard (Encrypted Clipboard Manager)...");

    // 1. Resolve application data directory (cross-platform: macOS, Windows, Linux)
    let data_dir = get_app_data_dir();
    fs::create_dir_all(&data_dir)?;
    println!("📁 Data directory: {}", data_dir.display());

    // 2. Resolve database path and check if it already exists
    let db_path = data_dir.join("clipboard_history.db");
    let db_exists = db_path.exists();

    // 3. Fetch or initialize the 256-bit encryption key
    let encryption_key = crypto::get_or_create_encryption_key(&data_dir, db_exists)?;

    // 4. Open or initialize the SQLCipher encrypted database
    let db = Database::open(&db_path, &encryption_key)?;
    println!("🛡️  Database opened & verified with SQLCipher.");

    // 5. Load persistent application settings
    let settings = AppSettings::load(&db);
    println!("⚙️  Loaded settings: hotkey preset {:?}", settings.hotkey);

    // 6. Configure window options for eframe
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([440.0, 580.0])
        .with_min_inner_size([320.0, 400.0])
        .with_title("CipherClipboard");

    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }

    // Load bundled icon for the window title bar and taskbar
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // 7. Setup inter-thread channels
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();
    let (hotkey_tx, hotkey_rx) = std::sync::mpsc::channel();

    // 8. Start GUI, global hotkeys, and background monitor
    eframe::run_native(
        "CipherClipboard",
        native_options,
        Box::new(move |cc| {
            // Register Phosphor icons and probe native system fonts (San Francisco, Segoe UI)
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            theme::setup_fonts(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);

            // Apply platform-adaptive HIG visuals (Dark or Light)
            let is_dark = settings.theme.resolve(&cc.egui_ctx);
            theme::setup_theme(&cc.egui_ctx, is_dark);

            // Initialize OS activation & reopen observers (macOS Dock click / Cmd+Tab)
            platform::set_repaint_context(&cc.egui_ctx);
            platform::setup_reopen_observer();

            // Initialize OS global hotkey service with user's configured hotkey
            let initial_hotkeys = settings.hotkey.to_hotkeys();
            let hotkey_service =
                match HotkeyService::new(initial_hotkeys, hotkey_tx, cc.egui_ctx.clone()) {
                    Ok(service) => Some(service),
                    Err(e) => {
                        eprintln!("⚠️ Failed to register global hotkey: {e}");
                        None
                    }
                };

            // Start background clipboard polling thread
            let monitor_db = db.clone();
            let monitor_ctx = cc.egui_ctx.clone();
            monitor::start_clipboard_monitor(monitor_db, monitor_ctx, notify_tx);

            Ok(Box::new(ClipboardApp::new(
                cc,
                db,
                notify_rx,
                hotkey_rx,
                hotkey_service,
                settings,
            )))
        }),
    )?;

    Ok(())
}

/// Computes the OS-appropriate local application data directory.
///
/// - macOS:   ~/Library/Application Support/cipherclipboard/
/// - Windows: %LOCALAPPDATA%\cipherclipboard\
/// - Linux:   ~/.local/share/cipherclipboard/
fn get_app_data_dir() -> PathBuf {
    dirs::data_local_dir().map_or_else(
        || PathBuf::from("./data"),
        |p| {
            let new_dir = p.join("cipherclipboard");
            let legacy_dir = p.join("rusty-clipboard");
            if !new_dir.exists() && legacy_dir.exists() {
                legacy_dir
            } else {
                new_dir
            }
        },
    )
}
