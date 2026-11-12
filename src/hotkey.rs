use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::mpsc::Sender;
use std::thread;

/// Manages system-wide global hotkey registration and event delivery.
pub(crate) struct HotkeyService {
    // Keep manager alive so the OS keyboard hook is not dropped
    _manager: GlobalHotKeyManager,
    _hotkeys: Vec<HotKey>,
}

impl HotkeyService {
    pub(crate) fn new(
        hotkey_tx: Sender<()>,
        ctx: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;

        #[cfg(target_os = "macos")]
        let base_mod = Modifiers::META; // Command (⌘) on macOS

        #[cfg(not(target_os = "macos"))]
        let base_mod = Modifiers::CONTROL; // Control on Windows / Linux

        // On French AZERTY keyboards, the 'ù' key corresponds physically to Code::Quote.
        // We register both `Cmd + ù` and `Cmd + Shift + ù` (or Ctrl counterparts) for convenience.
        let hotkey_base = HotKey::new(Some(base_mod), Code::Quote);
        let hotkey_shift = HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Quote);

        // Also register the universal clipboard shortcut: Cmd+Shift+V (macOS) / Ctrl+Shift+V (Win/Linux)
        let hotkey_v = HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::KeyV);

        let target_ids = [hotkey_base.id(), hotkey_shift.id(), hotkey_v.id()];

        let mut registered_hotkeys = Vec::new();

        for hk in [hotkey_base, hotkey_shift, hotkey_v] {
            if let Err(e) = manager.register(hk) {
                eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
            } else {
                registered_hotkeys.push(hk);
            }
        }

        let receiver = GlobalHotKeyEvent::receiver();

        // Background thread listens for OS hotkey events and wakes up egui
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                if target_ids.contains(&event.id) && event.state == HotKeyState::Pressed {
                    let _ = hotkey_tx.send(());
                    ctx.request_repaint();
                }
            }
        });

        #[cfg(target_os = "macos")]
        println!("⌨️ Registered global shortcuts: Cmd + ù and Cmd + Shift + V");

        #[cfg(not(target_os = "macos"))]
        println!("⌨️ Registered global shortcuts: Ctrl + ù and Ctrl + Shift + V");

        Ok(Self {
            _manager: manager,
            _hotkeys: registered_hotkeys,
        })
    }
}
