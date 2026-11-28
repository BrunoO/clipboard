mod generator_view;
mod history_view;
mod settings_view;

use history_view::ItemAction;

use crate::db::{ClearError, ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::icons;
use crate::media;
use crate::platform;
use crate::settings::AppSettings;
use crate::tray::{TrayAction, TrayService};
use arboard::Clipboard;
use eframe::egui;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

const MAX_CACHED_TEXTURES: usize = 24;

/// Bound-capped LRU texture cache for clipboard image previews.
/// Drops `egui::TextureHandle` references when capacity is exceeded or items are purged,
/// immediately freeing underlying GPU VRAM. Failed loads are remembered so a broken
/// blob is not retried on every frame.
#[derive(Default)]
pub(crate) struct ImageTextureCache {
    textures: HashMap<i64, egui::TextureHandle>,
    access_order: VecDeque<i64>,
    failed_ids: HashSet<i64>,
}

impl ImageTextureCache {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn is_cached(&self, id: i64) -> bool {
        self.textures.contains_key(&id) || self.failed_ids.contains(&id)
    }

    pub(crate) fn get_or_load(
        &mut self,
        id: i64,
        db: &Database,
        ctx: &egui::Context,
    ) -> Option<&egui::TextureHandle> {
        if self.textures.contains_key(&id) {
            if let Some(pos) = self.access_order.iter().position(|&x| x == id) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(id);
            return self.textures.get(&id);
        }
        if self.failed_ids.contains(&id) {
            return None;
        }

        let Ok(Some(png_bytes)) = db.get_image_data(id) else {
            self.failed_ids.insert(id);
            return None;
        };
        let Ok((w, h, rgba)) = media::png_to_rgba(&png_bytes) else {
            self.failed_ids.insert(id);
            return None;
        };
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
        self.failed_ids.remove(&id);
        self.textures.insert(id, handle);
        self.textures.get(&id)
    }

    pub(crate) fn remove(&mut self, id: i64) {
        self.textures.remove(&id);
        self.failed_ids.remove(&id);
        if let Some(pos) = self.access_order.iter().position(|&x| x == id) {
            self.access_order.remove(pos);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.textures.clear();
        self.access_order.clear();
        self.failed_ids.clear();
    }

    pub(crate) fn retain_active(&mut self, active_ids: &HashSet<i64>) {
        self.textures.retain(|id, _| active_ids.contains(id));
        self.access_order.retain(|id| active_ids.contains(id));
        self.failed_ids.retain(|id| active_ids.contains(id));
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
    Pinned,
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
    pinned: usize,
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
                ItemKind::Secret => {}
                ItemKind::Image => counts.images += 1,
            }
            if item.is_pinned {
                counts.pinned += 1;
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

/// Tracks window visibility, focus tracking, and interaction timestamps.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub(crate) struct WindowFocusState {
    pub(crate) is_visible: bool,
    pub(crate) has_been_focused: bool,
    pub(crate) was_inactive_since_hidden: bool,
    pub(crate) pending_order_front: bool,
    pub(crate) last_toggle_instant: Option<Instant>,
    pub(crate) last_drag_or_move_instant: Option<Instant>,
    pub(crate) last_outer_rect: Option<egui::Rect>,
    pub(crate) last_hide_instant: Option<Instant>,
    pub(crate) last_show_instant: Option<Instant>,
}

impl Default for WindowFocusState {
    fn default() -> Self {
        Self {
            is_visible: true,
            has_been_focused: false,
            was_inactive_since_hidden: false,
            pending_order_front: false,
            last_toggle_instant: None,
            last_drag_or_move_instant: None,
            last_outer_rect: None,
            last_hide_instant: None,
            last_show_instant: Some(Instant::now()),
        }
    }
}

/// Pure decision helper for whether a window should hide on blur.
#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub(crate) const fn should_hide_on_blur(
    has_been_focused: bool,
    hide_on_blur_setting: bool,
    in_show_grace_period: bool,
    is_minimized: bool,
    is_dragging_or_moving: bool,
    is_outside: bool,
) -> bool {
    has_been_focused
        && hide_on_blur_setting
        && !in_show_grace_period
        && !is_minimized
        && !is_dragging_or_moving
        && is_outside
}

/// Pure decision helper for whether a hidden/minimized window should reopen on focus/activation.
#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub(crate) fn should_reopen_on_focus(
    was_inactive_since_hidden: bool,
    elapsed_since_hide: Option<Duration>,
    is_active: bool,
    is_focused: bool,
    is_minimized: bool,
) -> bool {
    if !was_inactive_since_hidden || !(is_active || is_focused) {
        return false;
    }
    // Once the OS has completed the hide/minimize transition (`was_inactive_since_hidden`),
    // an explicit focus of the non-minimized main window (e.g. Taskbar or Alt+Tab restore)
    // must wake `show_window` immediately so the frame never renders blank.
    if is_focused && !is_minimized {
        return true;
    }
    elapsed_since_hide.is_none_or(|d| d >= Duration::from_millis(350))
}

/// Holds inter-thread communication channels and background services.
pub(crate) struct AppServices {
    pub(crate) notify_rx: Receiver<()>,
    pub(crate) hotkey_rx: Receiver<()>,
    pub(crate) paste_next_rx: crossbeam_channel::Receiver<()>,
    pub(crate) hotkey_service: Option<HotkeyService>,
    pub(crate) tray_rx: Receiver<TrayAction>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) tray_service: Option<TrayService>,
    pub(crate) monitor_shutdown_tx: Sender<()>,
    pub(crate) monitor_handle: Option<std::thread::JoinHandle<()>>,
    pub(crate) ocr_rx: Receiver<Result<(i64, String), (i64, String)>>,
    pub(crate) ocr_tx: Sender<Result<(i64, String), (i64, String)>>,
}

impl AppServices {
    /// Shuts down all background workers and joins threads.
    pub(crate) fn shutdown(&mut self) {
        if let Some(hotkey) = self.hotkey_service.as_mut() {
            hotkey.shutdown();
        }
        let _ = self.monitor_shutdown_tx.send(());
        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Ephemeral dialog and modal interaction state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
pub(crate) struct DialogState {
    pub(crate) show_clear_confirm_dialog: bool,
    pub(crate) show_keyring_explanation: bool,
    pub(crate) recording_toggle_hotkey: bool,
    pub(crate) manual_toggle_hotkey_edit: bool,
    pub(crate) pending_toggle_hotkey: String,
    pub(crate) recording_paste_next_hotkey: bool,
    pub(crate) manual_paste_next_hotkey_edit: bool,
    pub(crate) pending_paste_next_hotkey: String,
    pub(crate) hotkey_conflict_error: Option<String>,
    pub(crate) hotkey_recording_hint: Option<String>,
    pub(crate) show_new_secret_dialog: bool,
    pub(crate) new_secret_label: String,
    pub(crate) new_secret_value: zeroize::Zeroizing<String>,
    pub(crate) new_secret_notes: String,
    pub(crate) new_secret_reveal: bool,
    pub(crate) editing_annotation_id: Option<i64>,
    pub(crate) annotation_label_input: String,
    pub(crate) annotation_notes_input: String,
}

/// Stack stepping direction for Paste Next (§7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StackMode {
    #[default]
    Lifo,
    Fifo,
}

#[derive(Debug, Clone)]
struct InFlightPaste {
    seq: u64,
    submitted_id: i64,
    mode: StackMode,
    secret_to_clear: Option<zeroize::Zeroizing<String>>,
}

/// Result of reconciling a `Pasted` outcome against the in-flight queue.
pub(crate) enum PastedReconciliation {
    Stale,
    Reconciled {
        secret_to_clear: Option<zeroize::Zeroizing<String>>,
    },
}

/// Id-based LIFO / FIFO Paste Stack state machine (§7).
#[derive(Debug, Default)]
pub(crate) struct PasteStackState {
    pub(crate) next_marker: Option<i64>,
    pub(crate) mode: StackMode,
    next_seq: u64,
    in_flight: std::collections::VecDeque<InFlightPaste>,
    pub(crate) scroll_to_marker: bool,
}

impl PasteStackState {
    /// Resets to LIFO with the `NEXT` marker on the newest entry (or `None` if empty).
    pub(crate) fn reset_to_newest(&mut self, full_ids: &[i64]) {
        self.mode = StackMode::Lifo;
        self.next_marker = full_ids.first().copied();
        self.in_flight.clear();
    }

    /// Manually places the `NEXT` marker on `id`:
    /// - newest entry → `StackMode::Lifo` (Rule 5)
    /// - any older entry → `StackMode::Fifo` (Rule 2)
    pub(crate) fn place_marker(&mut self, id: i64, full_ids: &[i64]) {
        if !full_ids.contains(&id) {
            return;
        }
        self.next_marker = Some(id);
        self.mode = if full_ids.first().copied() == Some(id) {
            StackMode::Lifo
        } else {
            StackMode::Fifo
        };
        self.in_flight.clear();
    }

    /// Resolves the next history id to submit for a Paste Next press from full, unfiltered history.
    #[must_use]
    pub(crate) fn next_target_id(&self, full_ids: &[i64]) -> Option<i64> {
        if let Some(last) = self.in_flight.back() {
            let pos = full_ids.iter().position(|&id| id == last.submitted_id)?;
            match last.mode {
                StackMode::Lifo => full_ids.get(pos + 1).copied(),
                // Stop an in-flight FIFO burst at the newest entry to avoid accidental double-pastes.
                StackMode::Fifo => pos.checked_sub(1).and_then(|i| full_ids.get(i).copied()),
            }
        } else {
            self.next_marker.filter(|id| full_ids.contains(id))
        }
    }

    /// Records an in-flight submission and returns its monotonic sequence number.
    fn record_submission(
        &mut self,
        submitted_id: i64,
        secret_to_clear: Option<zeroize::Zeroizing<String>>,
    ) -> u64 {
        self.next_seq = self.next_seq.saturating_add(1);
        let seq = self.next_seq;
        self.in_flight.push_back(InFlightPaste {
            seq,
            submitted_id,
            mode: self.mode,
            secret_to_clear,
        });
        seq
    }

    /// Reconciles a `Pasted { seq }` outcome, advancing `next_marker` relative to `submitted_id`.
    pub(crate) fn on_pasted(&mut self, seq: u64, full_ids: &[i64]) -> PastedReconciliation {
        let Some(idx) = self.in_flight.iter().position(|e| e.seq == seq) else {
            return PastedReconciliation::Stale;
        };
        let Some(entry) = self.in_flight.remove(idx) else {
            return PastedReconciliation::Stale;
        };

        if let Some(pos) = full_ids.iter().position(|&id| id == entry.submitted_id) {
            match entry.mode {
                StackMode::Lifo => {
                    self.mode = StackMode::Lifo;
                    self.next_marker = full_ids.get(pos + 1).copied();
                }
                StackMode::Fifo => {
                    if pos == 0 {
                        // Rule 3: newest entry pasted → automatically return to LIFO on newest.
                        self.mode = StackMode::Lifo;
                        self.next_marker = full_ids.first().copied();
                    } else {
                        self.mode = StackMode::Fifo;
                        self.next_marker = full_ids.get(pos - 1).copied();
                    }
                }
            }
        } else if self
            .next_marker
            .is_some_and(|marker| !full_ids.contains(&marker))
        {
            self.reset_to_newest(full_ids);
        }

        PastedReconciliation::Reconciled {
            secret_to_clear: entry.secret_to_clear,
        }
    }

