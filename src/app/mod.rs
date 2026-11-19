mod generator_view;
mod history_view;
mod settings_view;

use history_view::ItemAction;

use crate::db::{ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::media;
use crate::platform;
use crate::settings::{AppSettings, HotkeyPreset};
use crate::tray::{TrayAction, TrayService};
use arboard::Clipboard;
use eframe::egui;
use egui_phosphor::regular as icons;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

const MAX_CACHED_TEXTURES: usize = 24;

/// Bound-capped LRU texture cache for clipboard image previews.
/// Drops `egui::TextureHandle` references when capacity is exceeded or items are purged,
/// immediately freeing underlying GPU VRAM.
#[derive(Default)]
pub(crate) struct ImageTextureCache {
    textures: HashMap<i64, egui::TextureHandle>,
    access_order: VecDeque<i64>,
}

impl ImageTextureCache {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get_or_load(
        &mut self,
        id: i64,
        png_bytes: &[u8],
        ctx: &egui::Context,
    ) -> Option<&egui::TextureHandle> {
        if self.textures.contains_key(&id) {
            if let Some(pos) = self.access_order.iter().position(|&x| x == id) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(id);
            return self.textures.get(&id);
        }

        let (w, h, rgba) = media::png_to_rgba(png_bytes).ok()?;
        let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let handle = ctx.load_texture(
            format!("clip_img_{id}"),
            color_image,
            egui::TextureOptions::LINEAR,
        );

        while self.textures.len() >= MAX_CACHED_TEXTURES {
            if let Some(oldest_id) = self.access_order.pop_front() {
                self.textures.remove(&oldest_id);
            } else {
                break;
            }
        }

        self.access_order.push_back(id);
        self.textures.insert(id, handle);
        self.textures.get(&id)
    }

    pub(crate) fn remove(&mut self, id: i64) {
        self.textures.remove(&id);
        if let Some(pos) = self.access_order.iter().position(|&x| x == id) {
            self.access_order.remove(pos);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.textures.clear();
        self.access_order.clear();
    }

    pub(crate) fn retain_active(&mut self, active_ids: &HashSet<i64>) {
        self.textures.retain(|id, _| active_ids.contains(id));
        self.access_order.retain(|id| active_ids.contains(id));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContentFilter {
    #[default]
    All,
    Text,
    Links,
    Code,
    Colors,
    Secrets,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ActiveView {
    #[default]
    History,
    Generator,
    Settings,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClipboardApp {
    db: Database,
    search_query: String,
    content_filter: ContentFilter,
    items: Vec<ClipboardItem>,
    clipboard_writer: Option<Clipboard>,
    toast: Option<(String, Instant)>,
    pending_toast: Option<String>,
    last_query_cached: String,
    notify_rx: Receiver<()>,
    hotkey_rx: Receiver<()>,
    hotkey_service: Option<HotkeyService>,
    tray_rx: Receiver<TrayAction>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    tray_service: Option<TrayService>,
    texture_cache: ImageTextureCache,
    is_visible: bool,
    has_been_focused: bool,
    was_inactive_since_hidden: bool,
    selected_index: usize,
    focus_search_input: bool,
    last_toggle_instant: Option<Instant>,
    settings: AppSettings,
    active_view: ActiveView,
    password_gen: crate::password::PasswordGeneratorState,
    revealed_secrets: HashSet<i64>,
    pending_clipboard_clear: Option<(String, Instant)>,
    show_clear_confirm_dialog: bool,
    scroll_to_selected: bool,
    current_is_dark: Option<bool>,
    title_bar_dark_mode: Option<bool>,
    monitor_shutdown_tx: Sender<()>,
    recording_custom_hotkey: bool,
    pending_custom_hotkey: String,
    is_quitting: bool,
    show_keyring_explanation: bool,
    last_drag_or_move_instant: Option<Instant>,
    last_outer_rect: Option<egui::Rect>,
    last_hide_instant: Option<Instant>,
}

impl ClipboardApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        _cc: &eframe::CreationContext<'_>,
        db: Database,
        notify_rx: Receiver<()>,
        hotkey_rx: Receiver<()>,
        hotkey_service: Option<HotkeyService>,
        tray_rx: Receiver<TrayAction>,
        tray_service: Option<TrayService>,
        settings: AppSettings,
        monitor_shutdown_tx: Sender<()>,
    ) -> Self {
        db.set_max_history(settings.max_history);
        db.set_ignore_concealed(settings.ignore_concealed);
        db.set_auto_detect_secrets(settings.auto_detect_secrets);
        if let Err(e) = db.prune_history(settings.max_history) {
            eprintln!("⚠️ Failed to prune history at startup: {e}");
        }

        let mut startup_toast = None;
        let initial_items = match db.get_recent(settings.max_history, None) {
            Ok(items) => items,
            Err(e) => {
                eprintln!("⚠️ Failed to load clipboard history: {e}");
                startup_toast = Some((
                    format!("{} Failed to load history: {e}", icons::WARNING),
                    Instant::now(),
                ));
                Vec::new()
            }
        };
        let clipboard_writer = match Clipboard::new() {
            Ok(cb) => Some(cb),
            Err(e) => {
                eprintln!("⚠️ Clipboard writer unavailable: {e}");
                None
            }
        };

        let password_gen = crate::password::PasswordGeneratorState::from_preferences(
            settings.password_config.clone(),
            settings.generator_skip_history,
            settings.generator_reveal_value,
        );
        Self {
            db,
            search_query: String::new(),
            content_filter: ContentFilter::All,
            items: initial_items,
            clipboard_writer,
            toast: startup_toast,
            pending_toast: None,
            last_query_cached: String::new(),
            notify_rx,
            hotkey_rx,
            hotkey_service,
            tray_rx,
            tray_service,
            texture_cache: ImageTextureCache::new(),
            is_visible: true,
            has_been_focused: false,
            was_inactive_since_hidden: false,
            selected_index: 0,
            focus_search_input: true,
            last_toggle_instant: None,
            settings,
            active_view: ActiveView::History,
            password_gen,
            revealed_secrets: HashSet::new(),
            pending_clipboard_clear: None,
            show_clear_confirm_dialog: false,
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            recording_custom_hotkey: false,
            pending_custom_hotkey: String::new(),
            monitor_shutdown_tx,
            is_quitting: false,
            show_keyring_explanation: false,
            last_drag_or_move_instant: None,
            last_outer_rect: None,
            last_hide_instant: None,
        }
    }

    fn is_dark(&self, ctx: &egui::Context) -> bool {
        self.settings.theme.resolve(ctx)
    }

    fn refresh_items(&mut self) {
        let filter = if self.search_query.trim().is_empty() {
            None
        } else {
            Some(self.search_query.as_str())
        };
        self.items = self
            .db
            .get_recent(self.settings.max_history, filter)
            .unwrap_or_default();
        let active_ids: HashSet<i64> = self.items.iter().map(|it| it.id).collect();
        self.texture_cache.retain_active(&active_ids);
    }

    fn show_toast(&mut self, message: &str) {
        self.toast = Some((message.to_string(), Instant::now()));
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        crate::platform::log_message("INFO", "show_window called");
        self.is_visible = true;
        self.has_been_focused = false;
        self.was_inactive_since_hidden = false;
        self.selected_index = 0;
        self.show_clear_confirm_dialog = false;
        self.scroll_to_selected = true;
        // Restore at the OS level first (important on Windows, where a minimized
        // window may otherwise stay minimized even after Visible(true)).
        crate::platform::order_front_main_window();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.refresh_items();
        if let Some(msg) = self.pending_toast.take() {
            self.show_toast(&msg);
        }
        self.focus_search_input = true;
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context, restore_focus: bool, auto_paste: bool) {
        self.is_visible = false;
        self.has_been_focused = false;
        self.was_inactive_since_hidden = false;
        self.last_hide_instant = Some(Instant::now());
        self.show_clear_confirm_dialog = false;
        self.revealed_secrets.clear();

        #[cfg(target_os = "windows")]
        {
            if self.tray_service.is_some() {
                // With a system tray icon active, hide the window cleanly so it
                // doesn't clutter the taskbar; clicking the tray icon restores it.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                // Fallback if tray icon failed to initialize: minimize to taskbar
                // so the user is never locked out of returning to the window.
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            crate::platform::hide_application();
        }

        #[allow(unused_mut)]
        let mut auto_paste = auto_paste;
        #[cfg(target_os = "windows")]
        if auto_paste && crate::platform::windows_target_is_elevated() {
            auto_paste = false;
            crate::platform::log_message(
                "WARN",
                "Auto-paste blocked: target app runs as Administrator",
            );
            self.pending_toast = Some(
                "Auto-paste blocked: target app runs as Administrator. Paste manually or run CipherClipboard as Administrator.".to_string(),
            );
        }

        if restore_focus || auto_paste {
            platform::restore_focus_and_paste(
                restore_focus,
                auto_paste,
                self.settings.auto_paste_delay_ms,
            );
        }
    }

    fn schedule_secret_clipboard_clear(&mut self, secret: &str) {
        if self.settings.auto_clear_secret_secs > 0 {
            let deadline =
                Instant::now() + Duration::from_secs(self.settings.auto_clear_secret_secs);
            self.pending_clipboard_clear = Some((secret.to_string(), deadline));
        }
    }

    fn handle_clipboard_auto_clear(&mut self) {
        let Some((_, deadline)) = &self.pending_clipboard_clear else {
            return;
        };
        if Instant::now() < *deadline {
            return;
        }
        let Some((expected_secret, _)) = self.pending_clipboard_clear.take() else {
            return;
        };
        if let Some(cb) = &mut self.clipboard_writer {
            if let Ok(current) = cb.get_text() {
                if current == expected_secret {
                    self.db.skip_next_capture("");
                    let _ = cb.clear();
                }
            }
        }
    }

    fn ensure_clipboard_writer(&mut self) -> Option<&mut Clipboard> {
        if self.clipboard_writer.is_none() {
            match Clipboard::new() {
                Ok(cb) => self.clipboard_writer = Some(cb),
                Err(e) => {
                    eprintln!("⚠️ Failed to initialize clipboard writer: {e}");
                }
            }
        }
        self.clipboard_writer.as_mut()
    }

    fn finish_copy(&mut self, ctx: &egui::Context, toast: &str, dismiss: bool) {
        self.show_toast(toast);
        if dismiss {
            self.hide_window(ctx, self.settings.restore_focus, self.settings.auto_paste);
        }
    }

    fn copy_text_to_clipboard(&mut self, content: &str, ctx: &egui::Context) {
        let Some(cb) = self.ensure_clipboard_writer() else {
            self.show_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(content.to_string()) {
            eprintln!("Failed to write text to clipboard: {e}");
            self.show_toast(&format!(
                "{} Failed to copy text to clipboard",
                icons::WARNING
            ));
        } else {
            let msg = format!("{} Copied text to clipboard!", icons::CHECK);
            self.finish_copy(ctx, &msg, self.settings.hide_on_copy);
        }
    }

    fn copy_secret_to_clipboard(&mut self, secret: &str, ctx: &egui::Context) {
        self.db.skip_next_capture(secret);
        let Some(cb) = self.ensure_clipboard_writer() else {
            self.show_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(secret.to_string()) {
            eprintln!("Failed to write secret to clipboard: {e}");
            self.show_toast(&format!(
                "{} Failed to copy secret to clipboard",
                icons::WARNING
            ));
        } else {
            self.schedule_secret_clipboard_clear(secret);
            let msg = if self.settings.auto_clear_secret_secs > 0 {
                format!(
                    "{} Copied secret (auto-clears in {}s)",
                    icons::LOCK,
                    self.settings.auto_clear_secret_secs
                )
            } else {
                format!("{} Copied secret to clipboard!", icons::LOCK)
            };
            self.finish_copy(ctx, &msg, self.settings.hide_on_copy);
        }
    }

    fn copy_generated_password(&mut self, ctx: &egui::Context, dismiss: bool) {
        let secret = self.password_gen.current_value.clone();
        if secret.is_empty() {
            return;
        }
        // Prevent the background monitor from redundantly re-capturing this copy 200ms later.
        self.db.skip_next_capture(&secret);

        let Some(cb) = self.ensure_clipboard_writer() else {
            self.show_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(secret.clone()) {
            eprintln!("Failed to write generated password to clipboard: {e}");
            self.show_toast(&format!(
                "{} Failed to copy password to clipboard",
                icons::WARNING
            ));
        } else {
            self.schedule_secret_clipboard_clear(&secret);
            if !self.password_gen.skip_history {
                if let Err(e) = self.db.insert_secret(&secret) {
                    eprintln!("Failed to save generated password to database: {e}");
                } else {
                    if let Err(e) = self.db.prune_history(self.settings.max_history) {
                        eprintln!("⚠️ Failed to prune history: {e}");
                    }
                    self.refresh_items();
                }
            }
            let msg = if self.password_gen.skip_history {
                format!("{} Copied password (not saved to history)", icons::CHECK)
            } else {
                format!("{} Copied & saved password as Secret!", icons::LOCK)
            };
            self.finish_copy(ctx, &msg, dismiss);
        }
    }

    fn copy_image_to_clipboard(&mut self, png_bytes: &[u8], ctx: &egui::Context) {
        let Some(cb) = self.ensure_clipboard_writer() else {
            self.show_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        match media::png_to_rgba(png_bytes) {
            Ok((width, height, rgba)) => {
                let img_data = arboard::ImageData {
                    width,
                    height,
                    bytes: std::borrow::Cow::Borrowed(&rgba),
                };
                if let Err(e) = cb.set_image(img_data) {
                    eprintln!("Failed to write image to clipboard: {e}");
                    self.show_toast(&format!(
                        "{} Failed to copy image to clipboard",
                        icons::WARNING
                    ));
                } else {
                    let msg = format!("{} Copied image to clipboard!", icons::CHECK);
                    self.finish_copy(ctx, &msg, self.settings.hide_on_copy);
                }
            }
            Err(e) => {
                eprintln!("Failed to decode PNG image for clipboard copy: {e}");
                self.show_toast(&format!(
                    "{} Failed to decode image for clipboard",
                    icons::WARNING
                ));
            }
        }
    }

    fn handle_hotkey_toggle(&mut self, ctx: &egui::Context) {
        let mut toggle_requested = false;
        while self.hotkey_rx.try_recv().is_ok() {
            toggle_requested = true;
        }

        if !toggle_requested {
            return;
        }

        // Debounce hotkey presses to ignore rapid OS key repeats (200ms threshold)
        if self
            .last_toggle_instant
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(200))
        {
            return;
        }
        self.last_toggle_instant = Some(Instant::now());

        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));

        if self.is_visible && is_focused {
            // Window is already active and focused right in front of user: dismiss it.
            self.hide_window(ctx, self.settings.restore_focus, false);
        } else {
            // Window is hidden, minimized, or in the background behind other apps:
            // ALWAYS summon and focus it immediately on the first press!
            self.show_window(ctx);
        }
    }

    fn handle_escape_key(&mut self, ctx: &egui::Context) {
        if self.recording_custom_hotkey {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.show_clear_confirm_dialog {
                self.show_clear_confirm_dialog = false;
            } else if self.active_view != ActiveView::History {
                self.active_view = ActiveView::History;
            } else {
                self.hide_window(ctx, self.settings.restore_focus, false);
            }
        }
    }

    /// Handles key input during custom hotkey recording mode.
    /// Uses raw events to reliably capture modifier+key combinations.
    #[allow(clippy::too_many_lines)]
    fn handle_custom_hotkey_recording(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            // Process raw key events which include modifier state at time of event
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    repeat: false,
                    ..
                } = event
                {
                    // Cancel on Escape
                    if *key == egui::Key::Escape {
                        self.recording_custom_hotkey = false;
                        self.pending_custom_hotkey.clear();
                        return;
                    }

                    // Clear on Backspace
                    if *key == egui::Key::Backspace {
                        self.pending_custom_hotkey.clear();
                        return;
                    }

                    // Build hotkey string from event's modifiers + key.
                    // Note: in egui, `modifiers.command` aliases `ctrl` on Linux/Windows,
                    // so only treat `mac_cmd` / `command` as Cmd on macOS.
                    let mut parts = Vec::new();
                    if modifiers.ctrl {
                        parts.push("Ctrl");
                    }
                    if modifiers.alt {
                        parts.push("Alt");
                    }
                    if modifiers.shift {
                        parts.push("Shift");
                    }
                    #[cfg(target_os = "macos")]
                    if modifiers.mac_cmd || modifiers.command {
                        parts.push("Cmd");
                    }

                    // Map key to display name
                    let key_name = match key {
                        egui::Key::Space => "Space",
                        egui::Key::Enter => "Enter",
                        egui::Key::Tab => "Tab",
                        egui::Key::Escape => "Esc",
                        egui::Key::Backspace => "Backspace",
                        egui::Key::Delete => "Delete",
                        egui::Key::Home => "Home",
                        egui::Key::End => "End",
                        egui::Key::PageUp => "PageUp",
                        egui::Key::PageDown => "PageDown",
                        egui::Key::ArrowUp => "Up",
                        egui::Key::ArrowDown => "Down",
                        egui::Key::ArrowLeft => "Left",
                        egui::Key::ArrowRight => "Right",
                        egui::Key::Insert => "Insert",
                        egui::Key::F1 => "F1",
                        egui::Key::F2 => "F2",
                        egui::Key::F3 => "F3",
                        egui::Key::F4 => "F4",
                        egui::Key::F5 => "F5",
                        egui::Key::F6 => "F6",
                        egui::Key::F7 => "F7",
                        egui::Key::F8 => "F8",
                        egui::Key::F9 => "F9",
                        egui::Key::F10 => "F10",
                        egui::Key::F11 => "F11",
                        egui::Key::F12 => "F12",
                        egui::Key::F13 => "F13",
                        egui::Key::F14 => "F14",
                        egui::Key::F15 => "F15",
                        egui::Key::F16 => "F16",
                        egui::Key::F17 => "F17",
                        egui::Key::F18 => "F18",
                        egui::Key::F19 => "F19",
                        egui::Key::F20 => "F20",
                        egui::Key::F21 => "F21",
                        egui::Key::F22 => "F22",
                        egui::Key::F23 => "F23",
                        egui::Key::F24 => "F24",
                        egui::Key::A => "A",
                        egui::Key::B => "B",
                        egui::Key::C => "C",
                        egui::Key::D => "D",
                        egui::Key::E => "E",
                        egui::Key::F => "F",
                        egui::Key::G => "G",
                        egui::Key::H => "H",
                        egui::Key::I => "I",
                        egui::Key::J => "J",
                        egui::Key::K => "K",
                        egui::Key::L => "L",
                        egui::Key::M => "M",
                        egui::Key::N => "N",
                        egui::Key::O => "O",
                        egui::Key::P => "P",
                        egui::Key::Q => "Q",
                        egui::Key::R => "R",
                        egui::Key::S => "S",
                        egui::Key::T => "T",
                        egui::Key::U => "U",
                        egui::Key::V => "V",
                        egui::Key::W => "W",
                        egui::Key::X => "X",
                        egui::Key::Y => "Y",
                        egui::Key::Z => "Z",
                        egui::Key::Num0 => "0",
                        egui::Key::Num1 => "1",
                        egui::Key::Num2 => "2",
                        egui::Key::Num3 => "3",
                        egui::Key::Num4 => "4",
                        egui::Key::Num5 => "5",
                        egui::Key::Num6 => "6",
                        egui::Key::Num7 => "7",
                        egui::Key::Num8 => "8",
                        egui::Key::Num9 => "9",
                        egui::Key::Quote => {
                            if crate::platform::windows_quote_key_is_ugrave() {
                                "ù"
                            } else {
                                "Quote"
                            }
                        }
                        egui::Key::Comma => "Comma",
                        egui::Key::Minus => "Minus",
                        egui::Key::Equals | egui::Key::Plus => "Equal",
                        egui::Key::Period => "Period",
                        egui::Key::Slash | egui::Key::Questionmark => "Slash",
                        egui::Key::Semicolon | egui::Key::Colon => "Semicolon",
                        egui::Key::Backslash | egui::Key::Pipe => "Backslash",
                        egui::Key::OpenBracket => "BracketLeft",
                        egui::Key::CloseBracket => "BracketRight",
                        egui::Key::Backtick => "Grave",
                        _ => continue, // Skip unhandled keys (modifiers, etc.)
                    };

                    parts.push(key_name);
                    self.pending_custom_hotkey = parts.join("+");
                    self.apply_custom_hotkey();
                    return;
                }
            }
        });
    }

    #[allow(clippy::too_many_lines)]
    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        // Handle custom hotkey recording
        if self.recording_custom_hotkey {
            self.handle_custom_hotkey_recording(ctx);
            return;
        }

        if self.show_clear_confirm_dialog || !self.is_visible {
            return;
        }

        if self.active_view == ActiveView::Generator {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                self.copy_generated_password(ctx, true);
            }
            return;
        }

        if self.active_view != ActiveView::History {
            return;
        }

        let filtered_indices = self.get_filtered_indices();
        if filtered_indices.is_empty() {
            self.selected_index = 0;
            return;
        }

        if self.selected_index >= filtered_indices.len() {
            self.selected_index = filtered_indices.len() - 1;
        }

        let mut move_down = false;
        let mut move_up = false;
        let mut wrap_down = false;
        let mut wrap_up = false;
        let mut copy_selected = false;
        let mut delete_selected = false;
        let mut copy_shortcut_slot: Option<usize> = None;

        ctx.input_mut(|i| {
            // 1. Direct number shortcuts (Cmd+1..9 on macOS, Ctrl+1..9 on Win/Linux)
            let num_keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            for (idx, &k) in num_keys.iter().enumerate() {
                if i.consume_key(egui::Modifiers::COMMAND, k) {
                    copy_shortcut_slot = Some(idx);
                    return;
                }
            }

            // 2. Down navigation (ArrowDown, Tab without Shift, Ctrl+N, Ctrl+J)
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::N)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::J)
            {
                move_down = true;
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                wrap_down = true;
            }

            // 3. Up navigation (ArrowUp, Shift+Tab, Ctrl+P, Ctrl+K)
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::P)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::K)
            {
                move_up = true;
            } else if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                wrap_up = true;
            }

            // 4. Enter to copy selected
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                copy_selected = true;
            }

            // 5. Delete or Cmd+Backspace / Ctrl+Backspace to delete selected
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Backspace)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::D)
            {
                delete_selected = true;
            }
        });

        if let Some(slot) = copy_shortcut_slot {
            self.selected_index = slot;
            self.copy_filtered_item(&filtered_indices, slot, ctx);
            return;
        }

        if move_down {
            if self.selected_index + 1 < filtered_indices.len() {
                self.selected_index += 1;
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
        } else if wrap_down {
            self.selected_index = if self.selected_index + 1 < filtered_indices.len() {
                self.selected_index + 1
            } else {
                0
            };
            self.scroll_to_selected = true;
            ctx.request_repaint();
        }

        if move_up {
            if self.selected_index > 0 {
                self.selected_index -= 1;
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
        } else if wrap_up {
            self.selected_index = if self.selected_index > 0 {
                self.selected_index - 1
            } else {
                filtered_indices.len().saturating_sub(1)
            };
            self.scroll_to_selected = true;
            ctx.request_repaint();
        }

        if copy_selected {
            self.copy_filtered_item(&filtered_indices, self.selected_index, ctx);
        }

        if delete_selected {
            self.delete_filtered_item(&filtered_indices, ctx);
        }
    }

    fn copy_filtered_item(&mut self, filtered_indices: &[usize], slot: usize, ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(slot) {
            let copy_action = self.items.get(item_idx).and_then(|item| match item.kind {
                ItemKind::Image => item.image_data.clone().map(ItemAction::CopyImage),
                ItemKind::Secret => Some(ItemAction::CopySecret(item.content.clone())),
                _ => Some(ItemAction::CopyText(item.content.clone())),
            });
            match copy_action {
                Some(ItemAction::CopyImage(png)) => self.copy_image_to_clipboard(&png, ctx),
                Some(ItemAction::CopySecret(secret)) => self.copy_secret_to_clipboard(&secret, ctx),
                Some(ItemAction::CopyText(text)) => self.copy_text_to_clipboard(&text, ctx),
                _ => {}
            }
        }
    }

    fn delete_filtered_item(&mut self, filtered_indices: &[usize], ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(self.selected_index) {
            if let Some(item) = self.items.get(item_idx) {
                let id = item.id;
                let _ = self.db.delete(id);
                self.texture_cache.remove(id);
                self.refresh_items();
                let new_count = self.get_filtered_indices().len();
                if self.selected_index >= new_count && new_count > 0 {
                    self.selected_index = new_count - 1;
                }
                self.scroll_to_selected = true;
                self.show_toast(&format!("{} Entry deleted.", icons::TRASH));
                ctx.request_repaint();
            }
        }
    }

    fn handle_app_activation(&mut self, ctx: &egui::Context) {
        let (is_focused, outer_rect, pointer_down) = ctx.input(|i| {
            (
                i.viewport().focused.unwrap_or(false),
                i.viewport().outer_rect,
                i.pointer.any_down(),
            )
        });
        if let Some(rect) = outer_rect {
            if self.last_outer_rect.is_some_and(|prev| prev != rect) {
                self.last_drag_or_move_instant = Some(Instant::now());
            }
            self.last_outer_rect = Some(rect);
        }

        let active_state = crate::platform::is_app_active_state();
        let is_active = active_state.unwrap_or(false);
        let reopen_requested = crate::platform::take_reopen_requested();

        if self.is_visible {
            if is_focused {
                self.has_been_focused = true;
            } else if self.has_been_focused
                && self.settings.hide_on_blur
                && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
            {
                let is_dragging_or_moving = pointer_down
                    || self
                        .last_drag_or_move_instant
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(350));

                if !is_dragging_or_moving {
                    // On macOS, Windows, and X11 Linux, only dismiss when another application
                    // actually becomes active (active_state == Some(false)). Titlebar drags,
                    // StartDrag grabs, and WM move/resize grabs keep our app active.
                    // If the session cannot report active-window ownership (e.g. pure Wayland),
                    // fall back to `!is_focused`.
                    let is_outside = active_state.map_or(!is_focused, |active| !active);

                    if is_outside {
                        self.hide_window(ctx, false, false);
                    }
                }
            }
        } else {
            if !is_active && !is_focused {
                self.was_inactive_since_hidden = true;
            }

            let allow_focus_reopen = self.was_inactive_since_hidden
                && self
                    .last_hide_instant
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(350));

            // Reveal window if OS reopen was requested (Dock icon click),
            // or if the app regained activation/focus after being hidden and inactive.
            if reopen_requested || (allow_focus_reopen && (is_active || is_focused)) {
                self.show_window(ctx);
            }
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.is_quitting {
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx, false, false);
        }
    }

    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        let mut latest_action = None;
        while let Ok(action) = self.tray_rx.try_recv() {
            crate::platform::log_message("INFO", &format!("tray event received: {action:?}"));
            latest_action = Some(action);
        }
        if let Some(action) = latest_action {
            match action {
                TrayAction::Show => {
                    self.show_window(ctx);
                }
                TrayAction::OpenSettings => {
                    self.active_view = ActiveView::Settings;
                    self.show_window(ctx);
                }
                TrayAction::Quit => {
                    self.quit(ctx);
                }
            }
        }
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.is_quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_quit_shortcut(&mut self, ctx: &egui::Context) {
        let quit_pressed = ctx.input(|i| {
            #[cfg(target_os = "macos")]
            let modifier = i.modifiers.command;
            #[cfg(not(target_os = "macos"))]
            let modifier = i.modifiers.ctrl;

            modifier && i.key_pressed(egui::Key::Q)
        });

        if quit_pressed {
            self.quit(ctx);
        }
    }

    fn handle_monitor_updates(&mut self) {
        let mut new_items_detected = false;
        while self.notify_rx.try_recv().is_ok() {
            new_items_detected = true;
        }

        if new_items_detected || self.search_query != self.last_query_cached {
            if new_items_detected {
                let _ = self.db.prune_history(self.settings.max_history);
            }
            self.refresh_items();
            self.last_query_cached = self.search_query.clone();
            self.selected_index = 0;
            self.scroll_to_selected = true;
        }
    }

    fn render_top_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_panel")
            .frame(
                egui::Frame::new()
                    .fill(crate::theme::panel_background(is_dark))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| {
                ui.add_space(crate::theme::top_padding());
                self.render_header_row(ui, &ctx, is_dark);
                ui.add_space(6.0_f32);
                if self.active_view == ActiveView::History {
                    self.render_search_bar(ui, is_dark);
                    ui.add_space(6.0_f32);
                    self.render_filter_tabs(ui, is_dark);
                    ui.add_space(4.0_f32);
                }
            });
    }

    fn render_filter_tabs(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let old_filter = self.content_filter;
        let all_count = self.items.len();
        let text_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Text)
            .count();
        let link_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Url)
            .count();
        let code_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Code)
            .count();
        let color_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Color || !i.embedded_colors().is_empty())
            .count();
        let secret_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Secret)
            .count();
        let img_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Image)
            .count();

        ui.horizontal_wrapped(|ui| {
            let tabs = [
                (ContentFilter::All, icons::STACK, "All", all_count),
                (ContentFilter::Text, icons::FILE_TEXT, "Text", text_count),
                (ContentFilter::Links, icons::LINK, "Links", link_count),
                (ContentFilter::Code, icons::CODE, "Code", code_count),
                (ContentFilter::Colors, icons::PALETTE, "Colors", color_count),
                (ContentFilter::Secrets, icons::LOCK, "Secrets", secret_count),
                (ContentFilter::Image, icons::IMAGE, "Images", img_count),
            ];

            for (target, icon, label, count) in tabs {
                if crate::theme::render_segmented_tab(
                    ui,
                    is_dark,
                    self.content_filter == target,
                    icon,
                    label,
                    count,
                )
                .clicked()
                {
                    self.content_filter = target;
                }
            }
        });

        if self.content_filter != old_filter {
            self.selected_index = 0;
            self.focus_search_input = true;
            self.scroll_to_selected = true;
        }
    }

    fn render_header_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, is_dark: bool) {
        ui.horizontal(|ui| {
            ui.add_space(crate::theme::header_left_padding());

            let title_resp = ui.heading(
                egui::RichText::new(format!("{} Clipboard", icons::LOCK_KEY))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            if title_resp.interact(egui::Sense::drag()).drag_started() {
                self.last_drag_or_move_instant = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            if ui.available_width() > 260.0 {
                let badge = if self.settings.hotkey == HotkeyPreset::Custom
                    && !self.settings.custom_hotkey.is_empty()
                {
                    self.settings.custom_hotkey.as_str()
                } else {
                    self.settings.hotkey.badge()
                };
                ui.label(
                    egui::RichText::new(format!("({badge})"))
                        .color(crate::theme::text_secondary(is_dark))
                        .size(11.0_f32),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::X)
                    .on_hover_text("Hide window (Esc)")
                    .clicked()
                {
                    self.hide_window(ctx, self.settings.restore_focus, false);
                }

                let quit_tooltip = if cfg!(target_os = "macos") {
                    "Quit CipherClipboard (Cmd+Q)"
                } else {
                    "Quit CipherClipboard (Ctrl+Q)"
                };
                if ui
                    .small_button(icons::POWER)
                    .on_hover_text(quit_tooltip)
                    .clicked()
                {
                    self.quit(ctx);
                }

                let settings_btn = match self.active_view {
                    ActiveView::Settings => ui
                        .small_button(format!("{} History", icons::CLOCK_COUNTER_CLOCKWISE))
                        .on_hover_text("Back to clipboard history"),
                    ActiveView::History | ActiveView::Generator => ui
                        .small_button(icons::GEAR)
                        .on_hover_text("Preferences & Settings"),
                };
                if settings_btn.clicked() {
                    self.active_view = match self.active_view {
                        ActiveView::Settings => ActiveView::History,
                        ActiveView::History | ActiveView::Generator => ActiveView::Settings,
                    };
                }

                let gen_btn = match self.active_view {
                    ActiveView::Generator => ui
                        .small_button(format!("{} History", icons::CLOCK_COUNTER_CLOCKWISE))
                        .on_hover_text("Back to clipboard history"),
                    ActiveView::History | ActiveView::Settings => ui
                        .small_button(icons::KEY)
                        .on_hover_text("Generate random password or passphrase"),
                };
                if gen_btn.clicked() {
                    self.active_view = match self.active_view {
                        ActiveView::Generator => ActiveView::History,
                        ActiveView::History | ActiveView::Settings => ActiveView::Generator,
                    };
                }

                if self.active_view == ActiveView::History
                    && ui
                        .small_button(icons::TRASH)
                        .on_hover_text("Clear clipboard history")
                        .clicked()
                {
                    self.show_clear_confirm_dialog = true;
                }
            });
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let is_focused = self.focus_search_input;
        egui::Frame::new()
            .fill(crate::theme::search_bg(is_dark))
            .stroke(crate::theme::search_stroke(is_dark, is_focused))
            .corner_radius(crate::theme::search_rounding())
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icons::MAGNIFYING_GLASS)
                            .color(crate::theme::text_secondary(is_dark))
                            .size(13.0_f32),
                    );
                    let search_box = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text(
                                egui::RichText::new(
                                    "Search clipboard history... (↑/↓ to select, Enter to paste)",
                                )
                                .color(crate::theme::text_secondary(is_dark)),
                            )
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY),
                    );

                    if !self.search_query.is_empty()
                        && ui
                            .small_button(icons::X)
                            .on_hover_text("Clear search")
                            .clicked()
                    {
                        self.search_query.clear();
                    }

                    if self.focus_search_input {
                        search_box.request_focus();
                        self.focus_search_input = false;
                    }
                });
            });
    }

    fn render_bottom_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        egui::Panel::bottom("bottom_panel")
            .frame(
                egui::Frame::new()
                    .fill(crate::theme::panel_background(is_dark))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        crate::theme::bottom_panel_stroke(is_dark),
                    ))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some((msg, time)) = &self.toast {
                        if time.elapsed() < Duration::from_secs(2) {
                            ui.label(
                                egui::RichText::new(msg)
                                    .color(crate::theme::success_color(is_dark))
                                    .strong(),
                            );
                        } else {
                            self.toast = None;
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {} entries • SQLCipher encrypted",
                                icons::SHIELD_CHECK,
                                self.items.len()
                            ))
                            .color(crate::theme::text_secondary(is_dark))
                            .size(11.0_f32),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        #[cfg(target_os = "macos")]
                        let nav_hint = "↑/↓ or Tab • Enter • Cmd+1..9 • Esc";
                        #[cfg(not(target_os = "macos"))]
                        let nav_hint = "↑/↓ or Tab • Enter • Ctrl+1..9 • Esc";
                        ui.label(
                            egui::RichText::new(nav_hint)
                                .color(crate::theme::text_secondary(is_dark))
                                .size(10.5_f32),
                        );
                    });
                });
            });
    }

    fn get_filtered_indices(&self) -> Vec<usize> {
        let filter = self.content_filter;
        self.items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                let matches = match filter {
                    ContentFilter::All => true,
                    ContentFilter::Text => item.kind == ItemKind::Text,
                    ContentFilter::Links => item.kind == ItemKind::Url,
                    ContentFilter::Code => item.kind == ItemKind::Code,
                    ContentFilter::Colors => {
                        item.kind == ItemKind::Color || !item.embedded_colors().is_empty()
                    }
                    ContentFilter::Secrets => item.kind == ItemKind::Secret,
                    ContentFilter::Image => item.kind == ItemKind::Image,
                };
                if matches {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    fn render_clear_confirm_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.show_clear_confirm_dialog {
            return;
        }

        let screen_rect = ctx.content_rect();
        egui::Area::new(egui::Id::new("clear_confirm_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    screen_rect,
                    0.0_f32,
                    egui::Color32::from_black_alpha(150),
                );
                if response.clicked() {
                    self.show_clear_confirm_dialog = false;
                }
            });

        let mut open = self.show_clear_confirm_dialog;
        egui::Window::new(format!("{} Clear History?", icons::WARNING))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0_f32, 0.0_f32))
            .order(egui::Order::Foreground)
            .default_width(300.0_f32)
            .max_width(320.0_f32)
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .corner_radius(crate::theme::card_rounding())
                    .stroke(crate::theme::card_stroke(is_dark, false)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0_f32);
                    ui.label(
                        egui::RichText::new("Clear all clipboard history?")
                            .color(crate::theme::text_primary(is_dark))
                            .strong()
                            .size(15.0_f32),
                    );
                    ui.add_space(8.0_f32);
                    ui.label(
                        egui::RichText::new(
                            "This action cannot be undone. All saved clips, URLs, code snippets, and images will be permanently erased.",
                        )
                        .size(12.0_f32)
                        .color(crate::theme::text_secondary(is_dark)),
                    );
                    ui.add_space(16.0_f32);

                    ui.horizontal(|ui| {
                        let btn_width = 110.0_f32;
                        let btn_height = 28.0_f32;
                        let available_width = ui.available_width();
                        let spacing =
                            btn_width.mul_add(-2.0_f32, available_width).max(0.0_f32) / 2.0_f32;
                        ui.add_space(spacing);

                        if ui
                            .add_sized([btn_width, btn_height], egui::Button::new("Cancel"))
                            .clicked()
                        {
                            self.show_clear_confirm_dialog = false;
                        }

                        let clear_btn = egui::Button::new(
                            egui::RichText::new(format!("{} Clear All", icons::TRASH))
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(220, 53, 69));

                        if ui.add_sized([btn_width, btn_height], clear_btn).clicked() {
                            let _ = self.db.clear();
                            let _ = self.db.vacuum();
                            self.texture_cache.clear();
                            self.refresh_items();
                            self.show_clear_confirm_dialog = false;
                            self.show_toast(&format!("{} History cleared.", icons::CHECK));
                        }
                    });
                    ui.add_space(4.0_f32);
                });
            });

        if !open {
            self.show_clear_confirm_dialog = false;
        }
    }

    fn render_keyring_explanation_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.show_keyring_explanation {
            return;
        }

        let screen_rect = ctx.content_rect();
        egui::Area::new(egui::Id::new("keyring_explanation_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    screen_rect,
                    0.0_f32,
                    egui::Color32::from_black_alpha(150),
                );
                if response.clicked() {
                    // Allow dismissing by clicking the backdrop.
                    self.show_keyring_explanation = false;
                    self.settings.keyring_explanation_shown = true;
                    let _ = self.settings.save(&self.db);
                }
            });

        let mut open = self.show_keyring_explanation;
        egui::Window::new(format!("{} Secure Key Storage", icons::SHIELD_CHECK))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0_f32, 0.0_f32))
            .order(egui::Order::Foreground)
            .default_width(360.0_f32)
            .max_width(400.0_f32)
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .corner_radius(crate::theme::card_rounding())
                    .stroke(crate::theme::card_stroke(is_dark, false)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0_f32);
                    ui.label(
                        egui::RichText::new("CipherClipboard uses your OS keyring")
                            .color(crate::theme::text_primary(is_dark))
                            .strong()
                            .size(15.0_f32),
                    );
                    ui.add_space(8.0_f32);
                    ui.label(
                        egui::RichText::new(
                            "To keep your clipboard history encrypted, CipherClipboard stores a \
                             256-bit encryption key in your operating system's secure credential \
                             store (macOS Keychain, Windows Credential Manager, or Linux Secret \
                             Service). You may see an OS permission prompt the first time this \
                             happens. This prompt will not appear again unless you reinstall or \
                             reset the app.",
                        )
                        .size(12.0_f32)
                        .color(crate::theme::text_secondary(is_dark)),
                    );
                    ui.add_space(16.0_f32);

                    ui.horizontal(|ui| {
                        let btn_width = 120.0_f32;
                        let btn_height = 28.0_f32;
                        let available_width = ui.available_width();
                        let spacing =
                            btn_width.mul_add(-1.0_f32, available_width).max(0.0_f32) / 2.0_f32;
                        ui.add_space(spacing);

                        if ui
                            .add_sized(
                                [btn_width, btn_height],
                                egui::Button::new(format!("{} Got it", icons::CHECK)),
                            )
                            .clicked()
                        {
                            self.show_keyring_explanation = false;
                            self.settings.keyring_explanation_shown = true;
                            let _ = self.settings.save(&self.db);
                        }
                    });
                    ui.add_space(4.0_f32);
                });
            });

        if !open {
            self.show_keyring_explanation = false;
            self.settings.keyring_explanation_shown = true;
            let _ = self.settings.save(&self.db);
        }
    }
}

