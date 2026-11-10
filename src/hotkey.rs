use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::mpsc::Sender;
use std::thread;

/// Manages system-wide global hotkey registration and event delivery.
pub struct HotkeyService {
    // Keep manager alive so the OS keyboard hook is not dropped
    _manager: GlobalHotKeyManager,
    _hotkeys: Vec<HotKey>,
}

impl HotkeyService {
    pub fn new(hotkey_tx: Sender<()>, ctx: egui::Context) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = GlobalHotKeyManager::new()?;

        #[cfg(target_os = "macos")]
        let base_mod = Modifiers::META; // Command (⌘) on macOS

        #[cfg(not(target_os = "macos"))]
        let base_mod = Modifiers::CONTROL; // Control on Windows / Linux

        // On French AZERTY keyboards, the 'ù' key corresponds physically to Code::Quote.
        // We register both `Cmd + ù` and `Cmd + Shift + ù` (or Ctrl counterparts) for convenience.
        let hotkey_base = HotKey::new(Some(base_mod), Code::Quote);
        let hotkey_shift = HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Quote);

        let id_base = hotkey_base.id();
        let id_shift = hotkey_shift.id();

        let mut registered_hotkeys = Vec::new();

        if let Err(e) = manager.register(hotkey_base) {
            eprintln!("⚠️ Could not register base hotkey: {e}");
        } else {
            registered_hotkeys.push(hotkey_base);
        }

        if let Err(e) = manager.register(hotkey_shift) {
            eprintln!("⚠️ Could not register shift hotkey: {e}");
        } else {
            registered_hotkeys.push(hotkey_shift);
        }

        let receiver = GlobalHotKeyEvent::receiver();

        // Background thread listens for OS hotkey events and wakes up egui
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                if (event.id == id_base || event.id == id_shift) && event.state == HotKeyState::Pressed {
                    let _ = hotkey_tx.send(());
                    ctx.request_repaint();
                }
            }
        });

        #[cfg(target_os = "macos")]
        println!("⌨️ Registered global shortcut: Cmd + ù (or Cmd + Shift + ù)");

        #[cfg(not(target_os = "macos"))]
        println!("⌨️ Registered global shortcut: Ctrl + ù (or Ctrl + Shift + ù)");

        Ok(Self {
            _manager: manager,
            _hotkeys: registered_hotkeys,
        })
    }
}