    /// Reconciles a `Failed { seq }` outcome: removes the in-flight entry and leaves `next_marker`
    /// and `mode` untouched.
    pub(crate) fn on_failed(&mut self, seq: u64) -> bool {
        if let Some(idx) = self.in_flight.iter().position(|e| e.seq == seq) {
            self.in_flight.remove(idx);
            true
        } else {
            false
        }
    }

    /// Updates `next_marker` after `deleted_id` is removed from history.
    pub(crate) fn on_entry_deleted(
        &mut self,
        deleted_id: i64,
        ids_before_delete: &[i64],
        ids_after_delete: &[i64],
    ) {
        if self.next_marker != Some(deleted_id) {
            return;
        }
        let Some(pos) = ids_before_delete.iter().position(|&id| id == deleted_id) else {
            self.reset_to_newest(ids_after_delete);
            return;
        };
        match self.mode {
            StackMode::Lifo => {
                self.next_marker = ids_before_delete
                    .get(pos + 1)
                    .copied()
                    .filter(|id| ids_after_delete.contains(id));
            }
            StackMode::Fifo => {
                if let Some(newer) = pos
                    .checked_sub(1)
                    .and_then(|i| ids_before_delete.get(i).copied())
                    .filter(|id| ids_after_delete.contains(id))
                {
                    // Stays in FIFO until the newest entry is pasted (Rule 3).
                    self.next_marker = Some(newer);
                } else {
                    self.mode = StackMode::Lifo;
                    self.next_marker = ids_after_delete.first().copied();
                }
            }
        }
    }
}

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
    services: AppServices,
    texture_cache: ImageTextureCache,
    window_state: WindowFocusState,
    selected_index: usize,
    focus_search_input: bool,
    settings: AppSettings,
    active_view: ActiveView,
    password_gen: crate::password::PasswordGeneratorState,
    revealed_secrets: HashSet<i64>,
    pending_clipboard_clear: Option<(zeroize::Zeroizing<String>, Instant)>,
    dialogs: DialogState,
    scroll_to_selected: bool,
    current_is_dark: Option<bool>,
    title_bar_dark_mode: Option<bool>,
    is_quitting: bool,
    pub(crate) copied_item_pulse: Option<(i64, Instant)>,
    pub(crate) paste_stack: PasteStackState,
    data_dir: std::path::PathBuf,
}

