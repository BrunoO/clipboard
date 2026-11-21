use crate::db::Database;
use crate::media;
use arboard::Clipboard;
use eframe::egui;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::mpsc::{Sender, SyncSender};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_TEXT_LENGTH: usize = 500_000; // Limit to ~500 KB to avoid accidental huge pastes
const MAX_IMAGE_PIXELS: usize = 40_000_000; // Limit to ~40 MP to guard memory

fn compute_image_fingerprint(width: usize, height: usize, rgba: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write_usize(width);
    hasher.write_usize(height);
    hasher.write_usize(rgba.len());
    if rgba.len() <= 65536 {
        hasher.write(rgba);
    } else {
        hasher.write(&rgba[..16384]);
        let mid = rgba.len() / 2;
        hasher.write(&rgba[mid..mid + 16384]);
        hasher.write(&rgba[rgba.len() - 16384..]);
    }
    hasher.finish()
}

/// Starts the background thread that monitors the OS clipboard for text and image changes.
///
/// Whenever new text or an image is copied:
/// 1. It checks if the content matches the last seen value.
/// 2. Saves it into the encrypted database.
/// 3. Sends a notification on `notify_tx`.
/// 4. Signals egui (`ctx.request_repaint()`) to update the GUI immediately.
pub(crate) fn start_clipboard_monitor(
    db: Database,
    ctx: egui::Context,
    notify_tx: SyncSender<()>,
) -> (Option<thread::JoinHandle<()>>, Sender<()>) {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

    let handle = thread::Builder::new()
        .name("clipboard-monitor".to_string())
        .spawn(move || {
            let Some(mut clipboard) = init_clipboard_with_retry(&shutdown_rx) else {
                return;
            };

            let mut state = MonitorState::default();

            println!("👀 Clipboard monitor active (polling every 200ms for text & images)...");

            loop {
                // Wait for POLL_INTERVAL, waking immediately if a shutdown signal arrives
                match shutdown_rx.recv_timeout(POLL_INTERVAL) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        println!("👀 Clipboard monitor shutting down...");
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }

                // Fast path: when the OS exposes a monotonic clipboard change counter
                // (macOS NSPasteboard.changeCount, Windows GetClipboardSequenceNumber),
                // skip all clipboard reads/allocations/hashing when nothing has changed.
                let change_token = crate::platform::clipboard_change_token();
                if change_token.is_some() && state.change_token == change_token {
                    continue;
                }

                // Skip processing if marked as concealed/transient by password managers
                if db.ignore_concealed() && crate::platform::is_clipboard_concealed() {
                    state.change_token = change_token;
                    continue;
                }

                match poll_clipboard_text(&mut clipboard, &db, &notify_tx, &ctx, &mut state) {
                    PollOutcome::Handled => {
                        state.change_token = change_token;
                        continue;
                    }
                    PollOutcome::Occupied => {
                        // Clipboard is transiently locked by another process; retry next tick.
                        continue;
                    }
                    PollOutcome::NotAvailable => {}
                }

                match poll_clipboard_image(&mut clipboard, &db, &notify_tx, &ctx, &mut state) {
                    PollOutcome::Handled | PollOutcome::NotAvailable => {
                        state.change_token = change_token;
                    }
                    PollOutcome::Occupied => {}
                }
            }
        })
        .ok();

    (handle, shutdown_tx)
}

#[derive(Default)]
struct MonitorState {
    last_copied_text: Option<String>,
    last_copied_image_hash: Option<u64>,
    change_token: Option<u64>,
}

enum PollOutcome {
    Handled,
    NotAvailable,
    Occupied,
}