impl eframe::App for ClipboardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let is_dark = self.is_dark(&ctx);
        if self.current_is_dark != Some(is_dark) {
            crate::theme::setup_theme(&ctx, is_dark);
            self.current_is_dark = Some(is_dark);
        }
        if self.title_bar_dark_mode != Some(is_dark) {
            crate::platform::windows_set_title_bar_dark_mode(is_dark);
            self.title_bar_dark_mode = Some(is_dark);
        }

        self.handle_window_close_request(&ctx);
        self.handle_tray_events(&ctx);
        self.handle_quit_shortcut(&ctx);
        self.handle_hotkey_toggle(&ctx);
        self.handle_escape_key(&ctx);
        self.handle_keyboard_navigation(&ctx);
        self.handle_app_activation(&ctx);
        self.handle_monitor_updates();
        self.handle_clipboard_auto_clear();

        // Show keyring explanation dialog on first run (informational)
        if !self.settings.keyring_explanation_shown && !self.show_keyring_explanation {
            self.show_keyring_explanation = true;
        }

        if !self.is_visible {
            // Keep the event loop alive with a gentle 50ms tick while hidden.
            // When a window is set to Visible(false), OS compositors (Windows DWM,
            // macOS WindowServer, X11) suppress redraw events, which can cause
            // request_repaint() from the background hotkey thread to stall.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }

        // Keep polling tray/hotkey events while minimized, since request_repaint()
        // from background threads may not wake a minimized window on Windows.
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        if self.pending_clipboard_clear.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        self.render_top_panel(ui, is_dark);
        self.render_bottom_panel(ui, is_dark);
        match self.active_view {
            ActiveView::History => self.render_central_panel(ui, is_dark),
            ActiveView::Generator => self.render_generator_panel(ui, is_dark),
            ActiveView::Settings => self.render_settings_panel(ui, is_dark),
        }

        self.render_clear_confirm_modal(&ctx, is_dark);
        self.render_keyring_explanation_modal(&ctx, is_dark);
    }
}