impl ClipboardApp {
    pub(crate) fn new(
        _cc: &eframe::CreationContext<'_>,
        db: Database,
        key_storage_mode: crate::crypto::KeyStorageMode,
        services: AppServices,
        settings: AppSettings,
        data_dir: std::path::PathBuf,
        startup_error: Option<String>,
    ) -> Self {
        db.apply_settings(&settings);

        let mut startup_toast = if let Some(message) = startup_error {
            Some(Toast {
                message: format!("{} {message}", icons::WARNING),
                severity: ToastSeverity::Error,
                shown_at: Instant::now(),
            })
        } else if services.hotkey_service.is_none() && !settings.toggle_hotkey.is_empty() {
            Some(Toast {
                message: format!(
                    "{} Global hotkey ({}) is unavailable or in use by another app",
                    icons::WARNING,
                    settings.toggle_hotkey,
                ),
                severity: ToastSeverity::Warning,
                shown_at: Instant::now(),
            })
        } else {
            None
        };
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
        let initial_ids: Vec<i64> = initial_items.iter().map(|i| i.id).collect();
        let mut paste_stack = PasteStackState::default();
        paste_stack.reset_to_newest(&initial_ids);
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
            services,
            texture_cache: ImageTextureCache::new(),
            window_state: WindowFocusState::default(),
            selected_index: 0,
            focus_search_input: true,
            settings,
            active_view: ActiveView::History,
            password_gen,
            revealed_secrets: HashSet::new(),
            pending_clipboard_clear: None,
            dialogs: DialogState::default(),
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            is_quitting: false,
            copied_item_pulse: None,
            paste_stack,
            data_dir,
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
        match self.db.get_recent(self.settings.max_history, filter) {
            Ok(items) => {
                self.items = items;
                self.filter_counts = FilterCounts::from_items(&self.items);
                let active_ids: HashSet<i64> = self.items.iter().map(|it| it.id).collect();
                self.texture_cache.retain_active(&active_ids);
            }
            Err(error) => {
                crate::logging::log_message(
                    "ERROR",
                    &format!("Failed to load clipboard history: {error}"),
                );
                self.show_error_toast(&format!(
                    "{} Failed to load clipboard history.",
                    icons::WARNING
                ));
            }
        }
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

    fn persist_settings(&mut self) -> bool {
        if let Err(error) = self.settings.save(&self.db) {
            crate::logging::log_message("ERROR", &format!("Failed to save settings: {error}"));
            self.show_error_toast(&format!(
                "{} Failed to save settings. Changes will be lost on restart.",
                icons::WARNING
            ));
            false
        } else {
            true
        }
    }

    fn show_toast_severity(&mut self, message: &str, severity: ToastSeverity) {
        self.toast = Some(Toast {
            message: message.to_string(),
            severity,
            shown_at: Instant::now(),
        });
    }

    fn export_encrypted_backup(&mut self) {
        let timestamp = jiff::ZonedDateTime::now()
            .strftime("%Y%m%d-%H%M%S")
            .to_string();
        let file_name = format!("cipherclipboard-backup-{timestamp}.db");
        let default_dir = dirs::download_dir().unwrap_or_else(|| self.data_dir.clone());

        let dest = rfd::FileDialog::new()
            .set_directory(&default_dir)
            .set_file_name(&file_name)
            .add_filter("CipherClipboard backup", &["db"])
            .set_title("Export encrypted backup")
            .save_file();
        // Native file dialogs blur the main window; reset the focus grace window so
        // `hide_on_blur` does not immediately hide the app and swallow the feedback toast.
        self.window_state.last_show_instant = Some(Instant::now());
        let Some(dest) = dest else {
            return;
        };

        let (encryption_key, _) =
            match crate::crypto::get_or_create_encryption_key(&self.data_dir, true) {
                Ok(key) => key,
                Err(error) => {
                    crate::logging::log_message(
                        "ERROR",
                        &format!("Backup export failed to load encryption key: {error}"),
                    );
                    self.show_error_toast(&format!(
                        "{} Backup export failed: {error}",
                        icons::WARNING
                    ));
                    return;
                }
            };

        match self.db.export_backup(&dest, &encryption_key) {
            Ok(()) => {
                crate::logging::log_message(
                    "INFO",
                    &format!("Encrypted backup exported to {}", dest.display()),
                );
                self.show_toast(&format!(
                    "{} Backup exported: {}",
                    icons::CHECK,
                    dest.display()
                ));
            }
            Err(error) => {
                crate::logging::log_message("ERROR", &format!("Backup export failed: {error}"));
                self.show_error_toast(&format!("{} Backup export failed: {error}", icons::WARNING));
            }
        }
    }

    fn stage_restore_backup(&mut self) {
        let default_dir = dirs::download_dir().unwrap_or_else(|| self.data_dir.clone());
        let path = rfd::FileDialog::new()
            .set_directory(&default_dir)
            .add_filter("CipherClipboard backup", &["db"])
            .set_title("Select encrypted backup to restore")
            .pick_file();
        // Reset the focus grace window after the native dialog closes so `hide_on_blur`
        // does not immediately hide the app and swallow the confirmation toast.
        self.window_state.last_show_instant = Some(Instant::now());
        let Some(path) = path else {
            return;
        };

        let (encryption_key, _) =
            match crate::crypto::get_or_create_encryption_key(&self.data_dir, true) {
                Ok(key) => key,
                Err(error) => {
                    crate::logging::log_message(
                        "ERROR",
                        &format!("Restore validation failed to load encryption key: {error}"),
                    );
                    self.show_error_toast(&format!("{} {error}", icons::WARNING));
                    return;
                }
            };

        if let Err(error) = crate::db::Database::validate_backup_file(&path, &encryption_key) {
            self.show_error_toast(&format!("{} {error}", icons::WARNING));
            return;
        }
        drop(encryption_key);

        let staged_db = self.data_dir.join("pending_restore.db");
        let marker = self.data_dir.join("pending_restore.marker");
        if let Err(e) = std::fs::copy(&path, &staged_db) {
            self.show_error_toast(&format!("{} Failed to stage restore: {e}", icons::WARNING));
            return;
        }
        if crate::platform::fs_security::set_restricted_permissions(&staged_db) {
            let _ = std::fs::remove_file(&staged_db);
            self.show_error_toast(&format!(
                "{} Failed to restrict permissions on staged backup.",
                icons::WARNING
            ));
            return;
        }
        if let Err(e) = std::fs::write(&marker, b"pending") {
            let _ = std::fs::remove_file(&staged_db);
            self.show_error_toast(&format!("{} Failed to mark restore: {e}", icons::WARNING));
            return;
        }

        crate::logging::log_message(
            "INFO",
            &format!("Staged backup restore from {}", path.display()),
        );
        self.show_warning_toast(&format!(
            "{} Restore staged. Restart CipherClipboard to apply. Current DB will be backed up first.",
            icons::WARNING
        ));
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
        #[cfg(debug_assertions)]
        crate::logging::log_message("INFO", "show_window called");
        self.window_state.is_visible = true;
        self.window_state.has_been_focused = false;
        self.window_state.was_inactive_since_hidden = false;
        self.window_state.last_show_instant = Some(Instant::now());
        self.selected_index = 0;
        self.dialogs.show_clear_confirm_dialog = false;
        self.scroll_to_selected = true;
        if crate::platform::defer_order_front_until_painted() {
            // Let `App::ui()` paint and present one fresh frame into the hidden Metal surface
            // before unhiding the app on the next `logic()` pass (eliminates blank-frame flicker).
            self.window_state.pending_order_front = true;
        } else {
            // Restore at the OS level first (important on Windows, where a minimized
            // window may otherwise stay minimized even after Visible(true)).
            crate::platform::order_front_main_window();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        // Only refresh from SQLite if the background monitor or search query actually changed
        // since the last pass, avoiding a redundant full DB query + re-classification on summon.
        self.handle_monitor_updates();
        if let Some((msg, severity)) = self.pending_toast.take() {
            self.show_toast_severity(&msg, severity);
        }
        self.focus_search_input = true;
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context, restore_focus: bool, auto_paste: bool) {
        self.window_state.is_visible = false;
        self.window_state.has_been_focused = false;
        self.window_state.was_inactive_since_hidden = false;
        self.window_state.pending_order_front = false;
        self.window_state.last_hide_instant = Some(Instant::now());
        self.dialogs.show_clear_confirm_dialog = false;
        self.revealed_secrets.clear();

        let has_live_tray = self
            .services
            .tray_service
            .as_ref()
            .is_some_and(TrayService::is_live);
        #[cfg(debug_assertions)]
        crate::logging::log_message(
            "INFO",
            &format!(
                "hide_window called (minimize_to_taskbar={}, has_live_tray={has_live_tray})",
                self.settings.minimize_to_taskbar
            ),
        );
        platform::hide_window_viewport(ctx, has_live_tray, self.settings.minimize_to_taskbar);

        let mut auto_paste = auto_paste;
        if auto_paste {
            if let Some(warning) = platform::auto_paste_blocked_warning() {
                auto_paste = false;
                self.pending_toast = Some((warning.to_string(), ToastSeverity::Warning));
            }
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
        if !clipboard_clear_is_due(Instant::now(), *deadline) {
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

    pub(super) fn copy_plain_text(&mut self, text: &str, toast: &str, ctx: &egui::Context) {
        ctx.copy_text(text.to_string());
        let Some(cb) = self.ensure_clipboard_writer() else {
            self.show_warning_toast(&format!("{} Clipboard unavailable", icons::WARNING));
            return;
        };
        if let Err(e) = cb.set_text(text.to_string()) {
            eprintln!("Failed to write text to clipboard: {e}");
            self.show_error_toast(&format!("{} Failed to copy to clipboard", icons::WARNING));
        } else {
            self.show_toast(toast);
        }
    }

    fn copy_secret_to_clipboard(&mut self, secret: &str, ctx: &egui::Context) {
        self.db
            .skip_next_capture(crate::db::SkipCaptureTarget::Text(zeroize::Zeroizing::new(
                secret.to_string(),
            )));
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

    fn copy_transformed_to_clipboard(
        &mut self,
        content: &str,
        toast: &str,
        is_secret: bool,
        ctx: &egui::Context,
    ) {
        if is_secret {
            self.copy_secret_to_clipboard(content, ctx);
        } else {
            self.copy_text_to_clipboard(content, ctx);
        }
        self.show_toast(toast);
    }

    fn copy_generated_password(&mut self, ctx: &egui::Context, dismiss: bool) {
        let secret = self.password_gen.current_value.clone();
        if secret.is_empty() {
            return;
        }
        // Prevent the background monitor from redundantly re-capturing this copy 200ms later.
        self.db
            .skip_next_capture(crate::db::SkipCaptureTarget::Text(secret.clone()));

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
            if self.password_gen.skip_history {
                let msg = format!("{} Copied password (not saved to history)", icons::CHECK);
                self.finish_copy(ctx, &msg, dismiss);
            } else {
                match self.db.insert_secret(&secret) {
                    Ok(_) => {
                        self.refresh_items();
                        let msg = format!("{} Copied & saved password as Secret!", icons::LOCK);
                        self.finish_copy(ctx, &msg, dismiss);
                    }
                    Err(err) => {
                        crate::logging::log_message(
                            "ERROR",
                            &format!("Failed to save generated password to database: {err}"),
                        );
                        self.show_error_toast(&format!(
                            "{} Copied password, but failed to save it to history.",
                            icons::WARNING
                        ));
                    }
                }
            }
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
        while self.services.hotkey_rx.try_recv().is_ok() {
            toggle_requested = true;
        }

        if !toggle_requested {
            return;
        }

        // Debounce hotkey presses to ignore rapid OS key repeats (200ms threshold)
        if self
            .window_state
            .last_toggle_instant
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(200))
        {
            return;
        }
        self.window_state.last_toggle_instant = Some(Instant::now());

        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));

        if self.window_state.is_visible && is_focused {
            // Window is already active and focused right in front of user: dismiss it.
            self.hide_window(ctx, self.settings.restore_focus, false);
        } else {
            // Window is hidden, minimized, or in the background behind other apps:
            // ALWAYS summon and focus it immediately on the first press!
            self.show_window(ctx);
        }
    }

    fn handle_escape_key(&mut self, ctx: &egui::Context) {
        if self.dialogs.recording_toggle_hotkey || self.dialogs.recording_paste_next_hotkey {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.dialogs.show_clear_confirm_dialog {
                self.dialogs.show_clear_confirm_dialog = false;
            } else if self.dialogs.editing_annotation_id.is_some() {
                self.dialogs.editing_annotation_id = None;
            } else if self.dialogs.show_new_secret_dialog {
                self.close_new_secret_dialog();
            } else if self.active_view != ActiveView::History {
                self.active_view = ActiveView::History;
            } else {
                self.hide_window(ctx, self.settings.restore_focus, false);
            }
        }
    }

    /// Handles key input during hotkey recording mode (for either Toggle Window or Paste Next).
    /// Uses raw events to reliably capture modifier+key combinations.
    fn handle_hotkey_recording(&mut self, ctx: &egui::Context, is_paste_next: bool) {
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
                    if is_paste_next {
                        self.cancel_recording_paste_next_hotkey();
                    } else {
                        self.cancel_recording_toggle_hotkey();
                    }
                    return;
                }

                let active_mods = active_modifier_labels(*modifiers);
                if *key == egui::Key::Backspace && active_mods.is_empty() {
                    if is_paste_next {
                        self.clear_paste_next_hotkey();
                    } else {
                        self.clear_toggle_hotkey();
                    }
                    return;
                }

                if active_mods.is_empty() {
                    self.dialogs.hotkey_recording_hint = Some(
                        "Include at least one modifier key (Ctrl, Alt, Shift, or Cmd)".to_string(),
                    );
                    return;
                }

                if let Some(combo) = format_recorded_hotkey(*modifiers, *key) {
                    if is_paste_next {
                        self.dialogs.pending_paste_next_hotkey = combo;
                        self.apply_paste_next_hotkey();
                    } else {
                        self.dialogs.pending_toggle_hotkey = combo;
                        self.apply_toggle_hotkey();
                    }
                    return;
                }
            }
        });
    }

    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        if self.dialogs.recording_toggle_hotkey {
            self.handle_hotkey_recording(ctx, false);
            return;
        }
        if self.dialogs.recording_paste_next_hotkey {
            self.handle_hotkey_recording(ctx, true);
            return;
        }

        if self.dialogs.show_clear_confirm_dialog
            || self.dialogs.editing_annotation_id.is_some()
            || self.dialogs.show_new_secret_dialog
            || !self.window_state.is_visible
        {
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

        if nav.copy_raw_selected {
            self.copy_raw_filtered_item(&filtered_indices, self.selected_index, ctx);
        } else if nav.copy_selected {
            self.copy_filtered_item(&filtered_indices, self.selected_index, ctx);
        }

        if nav.delete_selected {
            self.delete_filtered_item(&filtered_indices, ctx);
        }

        if nav.toggle_secret_selected {
            self.toggle_secret_filtered_item(&filtered_indices, ctx);
        }

        if nav.edit_annotation_selected {
            self.edit_annotation_filtered_item(&filtered_indices, ctx);
        }

        if nav.set_paste_next_marker_selected && self.settings.active_paste_next_hotkey().is_some()
        {
            if let Some(&item_idx) = filtered_indices.get(self.selected_index) {
                if let Some(item) = self.items.get(item_idx) {
                    let id = item.id;
                    self.place_paste_next_marker(id);
                    ctx.request_repaint();
                }
            }
        }

        if nav.new_secret {
            self.open_new_secret_dialog();
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

    fn copy_image_by_id(&mut self, id: i64, ctx: &egui::Context) {
        match self.db.get_image_data(id) {
            Ok(Some(png_bytes)) => self.copy_image_to_clipboard(&png_bytes, ctx),
            Ok(None) => self.show_warning_toast(&format!(
                "{} Image data is no longer available.",
                icons::WARNING
            )),
            Err(error) => {
                self.show_error_toast(&format!("{} Failed to load image: {error}", icons::WARNING));
            }
        }
    }

    fn start_image_ocr(&mut self, id: i64, ctx: &egui::Context) {
        if !crate::platform::supports_image_ocr() {
            self.show_warning_toast(&format!(
                "{} Image OCR is not supported on this platform.",
                icons::WARNING
            ));
            return;
        }

        let png_bytes = match self.db.get_image_data(id) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.show_warning_toast(&format!(
                    "{} Image data is no longer available.",
                    icons::WARNING
                ));
                return;
            }
            Err(err) => {
                self.show_error_toast(&format!("{} Failed to load image: {err}", icons::WARNING));
                return;
            }
        };

        self.show_toast(&format!(
            "{} Extracting text from image...",
            icons::ARROWS_CLOCKWISE
        ));

        let ocr_tx = self.services.ocr_tx.clone();
        let ctx_clone = ctx.clone();
        let thread_name = format!("image-ocr-worker-{id}");
        let spawn_res = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let res = crate::platform::extract_text_from_png(&png_bytes)
                    .map(|text| (id, text))
                    .map_err(|err| (id, err));
                let _ = ocr_tx.send(res);
                ctx_clone.request_repaint();
            });

        if let Err(e) = spawn_res {
            crate::logging::log_message("ERROR", &format!("Failed to spawn OCR worker: {e}"));
            self.show_error_toast(&format!("{} Failed to start OCR: {e}", icons::WARNING));
        }
    }

    fn handle_ocr_results(&mut self, ctx: &egui::Context) {
        while let Ok(res) = self.services.ocr_rx.try_recv() {
            match res {
                Ok((_id, text)) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        self.show_toast(&format!("{} No text detected in image.", icons::INFO));
                    } else {
                        let char_count = trimmed.chars().count();
                        let line_count = trimmed.lines().count();
                        let toast = format!(
                            "{} Extracted {char_count} chars ({line_count} lines) to clipboard!",
                            icons::CHECK
                        );
                        self.copy_transformed_to_clipboard(trimmed, &toast, false, ctx);
                    }
                }
                Err((_id, err)) => {
                    self.show_warning_toast(&format!("{} OCR failed: {err}", icons::WARNING));
                }
            }
        }
    }

    fn copy_filtered_item(&mut self, filtered_indices: &[usize], slot: usize, ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(slot) {
            if let Some(item) = self.items.get(item_idx) {
                self.copied_item_pulse = Some((item.id, Instant::now()));
            }
            let copy_action = self.items.get(item_idx).map(|item| match item.kind {
                ItemKind::Image => ItemAction::CopyImage(item.id),
                ItemKind::Secret => ItemAction::CopySecret(item.content.clone()),
                _ => ItemAction::CopyText(item.content.clone()),
            });
            match copy_action {
                Some(ItemAction::CopyImage(id)) => self.copy_image_by_id(id, ctx),
                Some(ItemAction::CopySecret(secret)) => self.copy_secret_to_clipboard(&secret, ctx),
                Some(ItemAction::CopyText(text)) => self.copy_text_to_clipboard(&text, ctx),
                _ => {}
            }
        }
    }

    fn copy_raw_filtered_item(
        &mut self,
        filtered_indices: &[usize],
        slot: usize,
        ctx: &egui::Context,
    ) {
        if let Some(&item_idx) = filtered_indices.get(slot) {
            if let Some(item) = self.items.get(item_idx) {
                self.copied_item_pulse = Some((item.id, Instant::now()));
                if item.kind == ItemKind::Image {
                    self.copy_image_by_id(item.id, ctx);
                } else {
                    let raw = crate::transform::strip_source_formatting(&item.content);
                    let is_secret = item.kind == ItemKind::Secret;
                    let toast = format!("{} Copied raw plain text to clipboard!", icons::CHECK);
                    self.copy_transformed_to_clipboard(&raw, &toast, is_secret, ctx);
                }
            }
        }
    }

    fn delete_filtered_item(&mut self, filtered_indices: &[usize], ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(self.selected_index) {
            if let Some(item) = self.items.get(item_idx) {
                let id = item.id;
                let ids_before = self.full_history_ids();
                if let Err(error) = self.db.delete(id) {
                    self.report_db_write_failure("delete entry", &error);
                    return;
                }
                let ids_after = self.full_history_ids();
                self.paste_stack
                    .on_entry_deleted(id, &ids_before, &ids_after);
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

    fn toggle_secret_filtered_item(&mut self, filtered_indices: &[usize], ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(self.selected_index) {
            if let Some(item) = self.items.get(item_idx) {
                if item.kind != ItemKind::Image {
                    let make_secret = item.kind != ItemKind::Secret;
                    let id = item.id;
                    self.dispatch_item_action(ItemAction::ToggleSecret(id, make_secret), ctx);
                }
            }
        }
    }

    fn edit_annotation_filtered_item(&mut self, filtered_indices: &[usize], ctx: &egui::Context) {
        if let Some(&item_idx) = filtered_indices.get(self.selected_index) {
            if let Some(item) = self.items.get(item_idx) {
                let id = item.id;
                self.dispatch_item_action(ItemAction::EditAnnotation(id), ctx);
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
            if self
                .window_state
                .last_outer_rect
                .is_some_and(|prev| prev != rect)
            {
                self.window_state.last_drag_or_move_instant = Some(Instant::now());
            }
            self.window_state.last_outer_rect = Some(rect);
        }

        let active_state = crate::platform::is_app_active_state();
        let is_active = active_state.unwrap_or(false);
        let reopen_requested = crate::platform::take_reopen_requested();

        let in_show_grace_period = self
            .window_state
            .last_show_instant
            .is_some_and(|t| t.elapsed() < Duration::from_millis(500));

        // If an OS reopen was requested (e.g. macOS Dock click, Windows second-instance launch),
        // summon and bring to front—unless `show_window()` already ran moments ago (which itself
        // triggers macOS activation/unhide notifications).
        if reopen_requested {
            if !(self.window_state.is_visible && in_show_grace_period) {
                self.show_window(ctx);
            }
            return;
        }

        if self.window_state.is_visible {
            if is_focused {
                if !in_show_grace_period {
                    self.window_state.has_been_focused = true;
                }
            } else {
                let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
                let is_dragging_or_moving = pointer_down
                    || self
                        .window_state
                        .last_drag_or_move_instant
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(350));
                let is_outside = active_state.map_or(!is_focused, |active| !active);

                if should_hide_on_blur(
                    self.window_state.has_been_focused,
                    self.settings.hide_on_blur,
                    in_show_grace_period,
                    is_minimized,
                    is_dragging_or_moving,
                    is_outside,
                ) {
                    self.hide_window(ctx, false, false);
                }
            }
        } else {
            let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
            if !is_active && !is_focused {
                self.window_state.was_inactive_since_hidden = true;
            }

            let elapsed_since_hide = self.window_state.last_hide_instant.map(|t| t.elapsed());
            // Reveal window if the app regained activation/focus after being hidden and inactive.
            if should_reopen_on_focus(
                self.window_state.was_inactive_since_hidden,
                elapsed_since_hide,
                is_active,
                is_focused,
                is_minimized,
            ) {
                self.show_window(ctx);
            }
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.is_quitting {
                return;
            }
            crate::logging::log_message(
                "INFO",
                "CloseRequested intercepted; canceling close and hiding window",
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx, false, false);
        }
    }

    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        while let Ok(action) = self.services.tray_rx.try_recv() {
            crate::logging::log_message("INFO", &format!("tray event received: {action:?}"));
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
                    break;
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
            crate::platform::is_primary_shortcut_modifier(i.modifiers)
                && i.key_pressed(egui::Key::Q)
        });

        if quit_pressed {
            self.quit(ctx);
        }
    }

    pub(super) fn full_history_ids(&self) -> Vec<i64> {
        self.db
            .get_recent(self.settings.max_history, None)
            .map(|items| items.into_iter().map(|i| i.id).collect())
            .unwrap_or_default()
    }

    pub(super) fn reset_paste_stack_to_newest(&mut self) {
        let full_ids = self.full_history_ids();
        self.paste_stack.reset_to_newest(&full_ids);
    }

    pub(super) fn place_paste_next_marker(&mut self, id: i64) {
        let full_ids = self.full_history_ids();
        self.paste_stack.place_marker(id, &full_ids);
        let mode_label = match self.paste_stack.mode {
            StackMode::Lifo => "LIFO ↓",
            StackMode::Fifo => "FIFO ↑",
        };
        self.show_toast(&format!(
            "{} Paste Next marker set ({mode_label})",
            icons::CHECK
        ));
    }

    fn handle_paste_next_events(&mut self, ctx: &egui::Context) {
        // Drain one press at a time — never coalesce Paste Next presses (§3.2).
        while self.services.paste_next_rx.try_recv().is_ok() {
            self.handle_paste_next(ctx);
        }
    }

    fn handle_paste_next(&mut self, ctx: &egui::Context) {
        if self.settings.has_hotkey_collision() {
            return;
        }
        let Some(parsed_hk) = self.settings.paste_next_hotkey_parsed() else {
            return;
        };
        let Ok(full_items) = self.db.get_recent(self.settings.max_history, None) else {
            return;
        };
        let full_ids: Vec<i64> = full_items.iter().map(|i| i.id).collect();
        let Some(target_id) = self.paste_stack.next_target_id(&full_ids) else {
            return;
        };
        let Some(item) = full_items.into_iter().find(|i| i.id == target_id) else {
            return;
        };

        let (payload, secret_to_clear) = match item.kind {
            ItemKind::Image => {
                let png_bytes = match self.db.get_image_data(item.id) {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => {
                        crate::logging::log_message(
                            "WARN",
                            "Paste Next skipped: image data missing",
                        );
                        return;
                    }
                    Err(e) => {
                        crate::logging::log_message(
                            "WARN",
                            &format!("Paste Next skipped: failed to load image ({e})"),
                        );
                        return;
                    }
                };
                match crate::media::png_to_rgba(&png_bytes) {
                    Ok((width, height, rgba)) => (
                        crate::platform::PasteNextPayload::Image {
                            width,
                            height,
                            rgba,
                        },
                        None,
                    ),
                    Err(e) => {
                        crate::logging::log_message(
                            "WARN",
                            &format!("Paste Next skipped oversized/invalid image: {e}"),
                        );
                        if self.window_state.is_visible {
                            self.show_warning_toast(&format!(
                                "{} Skipped invalid or oversized image.",
                                icons::WARNING
                            ));
                        }
                        return;
                    }
                }
            }
            ItemKind::Text
            | ItemKind::Url
            | ItemKind::Code
            | ItemKind::Color
            | ItemKind::Secret => {
                if item.content.len() > crate::monitor::MAX_TEXT_LENGTH {
                    crate::logging::log_message(
                        "WARN",
                        "Paste Next skipped: text entry exceeds maximum size limit",
                    );
                    if self.window_state.is_visible {
                        self.show_warning_toast(&format!(
                            "{} Skipped oversized text entry.",
                            icons::WARNING
                        ));
                    }
                    return;
                }
                let conceal = item.kind == ItemKind::Secret;
                let text = zeroize::Zeroizing::new(item.content);
                let secret_to_clear = conceal.then(|| text.clone());
                (
                    crate::platform::PasteNextPayload::Text { text, conceal },
                    secret_to_clear,
                )
            }
        };

        let seq = self
            .paste_stack
            .record_submission(target_id, secret_to_clear);
        crate::platform::submit_paste_next(crate::platform::PasteNextJob {
            seq,
            payload,
            settle_ms: self.settings.auto_paste_delay_ms,
            release_modifiers: parsed_hk.modifiers(),
            db: self.db.clone(),
            ctx: ctx.clone(),
        });
    }

    fn handle_paste_next_outcomes(&mut self) {
        while let Some(outcome) = crate::platform::try_recv_paste_next_outcome() {
            match outcome {
                crate::platform::PasteNextOutcome::Pasted { seq } => {
                    let full_ids = self.full_history_ids();
                    if let PastedReconciliation::Reconciled { secret_to_clear } =
                        self.paste_stack.on_pasted(seq, &full_ids)
                    {
                        if let Some(secret) = secret_to_clear {
                            self.schedule_secret_clipboard_clear(&secret);
                        } else {
                            self.pending_clipboard_clear = None;
                        }
                    }
                }
                crate::platform::PasteNextOutcome::Failed { seq, reason } => {
                    let _ = self.paste_stack.on_failed(seq);
                    self.db.clear_skip_capture();
                    crate::logging::log_message(
                        "WARN",
                        &format!("Paste Next failed (seq={seq}): {reason}"),
                    );
                    if reason == "Accessibility permission missing" {
                        let msg = format!(
                            "{} Accessibility permission required for Paste Next (grant in System Settings).",
                            icons::WARNING
                        );
                        if self.window_state.is_visible {
                            self.show_warning_toast(&msg);
                        } else {
                            self.pending_toast = Some((msg, ToastSeverity::Warning));
                        }
                    } else if self.window_state.is_visible {
                        self.show_warning_toast(&format!(
                            "{} Paste Next failed: {reason}",
                            icons::WARNING
                        ));
                    }
                }
            }
        }
    }

    fn handle_monitor_updates(&mut self) {
        let mut new_items_detected = false;
        while self.services.notify_rx.try_recv().is_ok() {
            new_items_detected = true;
        }

        if new_items_detected {
            self.reset_paste_stack_to_newest();
        }

        let query_changed = self.search_query != self.last_query_cached;
        if monitor_update_needs_refresh(new_items_detected, query_changed) {
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
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ui, |ui| {
                crate::theme::paint_brand_top_accent(ui.painter(), ctx.viewport_rect(), is_dark);
                ui.add_space(crate::theme::top_padding());
                self.render_header_row(ui, &ctx, is_dark);
                ui.add_space(4.0_f32);
                if self.active_view == ActiveView::History {
                    self.render_search_bar(ui, is_dark);
                    ui.add_space(4.0_f32);
                    self.render_filter_tabs(ui, is_dark);
                    ui.add_space(2.0_f32);
                }
            });
    }

    fn render_filter_tabs(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let old_filter = self.content_filter;
        let counts = self.filter_counts;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
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
                    ContentFilter::Pinned,
                    icons::PUSH_PIN,
                    "Pinned",
                    counts.pinned,
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

            let title_resp = if crate::theme::use_compact_header_icon() {
                ui.label(
                    egui::RichText::new(icons::LOCK_KEY.to_string())
                        .color(crate::theme::text_secondary(is_dark))
                        .size(14.0_f32),
                )
            } else {
                ui.heading(
                    egui::RichText::new(format!("{} Clipboard", icons::LOCK_KEY))
                        .color(crate::theme::text_primary(is_dark))
                        .strong(),
                )
            };
            if title_resp.interact(egui::Sense::drag()).drag_started() {
                self.window_state.last_drag_or_move_instant = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            if ui.available_width() > 220.0 {
                let badge = self.settings.toggle_hotkey_badge();
                ui.label(
                    egui::RichText::new(format!("({badge})"))
                        .color(crate::theme::text_secondary(is_dark))
                        .size(11.0_f32),
                );
            }

            if self.active_view == ActiveView::History
                && self.settings.active_paste_next_hotkey().is_some()
            {
                self.render_paste_stack_mode_chip(ui, is_dark);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_header_actions(ui, ctx, is_dark);
            });
        });
    }

    fn render_paste_stack_mode_chip(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let filtered_indices = self.get_filtered_indices();
        let marker_visible = self.paste_stack.next_marker.is_some_and(|marker_id| {
            filtered_indices
                .iter()
                .any(|&idx| self.items.get(idx).is_some_and(|item| item.id == marker_id))
        });
        let hidden_by_filter = self.paste_stack.next_marker.is_some() && !marker_visible;

        let (base_label, color) = match self.paste_stack.mode {
            StackMode::Lifo => ("LIFO ↓", crate::theme::accent_color(is_dark)),
            StackMode::Fifo => ("FIFO ↑", crate::theme::success_color(is_dark)),
        };
        let label = if hidden_by_filter {
            format!("{base_label} · hidden")
        } else {
            base_label.to_string()
        };
        let bg = color.gamma_multiply(if is_dark { 0.20 } else { 0.12 });
        let stroke = egui::Stroke::new(1.0, color.gamma_multiply(0.65));
        let hotkey_badge = self.settings.paste_next_hotkey_badge();
        let tip = if hidden_by_filter {
            format!(
                "Paste Next ({hotkey_badge}): {base_label} — marked entry is hidden by current filter (run still walks full history)"
            )
        } else {
            format!("Paste Next ({hotkey_badge}): {base_label} — click to jump to the NEXT entry")
        };

        let chip_resp = egui::Frame::new()
            .corner_radius(crate::theme::pill_rounding())
            .fill(bg)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(10.0_f32)
                        .color(color)
                        .strong(),
                );
            })
            .response;
        let click_resp = ui
            .interact(
                chip_resp.rect,
                ui.scope_id().with("paste_stack_mode_chip"),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip);
        if click_resp.clicked() && marker_visible {
            self.paste_stack.scroll_to_marker = true;
        }
    }

    fn render_header_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, is_dark: bool) {
        if ui
            .small_button(icons::button_icon(icons::X))
            .on_hover_text("Hide window (Esc)")
            .clicked()
        {
            self.hide_window(ctx, self.settings.restore_focus, false);
        }

        ui.add_space(2.0_f32);

        let quit_clicked = ui
            .scope(|ui| {
                let danger = crate::theme::error_color(is_dark);
                let visuals = &mut ui.style_mut().visuals.widgets;
                visuals.hovered.weak_bg_fill =
                    danger.gamma_multiply(if is_dark { 0.22 } else { 0.14 });
                visuals.hovered.bg_stroke = egui::Stroke::new(1.0, danger.gamma_multiply(0.55));
                visuals.hovered.fg_stroke = egui::Stroke::new(1.5, danger);
                visuals.active.weak_bg_fill =
                    danger.gamma_multiply(if is_dark { 0.32 } else { 0.22 });
                visuals.active.fg_stroke = egui::Stroke::new(1.5, danger);

                ui.small_button(icons::button_icon(icons::POWER))
                    .on_hover_text(crate::platform::quit_shortcut_tooltip())
                    .clicked()
            })
            .inner;
        if quit_clicked {
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
                .small_button(format!("{} + Secret", icons::LOCK))
                .on_hover_text(if cfg!(target_os = "macos") {
                    "New pinned secret (Cmd+N)"
                } else {
                    "New pinned secret (Ctrl+Shift+N)"
                })
                .clicked()
        {
            self.open_new_secret_dialog();
        }

        if self.active_view == ActiveView::History
            && ui
                .small_button(icons::button_icon(icons::TRASH))
                .on_hover_text("Clear clipboard history")
                .clicked()
        {
            self.dialogs.show_clear_confirm_dialog = true;
        }
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let is_focused = self.focus_search_input;
        egui::Frame::new()
            .fill(crate::theme::search_bg(is_dark))
            .stroke(crate::theme::search_stroke(is_dark, is_focused))
            .corner_radius(crate::theme::search_rounding())
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icons::MAGNIFYING_GLASS)
                            .color(crate::theme::text_secondary(is_dark))
                            .size(13.0_f32),
                    );
                    let trailing_width = 38.0_f32;
                    let edit_width = (ui.available_width() - trailing_width).max(60.0);
                    let search_box = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text(
                                egui::RichText::new(
                                    "Search history... (↑/↓ to select, ↵ to paste)",
                                )
                                .color(crate::theme::text_secondary(is_dark)),
                            )
                            .frame(egui::Frame::NONE)
                            .desired_width(edit_width),
                    );

                    if self.search_query.is_empty() {
                        egui::Frame::new()
                            .corner_radius(crate::theme::pill_rounding())
                            .fill(if is_dark {
                                egui::Color32::from_white_alpha(14)
                            } else {
                                egui::Color32::from_black_alpha(10)
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_dark {
                                    egui::Color32::from_white_alpha(22)
                                } else {
                                    egui::Color32::from_black_alpha(18)
                                },
                            ))
                            .inner_margin(egui::Margin::symmetric(5, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("ESC")
                                        .size(9.0_f32)
                                        .color(crate::theme::text_secondary(is_dark))
                                        .strong(),
                                )
                                .on_hover_text("Press ESC to dismiss window");
                            });
                    } else if ui
                        .small_button(icons::button_icon(icons::X))
                        .on_hover_text("Clear search (ESC)")
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
                    if !self.render_toast(ui, is_dark) {
                        self.render_storage_status(ui, is_dark);
                    }
                    Self::render_navigation_hint(ui, is_dark);
                });
            });
    }

    /// Renders the active toast in the status slot. Returns `true` whenever a toast
    /// was present (including the frame where it expires), so the caller skips the
    /// fallback status indicator exactly as before.
    #[allow(clippy::cast_precision_loss)]
    fn render_toast(&mut self, ui: &mut egui::Ui, is_dark: bool) -> bool {
        let Some(toast) = self.toast.as_ref() else {
            return false;
        };

        let elapsed = toast.shown_at.elapsed();
        if elapsed >= Duration::from_millis(2500) {
            self.toast = None;
            return true;
        }

        ui.ctx().request_repaint();
        let alpha_factor = if elapsed < Duration::from_millis(1800) {
            1.0_f32
        } else {
            (1.0_f32 - ((elapsed.as_millis() as f32) - 1800.0) / 700.0).clamp(0.0, 1.0)
        };

        let (base_bg, base_stroke, base_text) = Self::toast_colors(toast.severity, is_dark);
        let bg_color = base_bg.gamma_multiply(alpha_factor);
        let stroke_color = base_stroke.gamma_multiply(alpha_factor);
        let text_color = base_text.gamma_multiply(alpha_factor);

        egui::Frame::new()
            .corner_radius(crate::theme::pill_rounding())
            .fill(bg_color)
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&toast.message)
                        .size(11.0)
                        .color(text_color)
                        .strong(),
                );
            });
        true
    }

    /// Maps a severity to the toast background and accent colors.
    const fn toast_colors(
        severity: ToastSeverity,
        is_dark: bool,
    ) -> (egui::Color32, egui::Color32, egui::Color32) {
        let (background, accent) = match severity {
            ToastSeverity::Success => (
                if is_dark {
                    egui::Color32::from_rgba_premultiplied(18, 48, 28, 180)
                } else {
                    egui::Color32::from_rgba_premultiplied(235, 250, 240, 230)
                },
                crate::theme::success_color(is_dark),
            ),
            ToastSeverity::Warning => (
                if is_dark {
                    egui::Color32::from_rgba_premultiplied(48, 38, 18, 180)
                } else {
                    egui::Color32::from_rgba_premultiplied(254, 248, 235, 230)
                },
                crate::theme::warning_color(is_dark),
            ),
            ToastSeverity::Error => (
                if is_dark {
                    egui::Color32::from_rgba_premultiplied(48, 18, 20, 180)
                } else {
                    egui::Color32::from_rgba_premultiplied(254, 235, 238, 230)
                },
                crate::theme::error_color(is_dark),
            ),
        };
        (background, accent, accent)
    }

    /// Renders the key-storage status shown when no toast is active.
    fn render_storage_status(&self, ui: &mut egui::Ui, is_dark: bool) {
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
            crate::crypto::KeyStorageMode::OsKeyring => crate::theme::text_secondary(is_dark),
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

    /// Renders the right-aligned navigation/keyboard hint.
    fn render_navigation_hint(ui: &mut egui::Ui, is_dark: bool) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(crate::platform::navigation_hint_label())
                    .color(crate::theme::text_secondary(is_dark))
                    .size(10.5_f32),
            );
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
                    ContentFilter::Pinned => item.is_pinned,
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
                    .stroke(crate::theme::card_stroke(is_dark, false))
                    .shadow(crate::theme::card_shadow(is_dark, true)),
            )
    }

    fn render_clear_confirm_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.dialogs.show_clear_confirm_dialog {
            return;
        }

        if Self::render_modal_backdrop(ctx, "clear_confirm_backdrop") {
            self.dialogs.show_clear_confirm_dialog = false;
        }

        let mut open = self.dialogs.show_clear_confirm_dialog;
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
                        "This action cannot be undone. Unpinned items will be permanently erased. Pinned items are kept.",
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
                        self.dialogs.show_clear_confirm_dialog = false;
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
                                self.reset_paste_stack_to_newest();
                                self.dialogs.show_clear_confirm_dialog = false;
                                self.show_toast(&format!(
                                    "{} History cleared. Pinned items kept.",
                                    icons::CHECK
                                ));
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
                                self.reset_paste_stack_to_newest();
                                self.dialogs.show_clear_confirm_dialog = false;
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
            self.dialogs.show_clear_confirm_dialog = false;
        }
    }

    fn render_keyring_explanation_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.dialogs.show_keyring_explanation {
            return;
        }

        if Self::render_modal_backdrop(ctx, "keyring_explanation_backdrop") {
            // Allow dismissing by clicking the backdrop.
            self.dialogs.show_keyring_explanation = false;
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

        let mut open = self.dialogs.show_keyring_explanation;
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
                        self.dialogs.show_keyring_explanation = false;
                        self.settings.keyring_explanation_shown = true;
                        self.persist_settings();
                    }
                });
                ui.add_space(4.0_f32);
            });
        });

        if !open {
            self.dialogs.show_keyring_explanation = false;
            self.settings.keyring_explanation_shown = true;
            self.persist_settings();
        }
    }

    fn render_annotation_edit_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        let Some(edit_id) = self.dialogs.editing_annotation_id else {
            return;
        };

        if Self::render_modal_backdrop(ctx, "annotation_edit_backdrop") {
            self.dialogs.editing_annotation_id = None;
        }

        let mut open = true;
        Self::modal_window(
            format!("{} Edit Annotation", icons::PENCIL),
            &mut open,
            ctx,
            is_dark,
            360.0_f32,
            420.0_f32,
        )
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Title / Label")
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.dialogs.annotation_label_input)
                    .hint_text("e.g. GitHub Production PAT")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Notes")
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.dialogs.annotation_notes_input)
                    .hint_text("Optional notes, username, recovery info...")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(12.0);
            let save_shortcut = ui.input(|i| {
                i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
            });
            ui.horizontal(|ui| {
                let btn_width = 110.0_f32;
                let btn_height = 28.0_f32;
                let available_width = ui.available_width();
                let spacing = btn_width.mul_add(-2.0_f32, available_width).max(0.0_f32) / 2.0_f32;
                ui.add_space(spacing);

                if ui
                    .add_sized([btn_width, btn_height], egui::Button::new("Cancel (Esc)"))
                    .clicked()
                {
                    self.dialogs.editing_annotation_id = None;
                }

                let save_btn = egui::Button::new(format!("{} Save", icons::CHECK));
                let save_clicked = ui.add_sized([btn_width, btn_height], save_btn).clicked();
                if save_clicked || save_shortcut {
                    let label = self.dialogs.annotation_label_input.clone();
                    let notes = self.dialogs.annotation_notes_input.clone();
                    match self.db.set_item_annotation(
                        edit_id,
                        Some(label.as_str()),
                        Some(notes.as_str()),
                    ) {
                        Ok(()) => {
                            self.dialogs.editing_annotation_id = None;
                            self.refresh_items();
                            self.show_toast(&format!("{} Annotation saved.", icons::CHECK));
                        }
                        Err(error) => {
                            self.report_db_write_failure("save annotation", &error);
                        }
                    }
                }
            });
        });

        if !open {
            self.dialogs.editing_annotation_id = None;
        }
    }

    fn open_new_secret_dialog(&mut self) {
        self.dialogs.show_new_secret_dialog = true;
        self.dialogs.new_secret_label.clear();
        self.dialogs.new_secret_value = zeroize::Zeroizing::new(String::new());
        self.dialogs.new_secret_notes.clear();
        self.dialogs.new_secret_reveal = false;
    }

    fn open_new_secret_dialog_prefilled(&mut self, secret: String) {
        self.dialogs.show_new_secret_dialog = true;
        self.dialogs.new_secret_label.clear();
        self.dialogs.new_secret_value = zeroize::Zeroizing::new(secret);
        self.dialogs.new_secret_notes.clear();
        self.dialogs.new_secret_reveal = false;
    }

    fn close_new_secret_dialog(&mut self) {
        self.dialogs.show_new_secret_dialog = false;
        self.dialogs.new_secret_value = zeroize::Zeroizing::new(String::new());
        self.dialogs.new_secret_label.clear();
        self.dialogs.new_secret_notes.clear();
        self.dialogs.new_secret_reveal = false;
    }

    fn render_new_secret_modal(&mut self, ctx: &egui::Context, is_dark: bool) {
        if !self.dialogs.show_new_secret_dialog {
            return;
        }

        if Self::render_modal_backdrop(ctx, "new_secret_backdrop") {
            self.close_new_secret_dialog();
            return;
        }

        let mut open = self.dialogs.show_new_secret_dialog;
        Self::modal_window(
            format!("{} New Secret", icons::LOCK_KEY),
            &mut open,
            ctx,
            is_dark,
            400.0_f32,
            460.0_f32,
        )
        .show(ctx, |ui| {
            Self::render_new_secret_form_fields(ui, &mut self.dialogs, is_dark);
            ui.add_space(12.0);
            self.render_new_secret_actions(ui);
        });

        if !open {
            self.close_new_secret_dialog();
        }
    }

    fn render_new_secret_form_fields(ui: &mut egui::Ui, dialogs: &mut DialogState, is_dark: bool) {
        ui.label(
            egui::RichText::new("Label / Title")
                .color(crate::theme::text_primary(is_dark))
                .strong(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut dialogs.new_secret_label)
                .hint_text("e.g. AWS Root Password")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new("Secret Value")
                .color(crate::theme::text_primary(is_dark))
                .strong(),
        );
        ui.horizontal(|ui| {
            let edit = egui::TextEdit::singleline(&mut *dialogs.new_secret_value)
                .password(!dialogs.new_secret_reveal)
                .hint_text("Enter secret value")
                .desired_width(ui.available_width() - 34.0);
            ui.add(edit);
            let eye = if dialogs.new_secret_reveal {
                icons::EYE_SLASH
            } else {
                icons::EYE
            };
            if ui
                .small_button(icons::button_icon(eye))
                .on_hover_text(if dialogs.new_secret_reveal {
                    "Hide secret"
                } else {
                    "Reveal secret"
                })
                .clicked()
            {
                dialogs.new_secret_reveal = !dialogs.new_secret_reveal;
            }
        });
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new("Notes / Username (optional)")
                .color(crate::theme::text_primary(is_dark))
                .strong(),
        );
        ui.add(
            egui::TextEdit::multiline(&mut dialogs.new_secret_notes)
                .hint_text("Optional notes or username")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
    }

    fn render_new_secret_actions(&mut self, ui: &mut egui::Ui) {
        let can_save = !self.dialogs.new_secret_value.trim().is_empty();
        let save_shortcut = can_save
            && ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)
            });
        ui.horizontal(|ui| {
            let btn_width = 120.0_f32;
            let btn_height = 28.0_f32;
            let available_width = ui.available_width();
            let spacing = btn_width.mul_add(-2.0_f32, available_width).max(0.0_f32) / 2.0_f32;
            ui.add_space(spacing);

            if ui
                .add_sized([btn_width, btn_height], egui::Button::new("Cancel (Esc)"))
                .clicked()
            {
                self.close_new_secret_dialog();
                return;
            }

            let save_clicked = ui
                .add_enabled(
                    can_save,
                    egui::Button::new(format!("{} Save Secret", icons::LOCK))
                        .min_size(egui::vec2(btn_width, btn_height)),
                )
                .on_hover_text("Save directly as a pinned secret (without copying to clipboard)")
                .clicked();
            if save_clicked || save_shortcut {
                self.save_new_secret_from_dialog();
            }
        });
    }

    fn save_new_secret_from_dialog(&mut self) {
        let secret = self.dialogs.new_secret_value.clone();
        let label = self.dialogs.new_secret_label.clone();
        let notes = self.dialogs.new_secret_notes.clone();
        match self.db.insert_manual_secret(
            secret.as_str(),
            Some(label.as_str()),
            Some(notes.as_str()),
            true,
        ) {
            Ok(_) => {
                self.close_new_secret_dialog();
                self.refresh_items();
                self.show_toast(&format!("{} Secret saved & pinned.", icons::LOCK));
            }
            Err(error) => {
                self.report_db_write_failure("save pinned secret", &error);
            }
        }
    }
}

