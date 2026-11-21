//! Cross-platform focus restoration and simulated paste automation.
//!
//! When `CipherClipboard` is summoned via a global hotkey, it takes OS window focus.
//! This module captures the active application/window *before* `CipherClipboard` takes
//! focus, and restores focus and simulates `Cmd+V` (macOS) or `Ctrl+V` (Windows/Linux)
//! when an item is chosen.

use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

/// Stores a clone of the `egui::Context` to immediately trigger repaints on OS activation.
pub(crate) fn set_repaint_context(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "macos")]
    macos::set_repaint_context(ctx);
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}

/// Returns true if the application currently has OS focus/activation.
#[cfg(test)]
#[must_use]
pub(crate) fn is_app_active() -> bool {
    is_app_active_state().unwrap_or(false)
}

/// Returns `Some(true)` if this application is currently active at the OS/WM level,
/// `Some(false)` if another application is active, or `None` if the platform/session
/// cannot determine active-window ownership (e.g. pure Wayland without X11).
#[must_use]
#[cfg_attr(
    any(target_os = "macos", target_os = "windows"),
    allow(clippy::unnecessary_wraps)
)]
pub(crate) fn is_app_active_state() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::is_active())
    }
    #[cfg(target_os = "windows")]
    {
        Some(windows::is_active())
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_active()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// Returns `true` on Windows when the current keyboard layout produces `ù` for the
/// physical key in the US `Quote` position (French/Belgian AZERTY layouts).
#[must_use]
#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::missing_const_for_fn,
        reason = "non-Windows stub keeps the cross-platform facade uniform"
    )
)]
pub(crate) fn quote_key_is_ugrave() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::quote_key_is_ugrave()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Applies the platform title-bar dark mode attribute where supported (Windows DWM).
#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::missing_const_for_fn,
        reason = "non-Windows stub keeps the cross-platform facade uniform"
    )
)]
pub(crate) fn set_title_bar_dark_mode(is_dark: bool) {
    #[cfg(target_os = "windows")]
    {
        windows::set_title_bar_dark_mode(is_dark);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = is_dark;
    }
}

use crate::logging::log_message;

/// Returns a monotonic OS clipboard change counter when supported by the platform
/// (macOS `NSPasteboard.changeCount`, Windows `GetClipboardSequenceNumber`), allowing
/// the monitor thread to skip reading/copying unchanged clipboard payloads.
#[must_use]
pub(crate) fn clipboard_change_token() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        macos::clipboard_change_token()
    }
    #[cfg(target_os = "windows")]
    {
        windows::clipboard_change_token()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Checks if the current clipboard content is marked as concealed or transient
/// by password managers (e.g. 1Password, Bitwarden, `KeePass`, `KeePassXC`, macOS Keychain).
#[must_use]
pub(crate) fn is_clipboard_concealed() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_clipboard_concealed()
    }
    #[cfg(target_os = "windows")]
    {
        windows::is_clipboard_concealed()
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_clipboard_concealed()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// Attaches OS-native privacy/concealed markers to the current clipboard entry
/// so OS clipboard history, cloud sync, Universal Clipboard, and other clipboard
/// managers do not record or broadcast secrets copied by `CipherClipboard`.
pub(crate) fn mark_clipboard_concealed() {
    #[cfg(target_os = "macos")]
    macos::mark_clipboard_concealed();

    #[cfg(target_os = "windows")]
    windows::mark_clipboard_concealed();
}

/// Hides the entire application at the OS level (macOS `[NSApp hide:]`).
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn hide_application() {
    #[cfg(target_os = "macos")]
    macos::hide_app();
}

/// Unhides and orders the main window front at the OS level.
pub(crate) fn order_front_main_window() {
    #[cfg(target_os = "macos")]
    macos::order_front_main_window();

    #[cfg(target_os = "windows")]
    windows::order_front_main_window();
}

/// Checks and clears any pending reopen requests triggered by OS events (e.g. Dock icon click).
#[must_use]
pub(crate) fn take_reopen_requested() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::take_reopen_requested()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Registers OS-level activation and reopen observers (e.g. macOS Dock click / Cmd+Tab notifications).
pub(crate) fn setup_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::setup_reopen_observer();
}

/// Removes the macOS reopen observer to prevent crashes on app termination.
pub(crate) fn teardown_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::teardown_reopen_observer();
}

/// Captures the current frontmost application/window before `CipherClipboard` gains focus.
pub(crate) fn capture_active_window() {
    #[cfg(target_os = "macos")]
    macos::capture_frontmost();

    #[cfg(target_os = "windows")]
    windows::capture_frontmost();

    #[cfg(target_os = "linux")]
    linux::capture_frontmost();
}

#[derive(Clone, Copy)]
struct PasteRequest {
    restore_focus: bool,
    auto_paste: bool,
    auto_paste_delay_ms: u64,
}

fn restore_focus_platform() {
    #[cfg(target_os = "macos")]
    macos::restore_focus();

    #[cfg(target_os = "windows")]
    windows::restore_focus();

    #[cfg(target_os = "linux")]
    linux::restore_focus();
}

