use crate::db::Database;
use crate::media;
use arboard::Clipboard;
use eframe::egui;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::mpsc::Sender;
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
    notify_tx: Sender<()>,
) -> (thread::JoinHandle<()>, Sender<()>) {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

    let handle = thread::spawn(move || {
        // On Windows the clipboard can be briefly unavailable (e.g. at session
        // logon with auto-start, while disconnected from an RDP session, or
        // during session switches). Retry instead of silently disabling
        // clipboard capture until the app is restarted.
        let mut clipboard = loop {
            match Clipboard::new() {
                Ok(cb) => break cb,
                Err(e) => {
                    eprintln!("⚠️ Failed to initialize clipboard monitor: {e}");
                    if shutdown_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
                        println!("👀 Clipboard monitor shutting down...");
                        return;
                    }
                }
            }
        };

        let mut last_copied_text: Option<String> = None;
        let mut last_copied_image_hash: Option<u64> = None;

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

            // Skip processing if marked as concealed/transient by password managers
            if db.ignore_concealed() && crate::platform::is_clipboard_concealed() {
                continue;
            }

            // 1. Check for text on clipboard
            if let Ok(text) = clipboard.get_text() {
                let trimmed = text.trim();
                if !trimmed.is_empty()
                    && text.len() <= MAX_TEXT_LENGTH
                    && last_copied_text.as_deref() != Some(&text)
                {
                    last_copied_text = Some(text.clone());
                    last_copied_image_hash = None;

                    if db.take_if_skipped(&text) {
                        continue;
                    }

                    match db.insert(&text) {
                        Ok(Some(id)) => {
                            log_captured_text(id, &text);
                            let _ = notify_tx.send(());
                            ctx.request_repaint();
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("⚠️ Failed to save clipboard item to database: {e}");
                        }
                    }
                    continue;
                }
            }

            // 2. Check for image on clipboard
            if let Ok(img) = clipboard.get_image() {
                let width = img.width;
                let height = img.height;
                let len = img.bytes.len();

                if width > 0
                    && height > 0
                    && width.saturating_mul(height) <= MAX_IMAGE_PIXELS
                    && len == width.saturating_mul(height).saturating_mul(4)
                {
                    let fingerprint = compute_image_fingerprint(width, height, &img.bytes);
                    if last_copied_image_hash != Some(fingerprint) {
                        last_copied_image_hash = Some(fingerprint);
                        last_copied_text = None;

                        match media::rgba_to_png(width, height, &img.bytes) {
                            Ok(png_bytes) => {
                                let width_u32 = u32::try_from(width).unwrap_or(0);
                                let height_u32 = u32::try_from(height).unwrap_or(0);
                                match db.insert_image(width_u32, height_u32, &png_bytes) {
                                    Ok(Some(id)) => {
                                        #[cfg(debug_assertions)]
                                        println!(
                                            "🖼️ Captured new image clipboard entry [id: {id}] ({width}x{height})"
                                        );
                                        #[cfg(not(debug_assertions))]
                                        let _ = id;
                                        let _ = notify_tx.send(());
                                        ctx.request_repaint();
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        eprintln!("⚠️ Failed to save image to database: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("⚠️ Failed to encode clipboard image to PNG: {e}");
                            }
                        }
                    }
                }
            }
        }
    });

    (handle, shutdown_tx)
}

#[cfg(debug_assertions)]
fn log_captured_text(id: i64, text: &str) {
    if crate::secret::is_likely_secret(text) {
        println!("🔒 Captured new secret clipboard entry [id: {id}] (<redacted>)");
    } else {
        let preview = if text.chars().count() > 40 {
            let truncated: String = text.chars().take(40).collect();
            format!("{truncated}...")
        } else {
            text.to_string()
        };
        println!("📋 Captured new text clipboard entry [id: {id}]: {preview:?}");
    }
}

#[cfg(not(debug_assertions))]
const fn log_captured_text(_id: i64, _text: &str) {}
