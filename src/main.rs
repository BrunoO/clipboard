#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod code;
mod color;
mod crypto;
mod db;
mod hotkey;
mod icons;
mod instance;
mod logging;
mod media;
mod monitor;
mod password;
mod platform;
mod secret;
mod settings;
mod theme;
mod tray;

use app::ClipboardApp;
use db::Database;
use eframe::egui;
use hotkey::HotkeyService;
use instance::{AcquireResult, SingleInstanceLock};
use settings::AppSettings;
use std::fs;
use std::path::PathBuf;

fn main() {
    install_panic_hook();

    if let Err(e) = run() {
        eprintln!("Error: {e}");
        logging::log_message("ERROR", &format!("Startup failed: {e}"));
        platform::show_startup_error_dialog(&format!("CipherClipboard failed to start:\n\n{e}"));
        std::process::exit(1);
    }
}

/// Installs a panic hook that records crash diagnostics (thread, location, message,
/// and backtrace) to `app.log` before `panic = "abort"` terminates the process.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let location = info.location().map_or_else(
            || "<unknown location>".to_string(),
            |loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
        );
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let backtrace = std::backtrace::Backtrace::capture();

        let summary = format!("Thread '{thread_name}' panicked at {location}: {payload}");
        eprintln!("💥 {summary}\n{backtrace}");
        logging::log_message("PANIC", &format!("{summary}\n{backtrace}"));
        platform::show_panic_error_dialog(&summary);
    }));
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting CipherClipboard (Encrypted Clipboard Manager)...");

    // 1. Resolve application data directory (cross-platform: macOS, Windows, Linux)
    let data_dir = get_app_data_dir();
    fs::create_dir_all(&data_dir)?;
    crypto::set_restricted_dir_permissions(&data_dir);
    #[cfg(debug_assertions)]
    println!("📁 Data directory: {}", data_dir.display());

    // 1b. Enforce single-instance execution so two monitors don't race on the database
    let lock_path = data_dir.join(".instance.lock");
    let _instance_lock = match SingleInstanceLock::acquire(&lock_path)? {
        AcquireResult::Acquired(lock) => lock,
        AcquireResult::AlreadyRunning { pid } => {
            platform::activate_existing_instance(pid);

            if let Some(pid) = pid {
                println!(
                    "ℹ️ CipherClipboard is already running (PID {pid}). Brought existing window to front."
                );
            } else {
                println!(
                    "ℹ️ CipherClipboard is already running. Brought existing window to front."
                );
            }
            return Ok(());
        }
    };

    // 2. Open or initialize the SQLCipher encrypted database (key is zeroized on scope exit)
    let (db, key_storage_mode) = open_encrypted_database(&data_dir)?;
    #[cfg(debug_assertions)]
    println!("🛡️  Database opened & verified with SQLCipher.");

    // 5. Load persistent application settings
    let settings = AppSettings::load(&db);
    db.set_max_history(settings.max_history);
    db.set_ignore_concealed(settings.ignore_concealed);
    db.set_preserve_secrets_on_prune(settings.preserve_secrets_on_prune);
    if let Err(e) = db.prune_history(settings.max_history) {
        logging::log_message("WARN", &format!("Startup history prune failed: {e}"));
    }
    #[cfg(debug_assertions)]
    println!("⚙️  Loaded settings: hotkey preset {:?}", settings.hotkey);

    // 6. Configure window options for eframe
    let mut viewport = platform::configure_viewport_chrome(
        egui::ViewportBuilder::default()
            .with_inner_size([440.0, 580.0])
            .with_min_inner_size([320.0, 400.0])
            .with_title("CipherClipboard"),
    );

    // Load bundled icon for the window title bar and taskbar
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    crate::logging::log_message("INFO", "CipherClipboard starting");

    // 7. Setup inter-thread channels (bounded capacity 1 for coalesced wakeup signals)
    let (notify_tx, notify_rx) = std::sync::mpsc::sync_channel(1);
    let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel(1);
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();

    // 8. Start GUI, global hotkeys, system tray, and background monitor
    let result = eframe::run_native(
        "CipherClipboard",
        native_options,
        Box::new(move |cc| {
            // Probe native system fonts (San Francisco, Segoe UI) and register Material icons
            // Register outlined before filled so outline is the default fallback for inline icons,
            // while filled remains available explicitly (e.g. for selected/active states).
            let mut fonts = egui::FontDefinitions::default();
            theme::setup_fonts(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);
            let mut outlined = egui_material_icons::font_insert_outlined();
            outlined.data.tweak.scale = 1.20;
            outlined.data.tweak.y_offset_factor = 0.0;
            let mut filled = egui_material_icons::font_insert();
            filled.data.tweak.scale = 1.20;
            filled.data.tweak.y_offset_factor = 0.0;
            cc.egui_ctx.add_font(outlined);
            cc.egui_ctx.add_font(filled);

            // Apply platform-adaptive HIG visuals (Dark or Light)
            let is_dark = settings.theme.resolve(&cc.egui_ctx);
            theme::setup_theme(&cc.egui_ctx, is_dark);

            // Initialize OS activation & reopen observers (macOS Dock click / Cmd+Tab)
            platform::set_repaint_context(&cc.egui_ctx);
            platform::setup_reopen_observer();
            spawn_smoke_quit(&tray_tx, &cc.egui_ctx); // CI quit smoke test (debug only)

            // Initialize Windows system tray icon (runs on main UI thread event loop)
            let tray_service = tray::TrayService::new(&tray_tx, &cc.egui_ctx);

            // Initialize OS global hotkey service with user's configured hotkey
            let initial_hotkeys = settings.effective_hotkeys();
            let hotkey_service =
                match HotkeyService::new(&initial_hotkeys, hotkey_tx, cc.egui_ctx.clone()) {
                    Ok(service) => Some(service),
                    Err(e) => {
                        eprintln!("⚠️ Failed to register global hotkey: {e}");
                        None
                    }
                };

            // Start background clipboard polling thread
            let monitor_db = db.clone();
            let monitor_ctx = cc.egui_ctx.clone();
            let (monitor_handle, monitor_shutdown_tx) =
                monitor::start_clipboard_monitor(monitor_db, monitor_ctx, notify_tx);

            Ok(Box::new(ClipboardApp::new(
                cc,
                db,
                key_storage_mode,
                notify_rx,
                hotkey_rx,
                hotkey_service,
                tray_rx,
                tray_service,
                settings,
                monitor_shutdown_tx,
                monitor_handle,
            )))
        }),
    );

    // Cleanup on shutdown
    platform::teardown_reopen_observer();

    result?;

    Ok(())
}