/// Pure dispatch decision for monitor notifications and search-query changes.
const fn monitor_update_needs_refresh(new_items_detected: bool, query_changed: bool) -> bool {
    new_items_detected || query_changed
}

/// Pure deadline decision for pending secret clipboard clears.
fn clipboard_clear_is_due(now: Instant, deadline: Instant) -> bool {
    now >= deadline
}

fn log_viewport_state_transitions(window_state: &WindowFocusState, ctx: &egui::Context) {
    static LAST_VIEWPORT_LOG_BITS: std::sync::atomic::AtomicU8 =
        std::sync::atomic::AtomicU8::new(u8::MAX);

    let (vp_visible, vp_minimized, vp_focused) = ctx.input(|i| {
        let vp = i.viewport();
        (vp.visible(), vp.minimized, vp.focused)
    });
    let bits = u8::from(window_state.is_visible)
        | (u8::from(vp_visible.unwrap_or(true)) << 1)
        | (u8::from(vp_minimized.unwrap_or(false)) << 2)
        | (u8::from(vp_focused.unwrap_or(false)) << 3)
        | (u8::from(window_state.was_inactive_since_hidden) << 4);

    let prev = LAST_VIEWPORT_LOG_BITS.swap(bits, std::sync::atomic::Ordering::AcqRel);
    if prev != bits {
        crate::logging::log_message(
            "INFO",
            &format!(
                "Viewport state changed: app_visible={}, vp_visible={:?}, minimized={:?}, focused={:?}, was_inactive={}",
                window_state.is_visible,
                vp_visible,
                vp_minimized,
                vp_focused,
                window_state.was_inactive_since_hidden,
            ),
        );
    }
}

