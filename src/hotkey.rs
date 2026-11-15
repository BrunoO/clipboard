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
    shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
    last_registration_results: Arc<RwLock<Vec<HotkeyRegistrationResult>>>,
    #[cfg(target_os = "linux")]
    quote_fallback: linux_x11_fallback::QuoteFallback,
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
        let mut results = Vec::new();

        #[cfg(target_os = "linux")]
        let quote_fallback = linux_x11_fallback::QuoteFallback::new(hotkey_tx.clone(), ctx.clone());

        for hk in initial_hotkeys {
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
                    #[cfg(target_os = "linux")]
                    let fallback_ok = if hk.key == global_hotkey::hotkey::Code::Quote {
                        quote_fallback.register(hk).is_ok()
                    } else {
                        false
                    };
                    #[cfg(not(target_os = "linux"))]
                    let fallback_ok = false;

                    if fallback_ok {
                        results.push(HotkeyRegistrationResult {
                            hotkey: hk,
                            success: true,
                            error: None,
                        });
                    } else {
                        eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
                        results.push(HotkeyRegistrationResult {
                            hotkey: hk,
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }

        let target_ids = Arc::new(RwLock::new(ids));
        let thread_ids = Arc::clone(&target_ids);
        let receiver = GlobalHotKeyEvent::receiver();
        let results_arc = Arc::new(RwLock::new(results));

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
            last_registration_results: results_arc,
            #[cfg(target_os = "linux")]
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

        #[cfg(target_os = "linux")]
        self.quote_fallback.clear();

        let mut new_ids = Vec::new();
        let mut results = Vec::new();

        for &hk in new_hotkeys {
            match self.manager.register(hk) {
                Ok(()) => {
                    new_ids.push(hk.id());
                    self.hotkeys.push(hk);
                    results.push(HotkeyRegistrationResult {
                        hotkey: hk,
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    #[cfg(target_os = "linux")]
                    let fallback_ok = if hk.key == global_hotkey::hotkey::Code::Quote {
                        self.quote_fallback.register(hk).is_ok()
                    } else {
                        false
                    };
                    #[cfg(not(target_os = "linux"))]
                    let fallback_ok = false;

                    if fallback_ok {
                        results.push(HotkeyRegistrationResult {
                            hotkey: hk,
                            success: true,
                            error: None,
                        });
                    } else {
                        eprintln!("⚠️ Could not register hotkey {hk:?}: {e}");
                        results.push(HotkeyRegistrationResult {
                            hotkey: hk,
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }

        let mut lock = self.target_ids.write().unwrap_or_else(|p| {
            eprintln!("⚠️ Hotkey target_ids mutex recovered from poison");
            p.into_inner()
        });
        *lock = new_ids;

        // Store results for UI
        if let Ok(mut results_lock) = self.last_registration_results.write() {
            (*results_lock).clone_from(&results);
        }

        results
    }

    /// Returns the last registration results for UI display.
    pub(crate) fn registration_results(&self) -> Vec<HotkeyRegistrationResult> {
        self.last_registration_results
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Signals the background hotkey listener thread to shut down.
    pub(crate) fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        #[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
mod linux_x11_fallback {
    use eframe::egui;
    use global_hotkey::hotkey::{HotKey, Modifiers};
    use std::sync::mpsc::{self, Sender};
    use std::thread;
    use std::time::Duration;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode, KeyButMask, ModMask, Window};
    use x11rb::protocol::Event;

    enum Msg {
        Register(HotKey, Sender<Result<(), ()>>),
        Clear,
        Shutdown,
    }

    pub(super) struct QuoteFallback {
        tx: Sender<Msg>,
    }

    impl QuoteFallback {
        pub(super) fn new(hotkey_tx: Sender<()>, ctx: egui::Context) -> Self {
            let (tx, rx) = mpsc::channel::<Msg>();
            thread::spawn(move || run_loop(&rx, &hotkey_tx, &ctx));
            Self { tx }
        }

        pub(super) fn register(&self, hk: HotKey) -> Result<(), ()> {
            let (reply_tx, reply_rx) = mpsc::channel();
            self.tx.send(Msg::Register(hk, reply_tx)).map_err(|_| ())?;
            reply_rx.recv().map_err(|_| ())?
        }

        pub(super) fn clear(&self) {
            let _ = self.tx.send(Msg::Clear);
        }

        pub(super) fn shutdown(&self) {
            let _ = self.tx.send(Msg::Shutdown);
        }
    }

    fn mods_to_x11(mods: Modifiers) -> ModMask {
        let mut mask = ModMask::default();
        if mods.contains(Modifiers::CONTROL) {
            mask |= ModMask::CONTROL;
        }
        if mods.contains(Modifiers::SHIFT) {
            mask |= ModMask::SHIFT;
        }
        if mods.contains(Modifiers::ALT) {
            mask |= ModMask::M1;
        }
        if mods.contains(Modifiers::META) {
            mask |= ModMask::M4;
        }
        mask
    }

    fn ignored_mods() -> [ModMask; 4] {
        [
            ModMask::default(),
            ModMask::M2,
            ModMask::LOCK,
            ModMask::M2 | ModMask::LOCK,
        ]
    }

    fn find_quote_keycode<C: Connection>(conn: &C) -> Option<u8> {
        const XK_UGRAVE: u32 = 0x00f9;
        const XK_APOSTROPHE: u32 = 0x0027;
        let min_kc = conn.setup().min_keycode;
        let max_kc = conn.setup().max_keycode;
        let count = max_kc.checked_sub(min_kc)?.saturating_add(1);
        let mapping = conn
            .get_keyboard_mapping(min_kc, count)
            .ok()?
            .reply()
            .ok()?;
        let per_kc = usize::from(mapping.keysyms_per_keycode);
        if per_kc == 0 {
            return None;
        }
        for target in [XK_UGRAVE, XK_APOSTROPHE] {
            for (idx, chunk) in mapping.keysyms.chunks(per_kc).enumerate() {
                if chunk.contains(&target) {
                    if let Ok(offset) = u8::try_from(idx) {
                        if let Some(kc) = min_kc.checked_add(offset) {
                            return Some(kc);
                        }
                    }
                }
            }
        }
        None
    }

    fn ungrab_all<C: Connection>(conn: &C, root: Window, grabs: &mut Vec<(u8, ModMask)>) {
        for (kc, mods) in grabs.drain(..) {
            for m in ignored_mods() {
                let _ = conn.ungrab_key(kc, root, mods | m);
            }
        }
        let _ = conn.flush();
    }

    fn run_loop(rx: &mpsc::Receiver<Msg>, hotkey_tx: &Sender<()>, ctx: &egui::Context) {
        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            // Still drain messages until shutdown so callers don't block
            while let Ok(msg) = rx.recv() {
                match msg {
                    Msg::Register(_, reply) => {
                        let _ = reply.send(Err(()));
                    }
                    Msg::Clear => {}
                    Msg::Shutdown => break,
                }
            }
            return;
        };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
            return;
        };

        let mut grabs: Vec<(u8, ModMask)> = Vec::new();
        let full_mask =
            KeyButMask::CONTROL | KeyButMask::SHIFT | KeyButMask::MOD4 | KeyButMask::MOD1;

        loop {
            while let Ok(Some(event)) = conn.poll_for_event() {
                if let Event::KeyPress(ev) = event {
                    let ev_mods = ModMask::from((ev.state & full_mask).bits());
                    if grabs
                        .iter()
                        .any(|&(kc, mods)| kc == ev.detail && mods == ev_mods)
                    {
                        crate::platform::capture_active_window();
                        let _ = hotkey_tx.send(());
                        ctx.request_repaint();
                    }
                }
            }

            while let Ok(msg) = rx.try_recv() {
                match msg {
                    Msg::Register(hk, reply) => {
                        if let Some(kc) = find_quote_keycode(&conn) {
                            let mods = mods_to_x11(hk.mods);
                            let mut ok = true;
                            for m in ignored_mods() {
                                if conn
                                    .grab_key(
                                        false,
                                        root,
                                        mods | m,
                                        kc,
                                        GrabMode::ASYNC,
                                        GrabMode::ASYNC,
                                    )
                                    .is_err()
                                {
                                    ok = false;
                                }
                            }
                            let _ = conn.flush();
                            if ok {
                                grabs.push((kc, mods));
                                let _ = reply.send(Ok(()));
                            } else {
                                let _ = reply.send(Err(()));
                            }
                        } else {
                            let _ = reply.send(Err(()));
                        }
                    }
                    Msg::Clear => {
                        ungrab_all(&conn, root, &mut grabs);
                    }
                    Msg::Shutdown => {
                        ungrab_all(&conn, root, &mut grabs);
                        return;
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }
    }
}