/// On Windows the clipboard can be briefly unavailable (e.g. at session logon
/// with auto-start, while disconnected from an RDP session, or during session
/// switches). Retry instead of silently disabling clipboard capture.
fn init_clipboard_with_retry(shutdown_rx: &std::sync::mpsc::Receiver<()>) -> Option<Clipboard> {
    loop {
        match Clipboard::new() {
            Ok(cb) => return Some(cb),
            Err(e) => {
                eprintln!("⚠️ Failed to initialize clipboard monitor: {e}");
                match shutdown_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        println!("👀 Clipboard monitor shutting down...");
                        return None;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }
}

fn poll_clipboard_text(
    clipboard: &mut Clipboard,
    db: &Database,
    notify_tx: &SyncSender<()>,
    ctx: &egui::Context,
    state: &mut MonitorState,
) -> PollOutcome {
    let text = match clipboard.get_text() {
        Ok(text) => text,
        Err(arboard::Error::ClipboardOccupied) => return PollOutcome::Occupied,
        Err(_) => return PollOutcome::NotAvailable,
    };
    if text.trim().is_empty() {
        return PollOutcome::NotAvailable;
    }
    if text.len() > MAX_TEXT_LENGTH || state.last_copied_text.as_deref() == Some(text.as_str()) {
        return PollOutcome::Handled;
    }

    state.last_copied_text = Some(text.clone());
    state.last_copied_image_hash = None;

    if db.take_if_skipped(&text) {
        return PollOutcome::Handled;
    }

    match db.insert(&text) {
        Ok(Some(id)) => {
            log_captured_text(id, &text);
            let _ = notify_tx.try_send(());
            ctx.request_repaint();
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("⚠️ Failed to save clipboard item to database: {e}");
        }
    }
    PollOutcome::Handled
}

fn poll_clipboard_image(
    clipboard: &mut Clipboard,
    db: &Database,
    notify_tx: &SyncSender<()>,
    ctx: &egui::Context,
    state: &mut MonitorState,
) -> PollOutcome {
    let img = match clipboard.get_image() {
        Ok(img) => img,
        Err(arboard::Error::ClipboardOccupied) => return PollOutcome::Occupied,
        Err(_) => return PollOutcome::NotAvailable,
    };
    let width = img.width;
    let height = img.height;
    let expected_len = width.saturating_mul(height).saturating_mul(4);
    if width == 0
        || height == 0
        || width.saturating_mul(height) > MAX_IMAGE_PIXELS
        || img.bytes.len() != expected_len
    {
        return PollOutcome::NotAvailable;
    }

    let fingerprint = compute_image_fingerprint(width, height, &img.bytes);
    if state.last_copied_image_hash == Some(fingerprint) {
        return PollOutcome::Handled;
    }
    state.last_copied_image_hash = Some(fingerprint);
    state.last_copied_text = None;

    let png_bytes = match media::rgba_to_png(width, height, &img.bytes) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("⚠️ Failed to encode clipboard image to PNG: {e}");
            return PollOutcome::Handled;
        }
    };

    let width_u32 = u32::try_from(width).unwrap_or(0);
    let height_u32 = u32::try_from(height).unwrap_or(0);
    match db.insert_image(width_u32, height_u32, &png_bytes) {
        Ok(Some(id)) => {
            #[cfg(debug_assertions)]
            println!("🖼️ Captured new image clipboard entry [id: {id}] ({width}x{height})");
            #[cfg(not(debug_assertions))]
            let _ = id;
            let _ = notify_tx.try_send(());
            ctx.request_repaint();
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("⚠️ Failed to save image to database: {e}");
        }
    }
    PollOutcome::Handled
}

#[cfg(debug_assertions)]
fn log_captured_text(id: i64, text: &str) {
    let classification = if crate::secret::is_likely_secret(text) {
        "secret-like"
    } else {
        "text"
    };
    println!(
        "📋 Captured new {classification} clipboard entry [id: {id}] ({} chars, {} bytes; preview withheld)",
        text.chars().count(),
        text.len()
    );
}

#[cfg(not(debug_assertions))]
const fn log_captured_text(_id: i64, _text: &str) {}
