mod generator_view;
mod history_view;
mod settings_view;

use history_view::ItemAction;

use crate::db::{ClearError, ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::icons;
use crate::media;
use crate::platform;
use crate::settings::{AppSettings, HotkeyPreset};
use crate::tray::{TrayAction, TrayService};
use arboard::Clipboard;
use eframe::egui;
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

#[derive(Debug, Clone, Copy, Default)]
struct FilterCounts {
    all: usize,
    text: usize,
    links: usize,
    code: usize,
    colors: usize,
    secrets: usize,
    images: usize,
}

impl FilterCounts {
    fn from_items(items: &[ClipboardItem]) -> Self {
        let mut counts = Self {
            all: items.len(),
            ..Self::default()
        };
        for item in items {
            match item.kind {
                ItemKind::Text => counts.text += 1,
                ItemKind::Url => counts.links += 1,
                ItemKind::Code => counts.code += 1,
                ItemKind::Color => counts.colors += 1,
                ItemKind::Secret => counts.secrets += 1,
                ItemKind::Image => counts.images += 1,
            }
            if item.kind != ItemKind::Color && !item.embedded_colors().is_empty() {
                counts.colors += 1;
            }
        }
        counts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastSeverity {
    Success,
    Warning,
    Error,
}

struct Toast {
    message: String,
    severity: ToastSeverity,
    shown_at: Instant,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClipboardApp {
    db: Database,
    key_storage_mode: crate::crypto::KeyStorageMode,
    search_query: String,
    content_filter: ContentFilter,
    items: Vec<ClipboardItem>,
    filter_counts: FilterCounts,
    clipboard_writer: Option<Clipboard>,
    toast: Option<Toast>,
    pending_toast: Option<(String, ToastSeverity)>,
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
    pending_clipboard_clear: Option<(zeroize::Zeroizing<String>, Instant)>,
    show_clear_confirm_dialog: bool,
    scroll_to_selected: bool,
    current_is_dark: Option<bool>,
    title_bar_dark_mode: Option<bool>,
    monitor_shutdown_tx: Sender<()>,
    monitor_handle: Option<std::thread::JoinHandle<()>>,
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
        key_storage_mode: crate::crypto::KeyStorageMode,
        notify_rx: Receiver<()>,
        hotkey_rx: Receiver<()>,
        hotkey_service: Option<HotkeyService>,
        tray_rx: Receiver<TrayAction>,
        tray_service: Option<TrayService>,
        settings: AppSettings,
        monitor_shutdown_tx: Sender<()>,
        monitor_handle: Option<std::thread::JoinHandle<()>>,
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
                startup_toast = Some(Toast {
                    message: format!("{} Failed to load history: {e}", icons::WARNING),
                    severity: ToastSeverity::Error,
                    shown_at: Instant::now(),
                });
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
        let filter_counts = FilterCounts::from_items(&initial_items);
        Self {
            db,
            key_storage_mode,
            search_query: String::new(),
            content_filter: ContentFilter::All,
            items: initial_items,
            filter_counts,
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
            monitor_handle,
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
        self.filter_counts = FilterCounts::from_items(&self.items);
        let active_ids: HashSet<i64> = self.items.iter().map(|it| it.id).collect();
        self.texture_cache.retain_active(&active_ids);
    }

    fn show_toast(&mut self, message: &str) {
        self.show_toast_severity(message, ToastSeverity::Success);
    }

    fn show_warning_toast(&mut self, message: &str) {
        self.show_toast_severity(message, ToastSeverity::Warning);
    }

    fn show_error_toast(&mut self, message: &str) {
        self.show_toast_severity(message, ToastSeverity::Error);
    }

    fn report_db_write_failure(&mut self, operation: &str, error: &dyn std::error::Error) {
        crate::logging::log_message("ERROR", &format!("Failed to {operation}: {error}"));
        self.show_error_toast(&format!(
            "{} Failed to {operation}. Nothing was changed.",
            icons::WARNING
        ));
    }

    fn persist_settings(&mut self) {
        if let Err(error) = self.settings.save(&self.db) {
            crate::logging::log_message("ERROR", &format!("Failed to save settings: {error}"));
            self.show_error_toast(&format!(
                "{} Failed to save settings. Changes will be lost on restart.",
                icons::WARNING
            ));
        }
    }

    fn show_toast_severity(&mut self, message: &str, severity: ToastSeverity) {
        self.toast = Some(Toast {
            message: message.to_string(),
            severity,
            shown_at: Instant::now(),
        });
    }

    const fn keyring_explanation_copy(
        mode: crate::crypto::KeyStorageMode,
    ) -> (&'static str, &'static str, &'static str, &'static str) {
        match mode {
            crate::crypto::KeyStorageMode::OsKeyring => (
                icons::SHIELD_CHECK,
                "Secure Key Storage",
                "CipherClipboard uses your OS keyring",
                "To keep your clipboard history encrypted, CipherClipboard stores a 256-bit \
                 encryption key in your operating system's secure credential store (macOS \
                 Keychain, Windows Credential Manager, or Linux Secret Service). You may see an \
                 OS permission prompt the first time this happens. This prompt will not appear \
                 again unless you reinstall or reset the app.",
            ),
            crate::crypto::KeyStorageMode::LocalFileFallback => (
                icons::WARNING,
                "Local Key File Fallback",
                "CipherClipboard is using a local key file",
                "The OS keyring was unavailable, so CipherClipboard generated a restricted local \
                 .encryption_key file in the app data directory. Your history stays encrypted, \
                 but this is a degraded state. Unlock the keyring and restart if you want the key \
                 migrated. This prompt will not appear again unless you reinstall or reset the \
                 app.",
            ),
            crate::crypto::KeyStorageMode::LocalFileFallbackDegraded => (
                icons::WARNING,
                "Local Key File Permissions Warning",
                "Fallback key file permissions could not be restricted",
                "The OS keyring was unavailable, and CipherClipboard could not restrict the \
                 fallback .encryption_key file (Windows ACL hardening was skipped or icacls \
                 failed). Your history stays encrypted, but the key file may be readable more \
                 broadly than intended. Fix the account/ACL error, or unlock the keyring and \
                 restart to migrate the key. This prompt will not appear again unless you \
                 reinstall or reset the app.",
            ),
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        crate::logging::log_message("INFO", "show_window called");
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
        if let Some((msg, severity)) = self.pending_toast.take() {
            self.show_toast_severity(&msg, severity);
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
            if self.tray_service.as_ref().is_some_and(TrayService::is_live) {
                // With a confirmed shell tray icon, hide the window cleanly so
                // it doesn't clutter the taskbar; clicking the tray restores it.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                // No live tray icon (init failed, Explorer restarted, ...):
                // minimize to the taskbar so the user is never locked out.
                crate::logging::log_message(
                    "WARN",
                    "No live tray icon; minimizing to taskbar instead of hiding",
                );
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
        if auto_paste && crate::platform::windows::target_is_elevated() {
            auto_paste = false;
            crate::logging::log_message(
                "WARN",
                "Auto-paste blocked: target app runs as Administrator",
            );
            self.pending_toast = Some((
                "Auto-paste blocked: target app runs as Administrator. Paste manually (Ctrl+V)."
                    .to_string(),
                ToastSeverity::Warning,
            ));
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
            self.pending_clipboard_clear =
                Some((zeroize::Zeroizing::new(secret.to_string()), deadline));
        }
    }

    fn clear_pending_secret_from_clipboard(&mut self) {
        let Some((expected_secret, _)) = self.pending_clipboard_clear.take() else {
            return;
        };
        if let Some(cb) = &mut self.clipboard_writer {
            if let Ok(current) = cb.get_text() {
                if current == *expected_secret {
                    let _ = cb.clear();
                }
            }
        }
    }

    fn handle_clipboard_auto_clear(&mut self) {
        let Some((_, deadline)) = &self.pending_clipboard_clear else {
            return;
        };
        if Instant::now() < *deadline {
            return;
        }
        self.clear_pending_secret_from_clipboard();
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
            self.show_warning_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(content.to_string()) {
            eprintln!("Failed to write text to clipboard: {e}");
            self.show_error_toast(&format!(
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
            self.db.clear_skip_capture();
            self.show_warning_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(secret.to_string()) {
            self.db.clear_skip_capture();
            eprintln!("Failed to write secret to clipboard: {e}");
            self.show_error_toast(&format!(
                "{} Failed to copy secret to clipboard",
                icons::WARNING
            ));
        } else {
            crate::platform::mark_clipboard_concealed();
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
            self.db.clear_skip_capture();
            self.show_warning_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(secret.as_str()) {
            self.db.clear_skip_capture();
            eprintln!("Failed to write generated password to clipboard: {e}");
            self.show_error_toast(&format!(
                "{} Failed to copy password to clipboard",
                icons::WARNING
            ));
        } else {
            crate::platform::mark_clipboard_concealed();
            self.schedule_secret_clipboard_clear(&secret);
            if !self.password_gen.skip_history {
                if let Err(e) = self.db.insert_secret(&secret) {
                    eprintln!("Failed to save generated password to database: {e}");
                } else {
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
            self.show_warning_toast(&format!("{} Clipboard unavailable", icons::WARNING));
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
                    self.show_error_toast(&format!(
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
                self.show_error_toast(&format!(
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
    fn handle_custom_hotkey_recording(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for event in &i.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    repeat: false,
                    ..
                } = event
                else {
                    continue;
                };

                if *key == egui::Key::Escape {
                    self.recording_custom_hotkey = false;
                    self.pending_custom_hotkey.clear();
                    return;
                }

                if *key == egui::Key::Backspace {
                    self.pending_custom_hotkey.clear();
                    return;
                }

                if let Some(combo) = format_recorded_hotkey(*modifiers, *key) {
                    self.pending_custom_hotkey = combo;
                    self.apply_custom_hotkey();
                    return;
                }
            }
        });
    }

    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
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

        let nav = read_history_nav_input(ctx);

        if let Some(slot) = nav.copy_shortcut_slot {
            self.selected_index = slot;
            self.copy_filtered_item(&filtered_indices, slot, ctx);
            return;
        }

        self.apply_history_nav_step(nav, filtered_indices.len(), ctx);

        if nav.copy_selected {
            self.copy_filtered_item(&filtered_indices, self.selected_index, ctx);
        }

        if nav.delete_selected {
            self.delete_filtered_item(&filtered_indices, ctx);
        }
    }

    fn apply_history_nav_step(&mut self, nav: HistoryNavInput, len: usize, ctx: &egui::Context) {
        match nav.down {
            Some(NavStep::Clamp) if self.selected_index + 1 < len => {
                self.selected_index += 1;
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
            Some(NavStep::Wrap) => {
                self.selected_index = (self.selected_index + 1) % len;
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
            _ => {}
        }

        match nav.up {
            Some(NavStep::Clamp) if self.selected_index > 0 => {
                self.selected_index -= 1;
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
            Some(NavStep::Wrap) => {
                self.selected_index = if self.selected_index > 0 {
                    self.selected_index - 1
                } else {
                    len.saturating_sub(1)
                };
                self.scroll_to_selected = true;
                ctx.request_repaint();
            }
            _ => {}
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
                if let Err(error) = self.db.delete(id) {
                    self.report_db_write_failure("delete entry", &error);
                    return;
                }
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
            crate::logging::log_message("INFO", &format!("tray event received: {action:?}"));
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
        let counts = self.filter_counts;

        ui.horizontal_wrapped(|ui| {
            let tabs = [
                (ContentFilter::All, icons::STACK, "All", counts.all),
                (ContentFilter::Text, icons::FILE_TEXT, "Text", counts.text),
                (ContentFilter::Links, icons::LINK, "Links", counts.links),
                (ContentFilter::Code, icons::CODE, "Code", counts.code),
                (
                    ContentFilter::Colors,
                    icons::PALETTE,
                    "Colors",
                    counts.colors,
                ),
                (
                    ContentFilter::Secrets,
                    icons::LOCK,
                    "Secrets",
                    counts.secrets,
                ),
                (ContentFilter::Image, icons::IMAGE, "Images", counts.images),
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
                self.render_header_actions(ui, ctx);
            });
        });
    }

    fn render_header_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui
            .small_button(icons::button_icon(icons::X))
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
            .small_button(icons::button_icon(icons::POWER))
            .on_hover_text(quit_tooltip)
            .clicked()
        {
            self.quit(ctx);
        }

        if render_view_toggle_button(
            ui,
            self.active_view == ActiveView::Settings,
            icons::GEAR,
            "Preferences & Settings",
        ) {
            self.active_view = if self.active_view == ActiveView::Settings {
                ActiveView::History
            } else {
                ActiveView::Settings
            };
        }

        if render_view_toggle_button(
            ui,
            self.active_view == ActiveView::Generator,
            icons::KEY,
            "Generate random password or passphrase",
        ) {
            self.active_view = if self.active_view == ActiveView::Generator {
                ActiveView::History
            } else {
                ActiveView::Generator
            };
        }

        if self.active_view == ActiveView::History
            && ui
                .small_button(icons::button_icon(icons::TRASH))
                .on_hover_text("Clear clipboard history")
                .clicked()
        {
            self.show_clear_confirm_dialog = true;
        }
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
                            .size(14.0_f32),
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
                            .small_button(icons::button_icon(icons::X))
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
                    if let Some(toast) = &self.toast {
                        if toast.shown_at.elapsed() < Duration::from_secs(2) {
                            let color = match toast.severity {
                                ToastSeverity::Success => crate::theme::success_color(is_dark),
                                ToastSeverity::Warning => crate::theme::warning_color(is_dark),
                                ToastSeverity::Error => crate::theme::error_color(is_dark),
                            };
                            ui.label(egui::RichText::new(&toast.message).color(color).strong());
                        } else {
                            self.toast = None;
                        }
                    } else {
                        let key_status = match self.key_storage_mode {
                            crate::crypto::KeyStorageMode::OsKeyring => {
                                format!(
                                    "{} {} entries • SQLCipher encrypted",
                                    icons::SHIELD_CHECK,
                                    self.items.len()
                                )
                            }
                            crate::crypto::KeyStorageMode::LocalFileFallback => {
                                format!(
                                    "{} {} entries • Local key file (keyring unavailable)",
                                    icons::WARNING,
                                    self.items.len()
                                )
                            }
                            crate::crypto::KeyStorageMode::LocalFileFallbackDegraded => {
                                format!(
                                    "{} {} entries • Local key file (permissions degraded)",
                                    icons::WARNING,
                                    self.items.len()
                                )
                            }
                        };
                        let status_color = match self.key_storage_mode {
                            crate::crypto::KeyStorageMode::OsKeyring => {
                                crate::theme::text_secondary(is_dark)
                            }
                            crate::crypto::KeyStorageMode::LocalFileFallback => {
                                crate::theme::warning_color(is_dark)
                            }
                            crate::crypto::KeyStorageMode::LocalFileFallbackDegraded => {
                                crate::theme::error_color(is_dark)
                            }
                        };
                        ui.label(
                            egui::RichText::new(key_status)
                                .color(status_color)
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

    fn render_modal_backdrop(ctx: &egui::Context, id: &'static str) -> bool {
        let screen_rect = ctx.content_rect();
        let mut clicked = false;
        egui::Area::new(egui::Id::unique(id))
            .order(egui::Order::Foreground)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    screen_rect,
                    0.0_f32,
                    egui::Color32::from_black_alpha(150),
                );
                clicked = response.clicked();
            });
        clicked
    }

    fn modal_window<'a>(
        title: String,
        open: &'a mut bool,
        ctx: &egui::Context,
        is_dark: bool,
        default_width: f32,
        max_width: f32,
    ) -> egui::Window<'a> {
        egui::Window::new(title)
            .open(open)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0_f32, 0.0_f32))
            .order(egui::Order::Foreground)
            .default_width(default_width)
            .max_width(max_width)
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .corner_radius(crate::theme::card_rounding())
                    .stroke(crate::theme::card_stroke(is_dark, false)),
            )
    }

    fn render_clear_confirm_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.show_clear_confirm_dialog {
            return;
        }

        if Self::render_modal_backdrop(ctx, "clear_confirm_backdrop") {
            self.show_clear_confirm_dialog = false;
        }

        let mut open = self.show_clear_confirm_dialog;
        Self::modal_window(
            format!("{} Clear History?", icons::WARNING),
            &mut open,
            ctx,
            is_dark,
            300.0_f32,
            320.0_f32,
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
                        match self.db.clear() {
                            Ok(()) => {
                                self.texture_cache.clear();
                                self.refresh_items();
                                self.show_clear_confirm_dialog = false;
                                self.show_toast(&format!("{} History cleared.", icons::CHECK));
                            }
                            Err(ClearError::Delete(error)) => {
                                self.report_db_write_failure("clear history", &error);
                            }
                            Err(error @ ClearError::Compaction { .. }) => {
                                crate::logging::log_message(
                                    "ERROR",
                                    &format!("Failed to clear history: {error}"),
                                );
                                self.texture_cache.clear();
                                self.refresh_items();
                                self.show_clear_confirm_dialog = false;
                                self.show_error_toast(
                                    "History rows were removed, but secure compaction failed. Restart and clear again to purge residual data.",
                                );
                            }
                        }
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

        if Self::render_modal_backdrop(ctx, "keyring_explanation_backdrop") {
            // Allow dismissing by clicking the backdrop.
            self.show_keyring_explanation = false;
            self.settings.keyring_explanation_shown = true;
            self.persist_settings();
        }

        let (title_icon, title, heading, body) =
            Self::keyring_explanation_copy(self.key_storage_mode);
        let is_fallback = matches!(
            self.key_storage_mode,
            crate::crypto::KeyStorageMode::LocalFileFallback
                | crate::crypto::KeyStorageMode::LocalFileFallbackDegraded
        );
        let heading_color = if is_fallback {
            crate::theme::warning_color(is_dark)
        } else {
            crate::theme::text_primary(is_dark)
        };
        let body_color = if is_fallback {
            crate::theme::warning_color(is_dark)
        } else {
            crate::theme::text_secondary(is_dark)
        };

        let mut open = self.show_keyring_explanation;
        Self::modal_window(
            format!("{title_icon} {title}"),
            &mut open,
            ctx,
            is_dark,
            360.0_f32,
            400.0_f32,
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0_f32);
                ui.label(
                    egui::RichText::new(heading)
                        .color(heading_color)
                        .strong()
                        .size(15.0_f32),
                );
                ui.add_space(8.0_f32);
                ui.label(egui::RichText::new(body).size(12.0_f32).color(body_color));
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
                        self.persist_settings();
                    }
                });
                ui.add_space(4.0_f32);
            });
        });

        if !open {
            self.show_keyring_explanation = false;
            self.settings.keyring_explanation_shown = true;
            self.persist_settings();
        }
    }
}

impl eframe::App for ClipboardApp {
    /// On macOS, exit right after cleanup to dodge an `AppKit` Touch Bar observer crash.
    #[cfg(target_os = "macos")]
    fn on_exit(&mut self) {
        if self.is_quitting {
            self.shutdown_and_exit();
        }
    }

    /// Runs before every `ui()` frame AND while the window is hidden (`Visible(false)`).
    /// In `eframe` 0.36+, `ui()` is skipped while the root viewport is invisible, so
    /// all wake-up, tray, hotkey, activation/reopen, monitor, and auto-clear handlers
    /// MUST live in `logic()`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_window_close_request(ctx);
        self.handle_tray_events(ctx);
        self.handle_hotkey_toggle(ctx);
        self.handle_app_activation(ctx);
        self.handle_monitor_updates();
        self.handle_clipboard_auto_clear();

        if let Some((_, deadline)) = &self.pending_clipboard_clear {
            ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }

        // In eframe 0.36+, background threads calling ctx.request_repaint() wake
        // invisible/minimized windows directly via UserEvent::RequestRepaint.
        // Only tick briefly after hiding until the OS finishes deactivating our app
        // so `was_inactive_since_hidden` latches; afterwards sleep at 0% idle CPU.
        if !self.is_visible && !self.was_inactive_since_hidden {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let is_dark = self.is_dark(&ctx);
        if self.current_is_dark != Some(is_dark) {
            crate::theme::setup_theme(&ctx, is_dark);
            self.current_is_dark = Some(is_dark);
        }
        if self.title_bar_dark_mode != Some(is_dark) {
            crate::platform::set_title_bar_dark_mode(is_dark);
            self.title_bar_dark_mode = Some(is_dark);
        }

        self.handle_quit_shortcut(&ctx);
        self.handle_escape_key(&ctx);
        self.handle_keyboard_navigation(&ctx);

        // Show keyring explanation dialog on first run (informational)
        if !self.settings.keyring_explanation_shown && !self.show_keyring_explanation {
            self.show_keyring_explanation = true;
        }

        if !self.is_visible {
            return;
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

impl ClipboardApp {
    /// Terminates the process right after cleanup on macOS.
    ///
    /// After the close request, winit's `NSApplication run` loop is still pumping and
    /// `AppKit`'s display cycle invalidates `_NSTouchBarFinderObservation` on the already
    /// torn-down `WinitView`, raising an uncaught `NSRangeException`. Exiting here, once
    /// our own cleanup finished, avoids that last `AppKit` flush.
    #[cfg(target_os = "macos")]
    fn shutdown_and_exit(&mut self) -> ! {
        self.clear_pending_secret_from_clipboard();
        if let Some(hotkey) = self.hotkey_service.as_mut() {
            hotkey.shutdown();
        }
        let _ = self.monitor_shutdown_tx.send(());
        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.join();
        }
        crate::platform::teardown_reopen_observer();
        std::process::exit(0);
    }
}

impl Drop for ClipboardApp {
    fn drop(&mut self) {
        self.clear_pending_secret_from_clipboard();
        if let Some(hotkey) = self.hotkey_service.as_mut() {
            hotkey.shutdown();
        }
        let _ = self.monitor_shutdown_tx.send(());
        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Clone, Copy)]
enum NavStep {
    Clamp,
    Wrap,
}

#[derive(Default, Clone, Copy)]
struct HistoryNavInput {
    down: Option<NavStep>,
    up: Option<NavStep>,
    copy_selected: bool,
    delete_selected: bool,
    copy_shortcut_slot: Option<usize>,
}

fn read_history_nav_input(ctx: &egui::Context) -> HistoryNavInput {
    const NUM_KEYS: [egui::Key; 9] = [
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

    let mut nav = HistoryNavInput::default();
    ctx.input_mut(|i| {
        for (idx, &k) in NUM_KEYS.iter().enumerate() {
            if i.consume_key(egui::Modifiers::COMMAND, k) {
                nav.copy_shortcut_slot = Some(idx);
                return;
            }
        }

        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::N)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::J)
        {
            nav.down = Some(NavStep::Clamp);
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
            nav.down = Some(NavStep::Wrap);
        }

        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::P)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::K)
        {
            nav.up = Some(NavStep::Clamp);
        } else if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
            nav.up = Some(NavStep::Wrap);
        }

        if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
            nav.copy_selected = true;
        }

        if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::Backspace)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::D)
        {
            nav.delete_selected = true;
        }
    });
    nav
}