impl eframe::App for ClipboardApp {
    /// On macOS, exit right after cleanup to dodge an `AppKit` Touch Bar observer crash.
    fn on_exit(&mut self) {
        if crate::platform::requires_immediate_exit_on_quit() && self.is_quitting {
            self.shutdown_and_exit();
        }
    }

    /// Runs before every `ui()` frame AND while the window is hidden (`Visible(false)`).
    /// In `eframe` 0.36+, `ui()` is skipped while the root viewport is invisible, so
    /// all wake-up, tray, hotkey, activation/reopen, monitor, and auto-clear handlers
    /// MUST live in `logic()`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        static FIRST_LOGIC_STARTED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        static FIRST_LOGIC_COMPLETED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        if !FIRST_LOGIC_STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
            crate::logging::log_message("INFO", "First ClipboardApp::logic() pass started");
        }

        if self.window_state.pending_order_front {
            self.window_state.pending_order_front = false;
            if self.window_state.is_visible {
                self.window_state.last_show_instant = Some(Instant::now());
                crate::platform::order_front_main_window();
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        self.handle_tray_events(ctx);
        self.handle_window_close_request(ctx);
        #[cfg(debug_assertions)]
        let before_hotkey_visible = self.window_state.is_visible;
        self.handle_hotkey_toggle(ctx);
        #[cfg(debug_assertions)]
        if self.window_state.is_visible != before_hotkey_visible {
            crate::logging::log_message(
                "INFO",
                &format!(
                    "Hotkey toggled visibility: {before_hotkey_visible} -> {}",
                    self.window_state.is_visible
                ),
            );
        }
        self.handle_app_activation(ctx);
        self.handle_monitor_updates();
        self.handle_paste_next_outcomes();
        self.handle_paste_next_events(ctx);
        self.handle_clipboard_auto_clear();
        self.handle_ocr_results(ctx);
        log_viewport_state_transitions(&self.window_state, ctx);

        if let Some((_, deadline)) = &self.pending_clipboard_clear {
            ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }

        // In eframe 0.36+, background threads calling ctx.request_repaint() wake
        // invisible/minimized windows directly via UserEvent::RequestRepaint.
        // Only tick briefly after hiding until the OS finishes deactivating our app
        // so `was_inactive_since_hidden` latches; afterwards sleep at 0% idle CPU.
        if !self.window_state.is_visible && !self.window_state.was_inactive_since_hidden {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        if !FIRST_LOGIC_COMPLETED.swap(true, std::sync::atomic::Ordering::AcqRel) {
            crate::logging::log_message("INFO", "First ClipboardApp::logic() pass completed");
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        static FIRST_UI_LOGGED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        if !FIRST_UI_LOGGED.swap(true, std::sync::atomic::Ordering::AcqRel) {
            crate::logging::log_message("INFO", "First ClipboardApp::ui() pass");
        }

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
        if !self.settings.keyring_explanation_shown && !self.dialogs.show_keyring_explanation {
            self.dialogs.show_keyring_explanation = true;
        }

        if !self.window_state.is_visible {
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
        self.render_annotation_edit_modal(&ctx, is_dark);
        self.render_new_secret_modal(&ctx, is_dark);
    }
}

impl ClipboardApp {
    /// Releases owned background resources in a deterministic, idempotent order.
    ///
    /// Both explicit shutdown and `Drop` use this path: clear any pending secret
    /// clipboard payload first, stop and join hotkey/monitor/paste workers, then
    /// remove the macOS reopen observer.
    fn shutdown_resources(&mut self) {
        self.clear_pending_secret_from_clipboard();
        self.services.shutdown();
        crate::platform::shutdown_paste_worker();
        crate::platform::teardown_reopen_observer();
    }

    /// Terminates the process right after cleanup on macOS.
    ///
    /// After the close request, winit's `NSApplication run` loop is still pumping and
    /// `AppKit`'s display cycle invalidates `_NSTouchBarFinderObservation` on the already
    /// torn-down `WinitView`, raising an uncaught `NSRangeException`. Exiting here, once
    /// our own cleanup finished, avoids that last `AppKit` flush.
    fn shutdown_and_exit(&mut self) -> ! {
        self.shutdown_resources();
        std::process::exit(0);
    }
}

impl Drop for ClipboardApp {
    fn drop(&mut self) {
        self.shutdown_resources();
    }
}

#[derive(Clone, Copy)]
enum NavStep {
    Clamp,
    Wrap,
}

#[derive(Default, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct HistoryNavInput {
    down: Option<NavStep>,
    up: Option<NavStep>,
    copy_selected: bool,
    copy_raw_selected: bool,
    delete_selected: bool,
    toggle_secret_selected: bool,
    edit_annotation_selected: bool,
    set_paste_next_marker_selected: bool,
    new_secret: bool,
    copy_shortcut_slot: Option<usize>,
}

fn consume_key_logical_or_physical(
    i: &mut egui::InputState,
    modifiers: egui::Modifiers,
    key: egui::Key,
) -> bool {
    let mut count = 0usize;
    i.events.retain(|event| {
        let is_match = matches!(
            event,
            egui::Event::Key {
                key: ev_key,
                physical_key,
                modifiers: ev_mods,
                pressed: true,
                ..
            } if (*ev_key == key || *physical_key == Some(key))
                && ev_mods.matches_logically(modifiers)
        );
        count += usize::from(is_match);
        !is_match
    });
    count > 0
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
            if consume_key_logical_or_physical(i, egui::Modifiers::COMMAND, k) {
                nav.copy_shortcut_slot = Some(idx);
                return;
            }
        }

        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::L) {
            nav.toggle_secret_selected = true;
        }

        if i.consume_key(egui::Modifiers::NONE, egui::Key::F2)
            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::E)
        {
            nav.edit_annotation_selected = true;
        }

        if i.consume_key(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::M,
        ) {
            nav.set_paste_next_marker_selected = true;
        }

        let new_secret_shortcut = if cfg!(target_os = "macos") {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)
        } else {
            i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N)
        };
        if new_secret_shortcut {
            nav.new_secret = true;
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

        if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter) {
            nav.copy_raw_selected = true;
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
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

pub(super) fn active_modifier_labels(modifiers: egui::Modifiers) -> Vec<&'static str> {
    // Note: in egui, `modifiers.command` aliases `ctrl` on Linux/Windows,
    // so only treat `mac_cmd` / `command` as Cmd on macOS.
    let mut parts = Vec::with_capacity(4);
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if crate::platform::has_recorded_command_modifier(modifiers) {
        parts.push("Cmd");
    }
    parts
}

