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
    #[cfg(target_os = "linux")]
    quote_fallback: linux_x11_fallback::QuoteFallback,
}

fn register_hotkey_batch(
    manager: &GlobalHotKeyManager,
    hotkeys: &[HotKey],
    #[cfg(target_os = "linux")] quote_fallback: &linux_x11_fallback::QuoteFallback,
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

        #[cfg(target_os = "linux")]
        let quote_fallback = linux_x11_fallback::QuoteFallback::new(hotkey_tx.clone(), ctx.clone());

        let (registered, ids, results) = register_hotkey_batch(
            &manager,
            initial_hotkeys,
            #[cfg(target_os = "linux")]
            &quote_fallback,
        );

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

        let (registered, new_ids, results) = register_hotkey_batch(
            &self.manager,
            new_hotkeys,
            #[cfg(target_os = "linux")]
            &self.quote_fallback,
        );
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
    use std::sync::mpsc::{self, Sender, SyncSender};
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
        handle: Option<thread::JoinHandle<()>>,
    }

    impl QuoteFallback {
        pub(super) fn new(hotkey_tx: SyncSender<()>, ctx: egui::Context) -> Self {
            let (tx, rx) = mpsc::channel::<Msg>();
            let handle = thread::Builder::new()
                .name("x11-quote-hotkey".to_string())
                .spawn(move || run_loop(&rx, &hotkey_tx, &ctx))
                .ok();
            Self { tx, handle }
        }

        pub(super) fn register(&self, hk: HotKey) -> Result<(), ()> {
            let (reply_tx, reply_rx) = mpsc::channel();
            self.tx.send(Msg::Register(hk, reply_tx)).map_err(|_| ())?;
            reply_rx.recv().map_err(|_| ())?
        }

        pub(super) fn clear(&self) {
            let _ = self.tx.send(Msg::Clear);
        }

        pub(super) fn shutdown(&mut self) {
            let _ = self.tx.send(Msg::Shutdown);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
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

    fn drain_until_shutdown(rx: &mpsc::Receiver<Msg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Register(_, reply) => {
                    let _ = reply.send(Err(()));
                }
                Msg::Clear => {}
                Msg::Shutdown => break,
            }
        }
    }

    fn poll_x11_key_events(
        conn: &x11rb::rust_connection::RustConnection,
        grabs: &[(u8, ModMask)],
        full_mask: KeyButMask,
        hotkey_tx: &SyncSender<()>,
        ctx: &egui::Context,
    ) {
        while let Ok(Some(event)) = conn.poll_for_event() {
            if let Event::KeyPress(ev) = event {
                let ev_mods = ModMask::from((ev.state & full_mask).bits());
                if grabs
                    .iter()
                    .any(|&(kc, mods)| kc == ev.detail && mods == ev_mods)
                {
                    crate::platform::capture_active_window();
                    // Best-effort coalesced wakeup; a full or closed channel needs no action.
                    let _ = hotkey_tx.try_send(());
                    ctx.request_repaint();
                }
            }
        }
    }

    fn grab_quote_key(
        conn: &x11rb::rust_connection::RustConnection,
        root: u32,
        grabs: &mut Vec<(u8, ModMask)>,
        hk: HotKey,
    ) -> Result<(), ()> {
        let kc = find_quote_keycode(conn).ok_or(())?;
        let mods = mods_to_x11(hk.mods);
        let ok = ignored_mods().into_iter().all(|m| {
            conn.grab_key(false, root, mods | m, kc, GrabMode::ASYNC, GrabMode::ASYNC)
                .is_ok()
        });
        let _ = conn.flush();
        if ok {
            grabs.push((kc, mods));
            Ok(())
        } else {
            Err(())
        }
    }

    fn handle_x11_msg(
        conn: &x11rb::rust_connection::RustConnection,
        root: u32,
        grabs: &mut Vec<(u8, ModMask)>,
        msg: Msg,
    ) -> bool {
        match msg {
            Msg::Register(hk, reply) => {
                let _ = reply.send(grab_quote_key(conn, root, grabs, hk));
                true
            }
            Msg::Clear => {
                ungrab_all(conn, root, grabs);
                true
            }
            Msg::Shutdown => {
                ungrab_all(conn, root, grabs);
                false
            }
        }
    }

    fn run_loop(rx: &mpsc::Receiver<Msg>, hotkey_tx: &SyncSender<()>, ctx: &egui::Context) {
        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            // Still drain messages until shutdown so callers don't block
            drain_until_shutdown(rx);
            return;
        };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
            return;
        };

        let mut grabs: Vec<(u8, ModMask)> = Vec::new();
        let full_mask =
            KeyButMask::CONTROL | KeyButMask::SHIFT | KeyButMask::MOD4 | KeyButMask::MOD1;

        loop {
            if !grabs.is_empty() {
                poll_x11_key_events(&conn, &grabs, full_mask, hotkey_tx, ctx);
            }

            let next_msg = if grabs.is_empty() {
                // Zero idle wakeups while no fallback Quote hotkey is registered.
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            } else {
                rx.recv_timeout(Duration::from_millis(50))
            };

            match next_msg {
                Ok(msg) => {
                    if !handle_x11_msg(&conn, root, &mut grabs, msg) {
                        return;
                    }
                    loop {
                        match rx.try_recv() {
                            Ok(msg) => {
                                if !handle_x11_msg(&conn, root, &mut grabs, msg) {
                                    return;
                                }
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => {
                                ungrab_all(&conn, root, &mut grabs);
                                return;
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    ungrab_all(&conn, root, &mut grabs);
                    return;
                }
            }
        }
    }
}
