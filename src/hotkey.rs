use crate::platform::QuoteHotkeyFallback;
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;

/// Distinct actions triggered by registered global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyAction {
    ToggleWindow,
    PasteNext,
}

impl HotkeyAction {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ToggleWindow => "Toggle Window",
            Self::PasteNext => "Paste Next",
        }
    }
}

/// Result of a hotkey registration attempt.
#[derive(Debug, Clone)]
pub(crate) struct HotkeyRegistrationResult {
    pub(crate) action: HotkeyAction,
    pub(crate) hotkey: HotKey,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
}

/// Manages system-wide global hotkey registration and event delivery.
pub(crate) struct HotkeyService {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
    target_actions: Arc<RwLock<HashMap<u32, HotkeyAction>>>,
    shutdown_tx: Option<crossbeam_channel::Sender<()>>,
    listener_handle: Option<thread::JoinHandle<()>>,
    last_registration_results: Vec<HotkeyRegistrationResult>,
    quote_fallback: QuoteHotkeyFallback,
}

fn register_hotkey_batch(
    manager: &GlobalHotKeyManager,
    entries: &[(HotkeyAction, HotKey)],
    quote_fallback: &QuoteHotkeyFallback,
) -> (
    Vec<HotKey>,
    HashMap<u32, HotkeyAction>,
    Vec<HotkeyRegistrationResult>,
) {
    let mut registered = Vec::new();
    let mut action_map = HashMap::new();
    let mut results = Vec::new();

    for &(action, hk) in entries {
        match manager.register(hk) {
            Ok(()) => {
                action_map.insert(hk.id(), action);
                registered.push(hk);
                results.push(HotkeyRegistrationResult {
                    action,
                    hotkey: hk,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                if quote_fallback.try_register(hk, action) {
                    results.push(HotkeyRegistrationResult {
                        action,
                        hotkey: hk,
                        success: true,
                        error: None,
                    });
                } else {
                    eprintln!("⚠️ Could not register hotkey {hk:?} ({action:?}): {e}");
                    crate::logging::log_message(
                        "WARN",
                        &format!("Could not register hotkey {hk:?} ({action:?}): {e}"),
                    );
                    results.push(HotkeyRegistrationResult {
                        action,
                        hotkey: hk,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
    }

    (registered, action_map, results)
}

fn collect_hotkey_entries(
    toggle_hotkey: Option<HotKey>,
    paste_next_hotkey: Option<HotKey>,
) -> Vec<(HotkeyAction, HotKey)> {
    let mut entries = Vec::with_capacity(2);
    if let Some(hk) = toggle_hotkey {
        entries.push((HotkeyAction::ToggleWindow, hk));
    }
    if let Some(hk) = paste_next_hotkey {
        entries.push((HotkeyAction::PasteNext, hk));
    }
    entries
}

impl HotkeyService {
    /// Initializes the global hotkey manager and registers the initial hotkeys.
    ///
    /// # Errors
    /// Returns an error if the platform hotkey manager cannot be created.
    pub(crate) fn new(
        toggle_hotkey: Option<HotKey>,
        paste_next_hotkey: Option<HotKey>,
        hotkey_tx: std::sync::mpsc::SyncSender<()>,
        paste_next_tx: crossbeam_channel::Sender<()>,
        ctx: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;
        let quote_fallback =
            QuoteHotkeyFallback::new(hotkey_tx.clone(), paste_next_tx.clone(), ctx.clone());

        let entries = collect_hotkey_entries(toggle_hotkey, paste_next_hotkey);
        let (registered, action_map, results) =
            register_hotkey_batch(&manager, &entries, &quote_fallback);

        let target_actions = Arc::new(RwLock::new(action_map));
        let thread_actions = Arc::clone(&target_actions);
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
                        if event.state != HotKeyState::Pressed {
                            continue;
                        }
                        let action = {
                            let lock = thread_actions
                                .read()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            lock.get(&event.id).copied()
                        };
                        match action {
                            Some(HotkeyAction::ToggleWindow) => {
                                #[cfg(debug_assertions)]
                                crate::logging::log_message(
                                    "INFO",
                                    "Toggle Window hotkey pressed; queued wakeup for UI thread",
                                );
                                crate::platform::capture_active_window();
                                // Best-effort coalesced wakeup; a full or closed channel needs no action.
                                let _ = hotkey_tx.try_send(());
                                crate::platform::wake_hidden_window_for_hotkey();
                                ctx.request_repaint();
                            }
                            Some(HotkeyAction::PasteNext) => {
                                if crate::platform::is_paste_next_in_flight() {
                                    continue;
                                }
                                crate::logging::log_message(
                                    "INFO",
                                    "Paste Next hotkey pressed; queued event for UI thread",
                                );
                                if let Err(e) = paste_next_tx.try_send(()) {
                                    crate::logging::log_message(
                                        "WARN",
                                        &format!("Paste Next channel full/closed: {e}"),
                                    );
                                }
                                ctx.request_repaint();
                            }
                            None => {}
                        }
                    }
                }
            })
            .ok();

        Ok(Self {
            manager,
            hotkeys: registered,
            target_actions,
            shutdown_tx: Some(shutdown_tx),
            listener_handle,
            last_registration_results: results,
            quote_fallback,
        })
    }

    /// Dynamically re-registers global hotkeys at runtime.
    /// Returns registration results for each requested action.
    pub(crate) fn update_hotkeys(
        &mut self,
        toggle_hotkey: Option<HotKey>,
        paste_next_hotkey: Option<HotKey>,
    ) -> Vec<HotkeyRegistrationResult> {
        // Unregister all existing hotkeys
        for hk in &self.hotkeys {
            if let Err(e) = self.manager.unregister(*hk) {
                eprintln!("⚠️ Could not unregister hotkey {hk:?}: {e}");
            }
        }
        self.hotkeys.clear();
        self.quote_fallback.clear();

        let entries = collect_hotkey_entries(toggle_hotkey, paste_next_hotkey);
        let (registered, new_map, results) =
            register_hotkey_batch(&self.manager, &entries, &self.quote_fallback);
        self.hotkeys = registered;

        let mut lock = self.target_actions.write().unwrap_or_else(|p| {
            eprintln!("⚠️ Hotkey target_actions mutex recovered from poison");
            p.into_inner()
        });
        *lock = new_map;

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