/// CI smoke-test hook (active in debug builds only; a no-op in release builds).
///
/// When `CIPHERCLIPBOARD_SMOKE_QUIT_MS` is set, requests a regular quit through the same
/// path as the UI Quit button after the given delay, so CI can verify the app shuts down
/// cleanly (exit code 0, no AppKit/winit teardown crash).
fn spawn_smoke_quit(tray_tx: &std::sync::mpsc::Sender<tray::TrayAction>, ctx: &egui::Context) {
    #[cfg(debug_assertions)]
    {
        let Some(delay_ms) = std::env::var("CIPHERCLIPBOARD_SMOKE_QUIT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        else {
            return;
        };
        let tx = tray_tx.clone();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("smoke-quit".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                if tx.send(tray::TrayAction::Quit).is_ok() {
                    ctx.request_repaint();
                }
            });
    }
    #[cfg(not(debug_assertions))]
    let _ = (tray_tx, ctx);
}

/// Opens or initializes the `SQLCipher` database, ensuring the in-memory encryption key
/// is zeroized and dropped before returning.
fn open_encrypted_database(
    data_dir: &std::path::Path,
) -> Result<(Database, crypto::KeyStorageMode), Box<dyn std::error::Error>> {
    let db_path = data_dir.join("clipboard_history.db");
    let db_exists = db_path.exists();
    let (encryption_key, storage_mode) = crypto::get_or_create_encryption_key(data_dir, db_exists)?;
    let db = Database::open(&db_path, &encryption_key)?;
    Ok((db, storage_mode))
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
