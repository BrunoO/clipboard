use crate::db::Database;
use arboard::Clipboard;
use eframe::egui;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

/// Starts the background thread that monitors the OS clipboard for text changes.
///
/// Whenever a new, non-empty text entry is copied:
/// 1. It checks if the text matches the last seen value.
/// 2. Saves it into the encrypted database.
/// 3. Sends a notification on `notify_tx`.
/// 4. Signals egui (`ctx.request_repaint()`) to update the GUI immediately.
pub fn start_clipboard_monitor(
    db: Database,
    ctx: egui::Context,
    notify_tx: Sender<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                eprintln!("⚠️ Failed to initialize clipboard monitor: {e}");
                return;
            }
        };

        let mut last_copied: Option<String> = None;
        const POLL_INTERVAL: Duration = Duration::from_millis(200);
        const MAX_TEXT_LENGTH: usize = 500_000; // Limit to ~500 KB to avoid accidental huge pastes

        println!("👀 Clipboard monitor active (polling every 200ms)...");

        loop {
            thread::sleep(POLL_INTERVAL);

            match clipboard.get_text() {
                Ok(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() || text.len() > MAX_TEXT_LENGTH {
                        continue;
                    }

                    // If identical to the last seen text in memory, skip
                    if last_copied.as_deref() == Some(&text) {
                        continue;
                    }

                    last_copied = Some(text.clone());

                    // Insert into encrypted database
                    match db.insert(&text) {
                        Ok(Some(id)) => {
                            let preview = if text.len() > 40 {
                                format!("{}...", &text[..40])
                            } else {
                                text.clone()
                            };
                            println!("📋 Captured new clipboard entry [id: {id}]: {:?}", preview);

                            // Notify the UI thread to reload items from the DB
                            let _ = notify_tx.send(());
                            // Request UI repaint so new entry renders immediately
                            ctx.request_repaint();
                        }
                        Ok(None) => {
                            // Deduplicated against latest DB entry
                        }
                        Err(e) => {
                            eprintln!("⚠️ Failed to save clipboard item to database: {e}");
                        }
                    }
                }
                Err(arboard::Error::ClipboardOccupied) => {
                    // Clipboard is temporarily locked by another app while writing;
                    // next poll iteration will catch it.
                }
                Err(_) => {
                    // Other errors (e.g. non-text content copied, like an image or file)
                }
            }
        }
    })
}