fn simulate_paste_if_foreground() {
    #[cfg(target_os = "macos")]
    {
        if !macos::is_target_foreground() {
            log_message(
                "WARN",
                "Auto-paste skipped: captured target application is not frontmost",
            );
            return;
        }
        macos::simulate_paste();
    }

    #[cfg(target_os = "windows")]
    {
        if !windows::is_target_foreground() {
            log_message(
                "WARN",
                "Auto-paste skipped: captured target window is not in the foreground",
            );
            return;
        }
        let _ = windows::simulate_paste();
    }

    #[cfg(target_os = "linux")]
    {
        if !linux::is_target_foreground() {
            log_message(
                "WARN",
                "Auto-paste skipped: captured target window is not active",
            );
            return;
        }
        linux::simulate_paste();
    }
}

// The sender lives only in `PasteWorker`; shutdown drops it so both the `recv`
// and `recv_timeout` Disconnected arms below become reachable.
fn paste_worker_loop(rx: &std::sync::mpsc::Receiver<PasteRequest>) {
    let mut pending: Option<PasteRequest> = None;
    loop {
        let mut req = if let Some(r) = pending.take() {
            r
        } else {
            let Ok(r) = rx.recv() else {
                break;
            };
            r
        };
        // Coalesce any burst of queued requests, keeping only the latest.
        // A disconnected channel means shutdown already dropped the sender, so
        // do not begin new focus/paste side effects for this request.
        loop {
            match rx.try_recv() {
                Ok(newer) => req = newer,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // If auto_paste is active, focus MUST be switched to the target application
        // so the simulated keystrokes are received by that application.
        if req.restore_focus || req.auto_paste {
            restore_focus_platform();
        }

        if req.auto_paste {
            // Give the OS window server time to switch key focus to the target
            // application before dispatching keystrokes. If a newer request arrives
            // during this window, supersede this paste immediately.
            match rx.recv_timeout(Duration::from_millis(req.auto_paste_delay_ms)) {
                Ok(newer) => {
                    pending = Some(newer);
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            // Shutdown may have dropped the sender just after the delay elapsed.
            // Also honor a request that arrived in that narrow window.
            match rx.try_recv() {
                Ok(newer) => {
                    pending = Some(newer);
                    continue;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }

            simulate_paste_if_foreground();
        }
    }
}

/// Owned paste-worker state. Dropping the sender lets the worker exit through
/// its existing `Disconnected` break path; the join handle is taken exactly once.
#[derive(Debug)]
struct PasteWorker {
    sender: std::sync::Mutex<Option<std::sync::mpsc::Sender<PasteRequest>>>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl PasteWorker {
    fn send(&self, request: PasteRequest) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(request);
        }
    }

    /// Drops the only sender, then joins the worker so in-flight OS side effects complete.
    fn shutdown(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(sender);

        let handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            if handle.join().is_err() {
                log_message("ERROR", "focus-paste worker panicked during shutdown");
            }
        }
    }
}

static PASTE_WORKER: std::sync::OnceLock<PasteWorker> = std::sync::OnceLock::new();

fn paste_worker() -> &'static PasteWorker {
    PASTE_WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<PasteRequest>();
        let handle = std::thread::Builder::new()
            .name("focus-paste-worker".to_string())
            .spawn(move || paste_worker_loop(&rx));
        if let Err(e) = &handle {
            log_message("ERROR", &format!("Failed to spawn focus-paste worker: {e}"));
        }
        PasteWorker {
            sender: std::sync::Mutex::new(Some(tx)),
            handle: std::sync::Mutex::new(handle.ok()),
        }
    })
}

/// Shuts down the paste worker if it was started. Safe to call more than once.
pub(crate) fn shutdown_paste_worker() {
    if let Some(worker) = PASTE_WORKER.get() {
        worker.shutdown();
    }
}

/// Restores focus to the previous application and optionally simulates a paste keystroke.
pub(crate) fn restore_focus_and_paste(
    restore_focus: bool,
    auto_paste: bool,
    auto_paste_delay_ms: u64,
) {
    if !restore_focus && !auto_paste {
        return;
    }

    paste_worker().send(PasteRequest {
        restore_focus,
        auto_paste,
        auto_paste_delay_ms,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_capture_and_restore_no_panic() {
        // Calling capture and restore should never panic on any platform
        capture_active_window();
        restore_focus_and_paste(false, false, 120);
        #[cfg(target_os = "macos")]
        assert!(macos::is_trusted() || !macos::is_trusted());
        let _ = is_app_active();
        let _ = is_clipboard_concealed();
        let _ = take_reopen_requested();
        hide_application();
        order_front_main_window();
        setup_reopen_observer();
    }

    #[test]
    fn test_paste_worker_shutdown_joins_and_is_idempotent() {
        let (tx, rx) = std::sync::mpsc::channel::<PasteRequest>();
        let handle = std::thread::Builder::new()
            .name("test-focus-paste-worker".to_string())
            .spawn(move || paste_worker_loop(&rx))
            .unwrap();
        let worker = PasteWorker {
            sender: std::sync::Mutex::new(Some(tx)),
            handle: std::sync::Mutex::new(Some(handle)),
        };

        worker.send(PasteRequest {
            restore_focus: false,
            auto_paste: false,
            auto_paste_delay_ms: 0,
        });
        worker.shutdown();
        worker.shutdown();

        assert!(worker
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none());
        assert!(worker
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none());
    }
}