impl Drop for ClipboardApp {
    fn drop(&mut self) {
        if let Some(hotkey) = self.hotkey_service.as_mut() {
            hotkey.shutdown();
        }
        let _ = self.monitor_shutdown_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::ClipboardApp;
    use eframe::egui;

    #[test]
    fn test_format_display_preview_strips_variation_selectors_and_truncates() {
        let raw = "⚠️ Failed to save";
        assert_eq!(
            ClipboardApp::format_display_preview(raw, 180),
            "⚠ Failed to save"
        );

        let long = "a".repeat(200);
        let truncated = ClipboardApp::format_display_preview(&long, 180);
        assert_eq!(truncated.chars().count(), 183);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_quit_sets_is_quitting_flag() {
        let ctx = egui::Context::default();
        let (shutdown_tx, _shutdown_rx) = std::sync::mpsc::channel();
        let (_notify_tx, notify_rx) = std::sync::mpsc::channel();
        let (_hotkey_tx, hotkey_rx) = std::sync::mpsc::channel();
        let (tray_tx, tray_rx) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("quit_test.db");
        let db = crate::db::Database::open(
            &db_path,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        let mut app = ClipboardApp {
            db,
            search_query: String::new(),
            content_filter: super::ContentFilter::All,
            items: Vec::new(),
            clipboard_writer: None,
            toast: None,
            pending_toast: None,
            last_query_cached: String::new(),
            notify_rx,
            hotkey_rx,
            hotkey_service: None,
            tray_rx,
            tray_service: None,
            texture_cache: super::ImageTextureCache::new(),
            is_visible: true,
            has_been_focused: false,
            was_inactive_since_hidden: false,
            selected_index: 0,
            focus_search_input: true,
            last_toggle_instant: None,
            settings: crate::settings::AppSettings::default(),
            active_view: super::ActiveView::History,
            password_gen: crate::password::PasswordGeneratorState::default(),
            revealed_secrets: std::collections::HashSet::new(),
            pending_clipboard_clear: None,
            show_clear_confirm_dialog: false,
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            recording_custom_hotkey: false,
            pending_custom_hotkey: String::new(),
            monitor_shutdown_tx: shutdown_tx,
            is_quitting: false,
            show_keyring_explanation: false,
            last_drag_or_move_instant: None,
            last_outer_rect: None,
            last_hide_instant: None,
        };

        assert!(!app.is_quitting);

        // Verify TrayAction::Show restores visibility and surfaces any pending toast
        app.is_visible = false;
        app.pending_toast = Some("Auto-paste blocked".to_string());
        tray_tx.send(super::TrayAction::Show).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.is_visible);
        assert!(app.pending_toast.is_none());
        assert_eq!(
            app.toast.as_ref().map(|(msg, _)| msg.as_str()),
            Some("Auto-paste blocked")
        );

        // Verify TrayAction::OpenSettings switches view and shows window
        app.is_visible = false;
        tray_tx.send(super::TrayAction::OpenSettings).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.is_visible);
        assert_eq!(app.active_view, super::ActiveView::Settings);

        // Verify TrayAction::Quit sets is_quitting
        tray_tx.send(super::TrayAction::Quit).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.is_quitting);
    }
}