fn format_recorded_hotkey(modifiers: egui::Modifiers, key: egui::Key) -> Option<String> {
    let key_name = egui_key_to_hotkey_name(key)?;
    let mut parts = active_modifier_labels(modifiers);
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
    use super::{
        clipboard_clear_is_due, monitor_update_needs_refresh, AppServices, ClipboardApp,
        DialogState, PasteStackState, PastedReconciliation, StackMode, ToastSeverity,
        WindowFocusState,
    };
    use eframe::egui;
    use std::time::{Duration, Instant};

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
        let (ocr_tx, ocr_rx) = std::sync::mpsc::channel();
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
            services: AppServices {
                notify_rx,
                hotkey_rx,
                paste_next_rx: crossbeam_channel::bounded(64).1,
                hotkey_service: None,
                tray_rx,
                tray_service: None,
                monitor_shutdown_tx: shutdown_tx,
                monitor_handle: None,
                ocr_rx,
                ocr_tx,
            },
            texture_cache: super::ImageTextureCache::new(),
            window_state: WindowFocusState {
                is_visible: true,
                ..Default::default()
            },
            selected_index: 0,
            focus_search_input: true,
            settings: crate::settings::AppSettings::default(),
            active_view: super::ActiveView::History,
            password_gen: crate::password::PasswordGeneratorState::default(),
            revealed_secrets: std::collections::HashSet::new(),
            pending_clipboard_clear: None,
            dialogs: DialogState::default(),
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            is_quitting: false,
            copied_item_pulse: None,
            paste_stack: PasteStackState::default(),
            data_dir: dir.path().to_path_buf(),
        };

        assert!(!app.is_quitting);

        // Verify TrayAction::Show restores visibility and surfaces any pending toast
        app.window_state.is_visible = false;
        app.pending_toast = Some(("Auto-paste blocked".to_string(), ToastSeverity::Warning));
        tray_tx.send(super::TrayAction::Show).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.window_state.is_visible);
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
        app.window_state.is_visible = false;
        tray_tx.send(super::TrayAction::OpenSettings).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.window_state.is_visible);
        assert_eq!(app.active_view, super::ActiveView::Settings);

        // Verify TrayAction::Quit sets is_quitting and wins over a simultaneous Close event
        tray_tx.send(super::TrayAction::Quit).unwrap();
        let mut raw_input = egui::RawInput::default();
        raw_input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let logic_output = ctx.run_logic(&raw_input, |logic_ctx| {
            app.handle_tray_events(logic_ctx);
            app.handle_window_close_request(logic_ctx);
        });
        assert!(app.is_quitting);
        let root_cmds = logic_output
            .viewport_commands
            .get(&egui::ViewportId::ROOT)
            .map_or(&[][..], Vec::as_slice);
        assert!(root_cmds.contains(&egui::ViewportCommand::Close));
        assert!(!root_cmds.contains(&egui::ViewportCommand::CancelClose));
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

    #[test]
    fn test_copy_plain_text_sets_toast() {
        let (shutdown_tx, _shutdown_rx) = std::sync::mpsc::channel();
        let (_notify_tx, notify_rx) = std::sync::mpsc::channel();
        let (_hotkey_tx, hotkey_rx) = std::sync::mpsc::channel();
        let (_tray_tx, tray_rx) = std::sync::mpsc::channel();
        let (ocr_tx, ocr_rx) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("copy_test.db");
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
            services: AppServices {
                notify_rx,
                hotkey_rx,
                paste_next_rx: crossbeam_channel::bounded(64).1,
                hotkey_service: None,
                tray_rx,
                tray_service: None,
                monitor_shutdown_tx: shutdown_tx,
                monitor_handle: None,
                ocr_rx,
                ocr_tx,
            },
            texture_cache: super::ImageTextureCache::new(),
            window_state: WindowFocusState {
                is_visible: true,
                ..Default::default()
            },
            selected_index: 0,
            focus_search_input: true,
            settings: crate::settings::AppSettings::default(),
            active_view: super::ActiveView::Settings,
            password_gen: crate::password::PasswordGeneratorState::default(),
            revealed_secrets: std::collections::HashSet::new(),
            pending_clipboard_clear: None,
            dialogs: DialogState::default(),
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            is_quitting: false,
            copied_item_pulse: None,
            paste_stack: PasteStackState::default(),
            data_dir: dir.path().to_path_buf(),
        };

        let ctx = egui::Context::default();
        app.copy_plain_text("test_log_path", "Copied test toast", &ctx);
        assert!(app.toast.is_some());
    }

    pub(super) fn test_app_with_channels(
    ) -> (ClipboardApp, std::sync::mpsc::Sender<()>, tempfile::TempDir) {
        let (shutdown_tx, _shutdown_rx) = std::sync::mpsc::channel();
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();
        let (_hotkey_tx, hotkey_rx) = std::sync::mpsc::channel();
        let (_tray_tx, tray_rx) = std::sync::mpsc::channel();
        let (ocr_tx, ocr_rx) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("contract_test.db");
        let key = format!("{:016x}", 3_u64).repeat(4);
        let db = crate::db::Database::open(&db_path, &key).unwrap();

        let app = ClipboardApp {
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
            services: AppServices {
                notify_rx,
                hotkey_rx,
                paste_next_rx: crossbeam_channel::bounded(64).1,
                hotkey_service: None,
                tray_rx,
                tray_service: None,
                monitor_shutdown_tx: shutdown_tx,
                monitor_handle: None,
                ocr_rx,
                ocr_tx,
            },
            texture_cache: super::ImageTextureCache::new(),
            window_state: WindowFocusState {
                is_visible: true,
                ..Default::default()
            },
            selected_index: 0,
            focus_search_input: true,
            settings: crate::settings::AppSettings::default(),
            active_view: super::ActiveView::History,
            password_gen: crate::password::PasswordGeneratorState::default(),
            revealed_secrets: std::collections::HashSet::new(),
            pending_clipboard_clear: None,
            dialogs: DialogState::default(),
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            is_quitting: false,
            copied_item_pulse: None,
            paste_stack: PasteStackState::default(),
            data_dir: dir.path().to_path_buf(),
        };

        (app, notify_tx, dir)
    }

    #[test]
    fn test_monitor_update_needs_refresh_helper() {
        assert!(!monitor_update_needs_refresh(false, false));
        assert!(monitor_update_needs_refresh(true, false));
        assert!(monitor_update_needs_refresh(false, true));
        assert!(monitor_update_needs_refresh(true, true));
    }

    #[test]
    fn test_clipboard_clear_is_due_helper() {
        let now = Instant::now();
        assert!(!clipboard_clear_is_due(now, now + Duration::from_secs(1)));
        assert!(clipboard_clear_is_due(now + Duration::from_secs(1), now));
        assert!(clipboard_clear_is_due(now, now));
    }

    #[test]
    fn test_handle_monitor_updates_dispatches_on_notification_and_query_change() {
        let (mut app, notify_tx, _dir) = test_app_with_channels();

        app.selected_index = 3;
        notify_tx.send(()).unwrap();
        app.handle_monitor_updates();
        assert_eq!(app.selected_index, 0);
        assert!(app.scroll_to_selected);
        assert_eq!(app.last_query_cached, String::new());

        // With no new signal, handler state is left untouched.
        app.selected_index = 4;
        app.scroll_to_selected = false;
        app.handle_monitor_updates();
        assert_eq!(app.selected_index, 4);
        assert!(!app.scroll_to_selected);

        app.search_query = "changed".to_string();
        app.handle_monitor_updates();
        assert_eq!(app.selected_index, 0);
        assert!(app.scroll_to_selected);
        assert_eq!(app.last_query_cached, "changed");
    }

    #[test]
    fn test_handle_clipboard_auto_clear_dispatches_only_at_deadline() {
        let (mut app, _notify_tx, _dir) = test_app_with_channels();
        let secret = format!("{:016x}", 7_u64).repeat(4);

        app.pending_clipboard_clear = Some((
            zeroize::Zeroizing::new(secret.clone()),
            Instant::now() + Duration::from_secs(60),
        ));
        app.handle_clipboard_auto_clear();
        assert!(app.pending_clipboard_clear.is_some());

        app.pending_clipboard_clear = Some((
            zeroize::Zeroizing::new(secret),
            Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
        ));
        app.handle_clipboard_auto_clear();
        assert!(app.pending_clipboard_clear.is_none());
    }

    #[test]
    fn test_read_history_nav_input_consumes_cmd_l_for_toggle_secret() {
        let ctx = egui::Context::default();
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::L,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            });
        });
        let nav = super::read_history_nav_input(&ctx);
        assert!(nav.toggle_secret_selected);
    }

    #[test]
    fn test_read_history_nav_input_consumes_shift_enter_for_copy_raw() {
        let ctx = egui::Context::default();
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            });
        });
        let nav = super::read_history_nav_input(&ctx);
        assert!(nav.copy_raw_selected);
        assert!(!nav.copy_selected);
    }

    #[test]
    fn test_should_hide_on_blur_guardrails() {
        use super::should_hide_on_blur;

        // In show grace period: MUST NEVER hide on blur
        assert!(!should_hide_on_blur(true, true, true, false, false, true));

        // Window has not yet been focused: MUST NOT hide on blur
        assert!(!should_hide_on_blur(false, true, false, false, false, true));

        // Setting disabled: MUST NOT hide on blur
        assert!(!should_hide_on_blur(true, false, false, false, false, true));

        // Minimized: MUST NOT hide on blur
        assert!(!should_hide_on_blur(true, true, false, true, false, true));

        // Dragging or moving: MUST NOT hide on blur
        assert!(!should_hide_on_blur(true, true, false, false, true, true));

        // Not outside (still active/focused): MUST NOT hide
        assert!(!should_hide_on_blur(true, true, false, false, false, false));

        // All conditions met: hides cleanly
        assert!(should_hide_on_blur(true, true, false, false, false, true));
    }

    #[test]
    fn test_should_reopen_on_focus_guardrails() {
        use super::should_reopen_on_focus;

        // Has not yet observed inactivity after hide: MUST NOT reopen (even if focused)
        assert!(!should_reopen_on_focus(
            false,
            Some(Duration::from_millis(10)),
            false,
            true,
            false
        ));

        // Still inactive and unfocused: MUST NOT reopen
        assert!(!should_reopen_on_focus(
            true,
            Some(Duration::from_millis(500)),
            false,
            false,
            false
        ));

        // Explicit OS focus on non-minimized window reopens immediately even <350ms
        assert!(should_reopen_on_focus(
            true,
            Some(Duration::from_millis(80)),
            false,
            true,
            false
        ));

        // Activation-only (without viewport focus) still respects 350ms cooldown
        assert!(!should_reopen_on_focus(
            true,
            Some(Duration::from_millis(80)),
            true,
            false,
            false
        ));
        assert!(should_reopen_on_focus(
            true,
            Some(Duration::from_millis(400)),
            true,
            false,
            false
        ));

        // Focused while still reporting minimized respects cooldown
        assert!(!should_reopen_on_focus(
            true,
            Some(Duration::from_millis(80)),
            true,
            true,
            true
        ));
    }

    #[test]
    fn test_handle_tray_events_processes_queued_actions_sequentially() {
        let ctx = egui::Context::default();
        let (shutdown_tx, _shutdown_rx) = std::sync::mpsc::channel();
        let (_notify_tx, notify_rx) = std::sync::mpsc::channel();
        let (_hotkey_tx, hotkey_rx) = std::sync::mpsc::channel();
        let (tray_tx, tray_rx) = std::sync::mpsc::channel();
        let (ocr_tx, ocr_rx) = std::sync::mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("tray_seq_test.db");
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
            services: AppServices {
                notify_rx,
                hotkey_rx,
                paste_next_rx: crossbeam_channel::bounded(64).1,
                hotkey_service: None,
                tray_rx,
                tray_service: None,
                monitor_shutdown_tx: shutdown_tx,
                monitor_handle: None,
                ocr_rx,
                ocr_tx,
            },
            texture_cache: super::ImageTextureCache::new(),
            window_state: WindowFocusState {
                is_visible: false,
                ..WindowFocusState::default()
            },
            selected_index: 0,
            focus_search_input: false,
            settings: crate::settings::AppSettings::default(),
            active_view: super::ActiveView::History,
            password_gen: crate::password::PasswordGeneratorState::default(),
            revealed_secrets: std::collections::HashSet::new(),
            pending_clipboard_clear: None,
            dialogs: super::DialogState::default(),
            scroll_to_selected: false,
            current_is_dark: None,
            title_bar_dark_mode: None,
            is_quitting: false,
            copied_item_pulse: None,
            paste_stack: PasteStackState::default(),
            data_dir: dir.path().to_path_buf(),
        };

        // Enqueue OpenSettings followed by Show: both must execute so OpenSettings is not dropped.
        tray_tx.send(crate::tray::TrayAction::OpenSettings).unwrap();
        tray_tx.send(crate::tray::TrayAction::Show).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.window_state.is_visible);
        assert!(matches!(app.active_view, super::ActiveView::Settings));
        assert!(!app.is_quitting);

        // Enqueue Quit followed by Show: Quit stops draining and exits cleanly.
        app.window_state.is_visible = false;
        tray_tx.send(crate::tray::TrayAction::Quit).unwrap();
        tray_tx.send(crate::tray::TrayAction::Show).unwrap();
        app.handle_tray_events(&ctx);
        assert!(app.is_quitting);
        assert!(!app.window_state.is_visible);
    }

    #[test]
    fn test_paste_stack_lifo_and_fifo_transitions() {
        // History newest-first: [C=3, B=2, A=1]
        let ids = [3_i64, 2, 1];
        let mut stack = PasteStackState::default();
        stack.reset_to_newest(&ids);
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(3));

        // LIFO: C -> B -> A -> None (AC-3, AC-13)
        for (expected_id, expected_next) in [(3, Some(2)), (2, Some(1)), (1, None)] {
            assert_eq!(stack.next_target_id(&ids), Some(expected_id));
            let seq = stack.record_submission(expected_id, None);
            assert!(matches!(
                stack.on_pasted(seq, &ids),
                PastedReconciliation::Reconciled { .. }
            ));
            assert_eq!(stack.mode, StackMode::Lifo);
            assert_eq!(stack.next_marker, expected_next);
        }
        assert_eq!(stack.next_target_id(&ids), None);

        // AC-23: Place marker on A (oldest) -> switches to FIFO, 3 presses paste A, B, C, then back to LIFO on C
        stack.place_marker(1, &ids);
        assert_eq!(stack.mode, StackMode::Fifo);
        assert_eq!(stack.next_marker, Some(1));

        let s1 = stack.record_submission(1, None);
        assert!(matches!(
            stack.on_pasted(s1, &ids),
            PastedReconciliation::Reconciled { .. }
        ));
        assert_eq!(stack.mode, StackMode::Fifo);
        assert_eq!(stack.next_marker, Some(2));

        let s2 = stack.record_submission(2, None);
        assert!(matches!(
            stack.on_pasted(s2, &ids),
            PastedReconciliation::Reconciled { .. }
        ));
        // Stepping onto newest entry stays FIFO until newest is actually pasted!
        assert_eq!(stack.mode, StackMode::Fifo);
        assert_eq!(stack.next_marker, Some(3));

        let s3 = stack.record_submission(3, None);
        assert!(matches!(
            stack.on_pasted(s3, &ids),
            PastedReconciliation::Reconciled { .. }
        ));
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(3));

        // Rule 5: manually placing marker on newest means LIFO
        stack.place_marker(1, &ids);
        assert_eq!(stack.mode, StackMode::Fifo);
        stack.place_marker(3, &ids);
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(3));
    }

    #[test]
    fn test_paste_stack_reset_failure_in_flight_and_deletions() {
        let ids = [3_i64, 2, 1];
        let mut stack = PasteStackState::default();

        // AC-24: new copy during FIFO resets to LIFO on new entry
        stack.place_marker(1, &ids);
        assert_eq!(stack.mode, StackMode::Fifo);
        let ids_with_d = [4_i64, 3, 2, 1];
        stack.reset_to_newest(&ids_with_d);
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(4));

        // AC-14: failure does not advance marker; queued job behind it still advances from its own submitted_id
        let s1 = stack.record_submission(4, None);
        assert_eq!(stack.next_target_id(&ids_with_d), Some(3));
        let s2 = stack.record_submission(3, None);
        assert!(stack.on_failed(s1));
        assert_eq!(stack.next_marker, Some(4));
        assert!(matches!(
            stack.on_pasted(s2, &ids_with_d),
            PastedReconciliation::Reconciled { .. }
        ));
        assert_eq!(stack.next_marker, Some(2));

        // In-flight burst in FIFO stops at newest entry
        stack.place_marker(2, &ids);
        let f1 = stack.record_submission(2, None);
        assert_eq!(stack.next_target_id(&ids), Some(3));
        let f2 = stack.record_submission(3, None);
        assert_eq!(stack.next_target_id(&ids), None);
        assert!(matches!(
            stack.on_pasted(f1, &ids),
            PastedReconciliation::Reconciled { .. }
        ));
        assert!(matches!(
            stack.on_pasted(f2, &ids),
            PastedReconciliation::Reconciled { .. }
        ));
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(3));

        // AC-26: marker follows its entry by id across non-clipboard inserts and deletions above it
        stack.place_marker(2, &ids);
        let ids_after_non_clip_insert = [5_i64, 3, 2, 1];
        assert_eq!(stack.next_target_id(&ids_after_non_clip_insert), Some(2));

        // Deleting an entry above the marker leaves marker on its id
        stack.on_entry_deleted(3, &ids, &[2, 1]);
        assert_eq!(stack.next_marker, Some(2));

        // Deleting the marked entry in FIFO moves to next newer; if none, switches to LIFO on newest
        stack.place_marker(2, &ids);
        stack.on_entry_deleted(2, &ids, &[3, 1]);
        assert_eq!(stack.mode, StackMode::Fifo);
        assert_eq!(stack.next_marker, Some(3));
        stack.on_entry_deleted(3, &[3, 1], &[1]);
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, Some(1));

        // Deleting the marked entry in LIFO moves to next older; if none, sets None
        stack.on_entry_deleted(1, &[1], &[]);
        assert_eq!(stack.mode, StackMode::Lifo);
        assert_eq!(stack.next_marker, None);
    }
}