fn format_recorded_hotkey(modifiers: egui::Modifiers, key: egui::Key) -> Option<String> {
    let key_name = egui_key_to_hotkey_name(key)?;
    // Note: in egui, `modifiers.command` aliases `ctrl` on Linux/Windows,
    // so only treat `mac_cmd` / `command` as Cmd on macOS.
    let mut parts = Vec::with_capacity(5);
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
    parts.push(key_name);
    Some(parts.join("+"))
}

#[allow(
    clippy::too_many_lines,
    reason = "exhaustive egui::Key mapping is kept in one place for hotkey editor auditability"
)]
fn egui_key_to_hotkey_name(key: egui::Key) -> Option<&'static str> {
    let name = match key {
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
            if crate::platform::quote_key_is_ugrave() {
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
        _ => return None,
    };
    Some(name)
}

fn render_view_toggle_button(
    ui: &mut egui::Ui,
    is_active: bool,
    inactive_icon: &'static str,
    inactive_tooltip: &str,
) -> bool {
    let response = if is_active {
        ui.small_button(format!("{} History", icons::CLOCK_COUNTER_CLOCKWISE))
            .on_hover_text("Back to clipboard history")
    } else {
        ui.small_button(icons::button_icon(inactive_icon))
            .on_hover_text(inactive_tooltip)
    };
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::{ClipboardApp, ToastSeverity};
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
        let key = format!("{:016x}", 1_u64).repeat(4);
        let db = crate::db::Database::open(&db_path, &key).unwrap();
        let mut app = ClipboardApp {
            db,
            key_storage_mode: crate::crypto::KeyStorageMode::OsKeyring,
            search_query: String::new(),
            content_filter: super::ContentFilter::All,
            items: Vec::new(),
            filter_counts: super::FilterCounts::default(),
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
            monitor_handle: None,
            is_quitting: false,
            show_keyring_explanation: false,
            last_drag_or_move_instant: None,
            last_outer_rect: None,
            last_hide_instant: None,
        };

        assert!(!app.is_quitting);

        // Verify TrayAction::Show restores visibility and surfaces any pending toast
        app.is_visible = false;
        app.pending_toast = Some(("Auto-paste blocked".to_string(), ToastSeverity::Warning));
        tray_tx.send(super::TrayAction::Show).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.is_visible);
        assert!(app.pending_toast.is_none());
        assert_eq!(
            app.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Auto-paste blocked")
        );
        assert_eq!(
            app.toast.as_ref().map(|toast| toast.severity),
            Some(ToastSeverity::Warning)
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

    #[test]
    fn test_keyring_explanation_copy_matches_storage_mode() {
        let (os_icon, os_title, os_heading, os_body) =
            ClipboardApp::keyring_explanation_copy(crate::crypto::KeyStorageMode::OsKeyring);
        assert_eq!(os_title, "Secure Key Storage");
        assert!(os_heading.contains("OS keyring"));
        assert!(os_body.contains("credential store"));

        let (fallback_icon, fallback_title, fallback_heading, fallback_body) =
            ClipboardApp::keyring_explanation_copy(
                crate::crypto::KeyStorageMode::LocalFileFallback,
            );
        assert_eq!(fallback_title, "Local Key File Fallback");
        assert!(fallback_heading.contains("local key file"));
        assert!(fallback_body.contains(".encryption_key"));
        assert_ne!(os_icon, fallback_icon);

        let (_degraded_icon, degraded_title, degraded_heading, degraded_body) =
            ClipboardApp::keyring_explanation_copy(
                crate::crypto::KeyStorageMode::LocalFileFallbackDegraded,
            );
        assert_eq!(degraded_title, "Local Key File Permissions Warning");
        assert!(degraded_heading.contains("permissions could not be restricted"));
        assert!(degraded_body.contains("icacls"));
        assert_ne!(fallback_title, degraded_title);
    }
}
