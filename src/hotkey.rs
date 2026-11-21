use crate::platform::QuoteHotkeyFallback;
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{Arc, RwLock};
use std::thread;

/// Result of a hotkey registration attempt.
#[derive(Debug, Clone)]
pub(crate) struct HotkeyRegistrationResult {
    pub(crate) hotkey: HotKey,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
}

/// Manages system-wide global hotkey registration and event delivery.
pub(crate) struct HotkeyService {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
    target_ids: Arc<RwLock<Vec<u32>>>,
    shutdown_tx: Option<crossbeam_channel::Sender<()>>,
    listener_handle: Option<thread::JoinHandle<()>>,
    last_registration_results: Vec<HotkeyRegistrationResult>,
    quote_fallback: QuoteHotkeyFallback,
}

fn register_hotkey_batch(
    manager: &GlobalHotKeyManager,
    hotkeys: &[HotKey],
    quote_fallback: &QuoteHotkeyFallback,
) -> (Vec<HotKey>, Vec<u32>, Vec<HotkeyRegistrationResult>) {
    let mut registered = Vec::new();
    let mut ids = Vec::new();
    let mut results = Vec::new();

    for &hk in hotkeys {
        match manager.register(hk) {
            Ok(()) => {
                ids.push(hk.id());
                registered.push(hk);
                results.push(HotkeyRegistrationResult {
                    hotkey: hk,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                if quote_fallback.try_register(hk) {
                    results.push(HotkeyRegistrationResult {
                        hotkey: hk,
                        success: true,
                        error: None,
                    });
                } else {
                    eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
                    crate::logging::log_message(
                        "WARN",
                        &format!("Could not register hotkey {hk:?}: {e}"),
                    );
                    results.push(HotkeyRegistrationResult {
                        hotkey: hk,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
    }

    (registered, ids, results)
}

impl HotkeyService {
    /// Initializes the global hotkey manager and registers the initial hotkeys.
    ///
    /// # Errors
    /// Returns an error if the platform hotkey manager cannot be created or a
    /// requested hotkey cannot be registered.
    pub(crate) fn new(
        initial_hotkeys: &[HotKey],
        hotkey_tx: std::sync::mpsc::SyncSender<()>,
        ctx: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;
        let quote_fallback = QuoteHotkeyFallback::new(hotkey_tx.clone(), ctx.clone());

        let (registered, ids, results) =
            register_hotkey_batch(&manager, initial_hotkeys, &quote_fallback);

        let target_ids = Arc::new(RwLock::new(ids));
        let thread_ids = Arc::clone(&target_ids);
        let receiver = GlobalHotKeyEvent::receiver();

        // Background thread blocks via crossbeam::select! on OS hotkey events and shutdown
        // with zero idle polling wakeups.
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let listener_handle = thread::Builder::new()
            .name("hotkey-listener".to_string())
            .spawn(move || loop {
                crossbeam_channel::select! {
                    recv(shutdown_rx) -> _ => break,
                    recv(receiver) -> msg => {
                        let Ok(event) = msg else {
                            break;
                        };
                        let matches = {
                            let lock = thread_ids
                                .read()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            lock.contains(&event.id)
                        };
                        if matches && event.state == HotKeyState::Pressed {
                            crate::platform::capture_active_window();
                            // Best-effort coalesced wakeup; a full or closed channel needs no action.
                            let _ = hotkey_tx.try_send(());
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .ok();

        Ok(Self {
            manager,
            hotkeys: registered,
            target_ids,
            shutdown_tx: Some(shutdown_tx),
            listener_handle,
            last_registration_results: results,
            quote_fallback,
        })
    }

    /// Dynamically re-registers global hotkeys at runtime.
    /// Returns registration results for each hotkey.
    pub(crate) fn update_hotkeys(
        &mut self,
        new_hotkeys: &[HotKey],
    ) -> Vec<HotkeyRegistrationResult> {
        // Unregister all existing hotkeys
        for hk in &self.hotkeys {
            if let Err(e) = self.manager.unregister(*hk) {
                eprintln!("⚠️ Could not unregister hotkey {hk:?}: {e}");
            }
        }
        self.hotkeys.clear();
        self.quote_fallback.clear();

        let (registered, new_ids, results) =
            register_hotkey_batch(&self.manager, new_hotkeys, &self.quote_fallback);
        self.hotkeys = registered;

        let mut lock = self.target_ids.write().unwrap_or_else(|p| {
            eprintln!("⚠️ Hotkey target_ids mutex recovered from poison");
            p.into_inner()
        });
        *lock = new_ids;

        self.last_registration_results.clone_from(&results);

        results
    }

    /// Returns the last registration results for UI display.
    pub(crate) fn registration_results(&self) -> Vec<HotkeyRegistrationResult> {
        self.last_registration_results.clone()
    }

    /// Signals the background hotkey listener threads to shut down and joins them.
    pub(crate) fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            // The listener may already have exited; the join below is authoritative.
            let _ = tx.try_send(());
        }
        if let Some(handle) = self.listener_handle.take() {
            let _ = handle.join();
        }
        self.quote_fallback.shutdown();
    }
}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        self.shutdown();
        for hk in &self.hotkeys {
            if let Err(e) = self.manager.unregister(*hk) {
                eprintln!("⚠️ Failed to unregister hotkey {hk:?} on drop: {e}");
            }
        }
    }
}
