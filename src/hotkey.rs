use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;

/// Manages system-wide global hotkey registration and event delivery.
pub(crate) struct HotkeyService {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
    target_ids: Arc<RwLock<Vec<u32>>>,
}

impl HotkeyService {
    pub(crate) fn new(
        initial_hotkeys: Vec<HotKey>,
        hotkey_tx: Sender<()>,
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
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let matches = {
                    let Ok(lock) = thread_ids.read() else { break };
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

        if let Ok(mut lock) = self.target_ids.write() {
            *lock = new_ids;
        }
    }
}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        for hk in &self.hotkeys {
            if let Err(e) = self.manager.unregister(*hk) {
                eprintln!("⚠️ Failed to unregister hotkey {hk:?} on drop: {e}");
            }
        }
    }
}
