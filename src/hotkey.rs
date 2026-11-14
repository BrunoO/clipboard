use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{Arc, RwLock};
use std::thread;

/// Manages system-wide global hotkey registration and event delivery.
pub(crate) struct HotkeyService {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
    target_ids: Arc<RwLock<Vec<u32>>>,
    shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
}

impl HotkeyService {
    pub(crate) fn new(
        initial_hotkeys: Vec<HotKey>,
        hotkey_tx: std::sync::mpsc::Sender<()>,
        ctx: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;
        let mut registered = Vec::new();
        let mut ids = Vec::new();

        for hk in initial_hotkeys {
            if let Err(e) = manager.register(hk) {
                eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
            } else {
                ids.push(hk.id());
                registered.push(hk);
            }
        }

        let target_ids = Arc::new(RwLock::new(ids));
        let thread_ids = Arc::clone(&target_ids);
        let receiver = GlobalHotKeyEvent::receiver();

        // Background thread listens for OS hotkey events and wakes up egui
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        thread::spawn(move || {
            loop {
                // Check for shutdown signal first
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                // Wait for hotkey event with timeout to allow shutdown checks
                let event = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(e) => e,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                };

                let matches = {
                    let lock = thread_ids
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    lock.contains(&event.id)
                };
                if matches && event.state == HotKeyState::Pressed {
                    crate::platform::capture_active_window();
                    let _ = hotkey_tx.send(());
                    ctx.request_repaint();
                }
            }
        });

        Ok(Self {
            manager,
            hotkeys: registered,
            target_ids,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Dynamically re-registers global hotkeys at runtime.
    pub(crate) fn update_hotkeys(&mut self, new_hotkeys: &[HotKey]) {
        for hk in &self.hotkeys {
            if let Err(e) = self.manager.unregister(*hk) {
                eprintln!("⚠️ Could not unregister hotkey {hk:?}: {e}");
            }
        }
        self.hotkeys.clear();

        let mut new_ids = Vec::new();
        for &hk in new_hotkeys {
            if let Err(e) = self.manager.register(hk) {
                eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
            } else {
                new_ids.push(hk.id());
                self.hotkeys.push(hk);
            }
        }

        let mut lock = self.target_ids.write().unwrap_or_else(|p| {
            eprintln!("⚠️ Hotkey target_ids mutex recovered from poison");
            p.into_inner()
        });
        *lock = new_ids;
    }

    /// Signals the background hotkey listener thread to shut down.
    pub(crate) fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
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
