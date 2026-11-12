use crate::db::{ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::media;
use crate::settings::{AppSettings, HotkeyPreset};
use arboard::Clipboard;
use eframe::egui;
use egui_phosphor::regular as icons;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContentFilter {
    #[default]
    All,
    Text,
    Links,
    Code,
    Colors,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ActiveView {
    #[default]
    History,
    Settings,
}

enum ItemAction {
    None,
    CopyText(String),
    CopyFormattedJson(String),
    CopyColor(String, String),
    CopyImage(Vec<u8>),
    OpenBrowser(String),
    Delete(i64),
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClipboardApp {
    db: Database,
    search_query: String,
    content_filter: ContentFilter,
    items: Vec<ClipboardItem>,
    clipboard_writer: Option<Clipboard>,
    toast: Option<(String, Instant)>,
    last_query_cached: String,
    notify_rx: Receiver<()>,
    hotkey_rx: Receiver<()>,
    hotkey_service: Option<HotkeyService>,
    texture_cache: HashMap<i64, egui::TextureHandle>,
    is_visible: bool,
    has_been_focused: bool,
    was_inactive_since_hidden: bool,
    selected_index: usize,
    focus_search_input: bool,
    last_toggle_instant: Option<Instant>,
    settings: AppSettings,
    active_view: ActiveView,
}

impl ClipboardApp {
    pub(crate) fn new(
        _cc: &eframe::CreationContext<'_>,
        db: Database,
        notify_rx: Receiver<()>,
        hotkey_rx: Receiver<()>,
        hotkey_service: Option<HotkeyService>,
        settings: AppSettings,
    ) -> Self {
        let initial_items = db
            .get_recent(settings.max_history, None)
            .unwrap_or_default();
        let clipboard_writer = Clipboard::new().ok();

        Self {
            db,
            search_query: String::new(),
            content_filter: ContentFilter::All,
            items: initial_items,
            clipboard_writer,
            toast: None,
            last_query_cached: String::new(),
            notify_rx,
            hotkey_rx,
            hotkey_service,
            texture_cache: HashMap::new(),
            is_visible: true,
            has_been_focused: false,
            was_inactive_since_hidden: false,
            selected_index: 0,
            focus_search_input: true,
            last_toggle_instant: None,
            settings,
            active_view: ActiveView::History,
        }
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
    }

    fn show_toast(&mut self, message: &str) {
        self.toast = Some((message.to_string(), Instant::now()));
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.is_visible = true;
        self.has_been_focused = false;
        self.was_inactive_since_hidden = false;
        self.selected_index = 0;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        crate::platform::order_front_main_window();
        self.refresh_items();
        self.focus_search_input = true;
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context, restore_focus: bool, auto_paste: bool) {
        self.is_visible = false;
        self.has_been_focused = false;
        self.was_inactive_since_hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        crate::platform::hide_application();
        if restore_focus || auto_paste {
            crate::platform::restore_focus_and_paste(restore_focus, auto_paste);
        }
    }

    fn copy_text_to_clipboard(&mut self, content: &str, ctx: &egui::Context) {
        if let Some(cb) = &mut self.clipboard_writer {
            if let Err(e) = cb.set_text(content.to_string()) {
                eprintln!("Failed to write text to clipboard: {e}");
            } else {
                self.show_toast(&format!("{} Copied text to clipboard!", icons::CHECK));
                if self.settings.hide_on_copy {
                    self.hide_window(ctx, self.settings.restore_focus, self.settings.auto_paste);
                }
            }
        }
    }

    fn copy_image_to_clipboard(&mut self, png_bytes: &[u8], ctx: &egui::Context) {
        if let Some(cb) = &mut self.clipboard_writer {
            match media::png_to_rgba(png_bytes) {
                Ok((width, height, rgba)) => {
                    let img_data = arboard::ImageData {
                        width,
                        height,
                        bytes: std::borrow::Cow::Borrowed(&rgba),
                    };
                    if let Err(e) = cb.set_image(img_data) {
                        eprintln!("Failed to write image to clipboard: {e}");
                    } else {
                        self.show_toast(&format!("{} Copied image to clipboard!", icons::CHECK));
                        if self.settings.hide_on_copy {
                            self.hide_window(
                                ctx,
                                self.settings.restore_focus,
                                self.settings.auto_paste,
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to decode PNG image for clipboard copy: {e}");
                }
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
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.active_view == ActiveView::Settings {
                self.active_view = ActiveView::History;
            } else {
                self.hide_window(ctx, self.settings.restore_focus, false);
            }
        }
    }

    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        if self.active_view != ActiveView::History || !self.is_visible {
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
                ctx.request_repaint();
            }
        } else if wrap_down {
            self.selected_index = if self.selected_index + 1 < filtered_indices.len() {
                self.selected_index + 1
            } else {
                0
            };
            ctx.request_repaint();
        }

        if move_up {
            if self.selected_index > 0 {
                self.selected_index -= 1;
                ctx.request_repaint();
            }
        } else if wrap_up {
            self.selected_index = if self.selected_index > 0 {
                self.selected_index - 1
            } else {
                filtered_indices.len().saturating_sub(1)
            };
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
                _ => Some(ItemAction::CopyText(item.content.clone())),
            });
            match copy_action {
                Some(ItemAction::CopyImage(png)) => self.copy_image_to_clipboard(&png, ctx),
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
                self.texture_cache.remove(&id);
                self.refresh_items();
                let new_count = self.get_filtered_indices().len();
                if self.selected_index >= new_count && new_count > 0 {
                    self.selected_index = new_count - 1;
                }
                self.show_toast(&format!("{} Entry deleted.", icons::TRASH));
                ctx.request_repaint();
            }
        }
    }

    fn handle_app_activation(&mut self, ctx: &egui::Context) {
        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        let is_active = crate::platform::is_app_active();
        let reopen_requested = crate::platform::take_reopen_requested();

        if self.is_visible {
            if is_focused {
                self.has_been_focused = true;
            } else if self.has_been_focused && self.settings.hide_on_blur {
                self.hide_window(ctx, false, false);
            }
        } else {
            if !is_active && !is_focused {
                self.was_inactive_since_hidden = true;
            }

            // Reveal window if OS reopen was requested (Dock icon click),
            // or if the app was activated after being hidden (Cmd+Tab),
            // or if viewport focus was gained.
            if reopen_requested || (is_active && self.was_inactive_since_hidden) || is_focused {
                self.show_window(ctx);
            }
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx, false, false);
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
        }
    }

    fn render_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::panel_background())
                    .inner_margin(egui::Margin::symmetric(10.0_f32, 6.0_f32)),
            )
            .show(ctx, |ui| {
                ui.add_space(crate::theme::top_padding());
                self.render_header_row(ui, ctx);
                ui.add_space(6.0_f32);
                if self.active_view == ActiveView::History {
                    self.render_search_bar(ui);
                    ui.add_space(6.0_f32);
                    self.render_filter_tabs(ui);
                    ui.add_space(4.0_f32);
                }
            });
    }

    fn render_filter_tabs(&mut self, ui: &mut egui::Ui) {
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
        let img_count = self
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Image)
            .count();

        ui.horizontal(|ui| {
            let tabs = [
                (ContentFilter::All, icons::STACK, "All", all_count),
                (ContentFilter::Text, icons::FILE_TEXT, "Text", text_count),
                (ContentFilter::Links, icons::LINK, "Links", link_count),
                (ContentFilter::Code, icons::CODE, "Code", code_count),
                (ContentFilter::Colors, icons::PALETTE, "Colors", color_count),
                (ContentFilter::Image, icons::IMAGE, "Images", img_count),
            ];

            for (target, icon, label, count) in tabs {
                if crate::theme::render_segmented_tab(
                    ui,
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
        }
    }

    fn render_header_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.add_space(crate::theme::header_left_padding());

            let title_resp = ui.heading(
                egui::RichText::new(format!("{} Clipboard", icons::LOCK_KEY))
                    .color(egui::Color32::from_rgb(245, 245, 248))
                    .strong(),
            );
            if title_resp.interact(egui::Sense::drag()).dragged() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            let badge = self.settings.hotkey.badge();
            ui.label(
                egui::RichText::new(format!("({badge})"))
                    .color(egui::Color32::from_rgb(180, 180, 186))
                    .size(11.0_f32),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(format!("{} Hide (Esc)", icons::X))
                    .clicked()
                {
                    self.hide_window(ctx, self.settings.restore_focus, false);
                }

                let settings_btn = match self.active_view {
                    ActiveView::Settings => {
                        ui.small_button(format!("{} History", icons::CLOCK_COUNTER_CLOCKWISE))
                    }
                    ActiveView::History => ui.small_button(format!("{} Settings", icons::GEAR)),
                };
                if settings_btn.clicked() {
                    self.active_view = match self.active_view {
                        ActiveView::Settings => ActiveView::History,
                        ActiveView::History => ActiveView::Settings,
                    };
                }

                if self.active_view == ActiveView::History
                    && ui
                        .small_button(format!("{} Clear", icons::TRASH))
                        .on_hover_text("Permanently deletes all history")
                        .clicked()
                {
                    let _ = self.db.clear();
                    self.texture_cache.clear();
                    self.refresh_items();
                    self.show_toast(&format!("{} History cleared.", icons::CHECK));
                }
            });
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui) {
        let is_focused = self.focus_search_input;
        egui::Frame::none()
            .fill(crate::theme::search_bg())
            .stroke(crate::theme::search_stroke(is_focused))
            .rounding(crate::theme::search_rounding())
            .inner_margin(egui::Margin::symmetric(8.0_f32, 6.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icons::MAGNIFYING_GLASS)
                            .weak()
                            .size(13.0_f32),
                    );
                    let search_box = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text(
                                "Search clipboard history... (↑/↓ to select, Enter to paste)",
                            )
                            .frame(false)
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

    fn render_bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::panel_background())
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(58, 58, 62),
                    ))
                    .inner_margin(egui::Margin::symmetric(10.0_f32, 6.0_f32)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some((msg, time)) = &self.toast {
                        if time.elapsed() < Duration::from_secs(2) {
                            ui.label(
                                egui::RichText::new(msg)
                                    .color(egui::Color32::from_rgb(46, 204, 113))
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
                            .color(egui::Color32::from_rgb(180, 180, 186))
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
                                .color(egui::Color32::from_rgb(165, 165, 172))
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

    fn render_central_panel(&mut self, ctx: &egui::Context) {
        let filter = self.content_filter;
        let filtered_indices = self.get_filtered_indices();

        egui::CentralPanel::default().show(ctx, |ui| {
            if filtered_indices.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(35.0);
                    let (empty_icon, empty_hint) = match filter {
                        ContentFilter::All => (icons::CLIPBOARD, "No clipboard history found."),
                        ContentFilter::Text => (icons::FILE_TEXT, "No text entries found."),
                        ContentFilter::Links => {
                            (icons::LINK, "No links found in clipboard history.")
                        }
                        ContentFilter::Code => {
                            (icons::CODE, "No code snippets found in clipboard history.")
                        }
                        ContentFilter::Colors => {
                            (icons::PALETTE, "No colors found in clipboard history.")
                        }
                        ContentFilter::Image => (icons::IMAGE, "No image entries found."),
                    };
                    ui.label(egui::RichText::new(empty_icon).size(28.0).weak());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(empty_hint).weak());
                    ui.label(
                        egui::RichText::new(
                            "Copy text, links, or screenshots in any app to save here!",
                        )
                        .size(12.0)
                        .weak(),
                    );
                });
                return;
            }

            self.render_items_scroll_area(ui, &filtered_indices, ctx);
        });
    }

    fn render_items_scroll_area(
        &mut self,
        ui: &mut egui::Ui,
        filtered_indices: &[usize],
        ctx: &egui::Context,
    ) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut action = ItemAction::None;
                let mut clicked_slot: Option<usize> = None;

                for (slot, &idx) in filtered_indices.iter().enumerate() {
                    if let Some(item) = self.items.get(idx) {
                        let is_selected = slot == self.selected_index;
                        let shortcut_num = if slot < 9 { Some(slot + 1) } else { None };

                        let (item_action, clicked) = Self::render_clipboard_item(
                            ui,
                            item,
                            &mut self.texture_cache,
                            ctx,
                            is_selected,
                            shortcut_num,
                        );

                        if clicked {
                            clicked_slot = Some(slot);
                        }

                        if !matches!(item_action, ItemAction::None) {
                            action = item_action;
                        }
                    }
                    ui.add_space(4.0);
                }

                if let Some(slot) = clicked_slot {
                    self.selected_index = slot;
                }

                match action {
                    ItemAction::CopyText(content) => self.copy_text_to_clipboard(&content, ctx),
                    ItemAction::CopyFormattedJson(formatted) => {
                        self.copy_text_to_clipboard(&formatted, ctx);
                        self.show_toast(&format!(
                            "{} Copied formatted JSON to clipboard!",
                            icons::CHECK
                        ));
                    }
                    ItemAction::CopyColor(val, name) => {
                        self.copy_text_to_clipboard(&val, ctx);
                        self.show_toast(&format!("{} Copied {name}: {val}", icons::CHECK));
                    }
                    ItemAction::CopyImage(png_bytes) => {
                        self.copy_image_to_clipboard(&png_bytes, ctx);
                    }
                    ItemAction::OpenBrowser(url) => {
                        if let Err(e) = webbrowser::open(&url) {
                            eprintln!("Failed to open link in browser: {e}");
                            self.show_toast(&format!("{} Failed to open browser.", icons::WARNING));
                        } else {
                            self.show_toast(&format!(
                                "{} Opened in default browser!",
                                icons::ARROW_SQUARE_OUT
                            ));
                        }
                    }
                    ItemAction::Delete(id) => {
                        let _ = self.db.delete(id);
                        self.texture_cache.remove(&id);
                        self.refresh_items();
                        self.show_toast(&format!("{} Entry deleted.", icons::TRASH));
                    }
                    ItemAction::None => {}
                }
            });
    }

    fn render_settings_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(format!("{} Preferences", icons::GEAR));
                ui.add_space(8.0);

                self.render_hotkey_settings_group(ui);
                ui.add_space(10.0);

                self.render_behavior_settings_group(ui);
                ui.add_space(10.0);

                self.render_security_settings_group(ui);
                ui.add_space(12.0);

                if ui
                    .button(format!("{} Back to History", icons::ARROW_LEFT))
                    .clicked()
                {
                    self.active_view = ActiveView::History;
                }
            });
        });
    }

    fn render_hotkey_settings_group(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new(format!("{} Global Shortcut", icons::KEYBOARD)).strong());
            ui.label(
                egui::RichText::new(
                    "Select the keyboard shortcut used to summon the clipboard manager from anywhere.",
                )
                .weak()
                .size(11.0),
            );
            ui.add_space(6.0);

            let current_preset = self.settings.hotkey;
            for &preset in HotkeyPreset::all_presets() {
                let is_selected = current_preset == preset;
                let mut selected = is_selected;
                if ui.radio_value(&mut selected, true, preset.display_name()).clicked()
                    && !is_selected
                {
                    self.settings.hotkey = preset;
                    if let Err(e) = self.settings.save(&self.db) {
                        eprintln!("Failed to save settings: {e}");
                    }
                    if let Some(service) = &mut self.hotkey_service {
                        service.update_hotkeys(&preset.to_hotkeys());
                    }
                    self.show_toast(&format!(
                        "{} Shortcut set to {}",
                        icons::CHECK,
                        preset.badge()
                    ));
                }
            }
        });
    }

    fn render_behavior_settings_group(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new(format!("{} General Behavior", icons::SLIDERS)).strong());
            ui.add_space(4.0);

            let mut hide_on_copy = self.settings.hide_on_copy;
            if ui
                .checkbox(&mut hide_on_copy, "Close window immediately after copying")
                .changed()
            {
                self.settings.hide_on_copy = hide_on_copy;
                let _ = self.settings.save(&self.db);
            }

            let mut hide_on_blur = self.settings.hide_on_blur;
            if ui
                .checkbox(&mut hide_on_blur, "Dismiss window when clicking outside")
                .changed()
            {
                self.settings.hide_on_blur = hide_on_blur;
                let _ = self.settings.save(&self.db);
            }

            let mut restore_focus = self.settings.restore_focus;
            if ui
                .checkbox(
                    &mut restore_focus,
                    "Restore focus to initial window after copying or hiding",
                )
                .changed()
            {
                self.settings.restore_focus = restore_focus;
                let _ = self.settings.save(&self.db);
            }

            let mut auto_paste = self.settings.auto_paste;
            if ui
                .checkbox(
                    &mut auto_paste,
                    "Auto-paste selected entry directly into initial window",
                )
                .changed()
            {
                self.settings.auto_paste = auto_paste;
                let _ = self.settings.save(&self.db);
            }

            #[cfg(target_os = "macos")]
            if auto_paste {
                ui.add_space(2.0);
                if !crate::platform::is_accessibility_trusted() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} macOS note: Direct auto-paste requires Accessibility permissions in System Settings > Privacy & Security > Accessibility.\nNote: When running via terminal, grant permission to your terminal app as well.",
                            icons::WARNING
                        ))
                        .size(11.0)
                        .color(egui::Color32::from_rgb(230, 126, 34)),
                    );
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(format!("{} Prompt Permission", icons::SHIELD_CHECK))
                            .clicked()
                        {
                            crate::platform::prompt_accessibility_permission();
                        }
                        if ui
                            .small_button(format!("{} Open System Settings", icons::ARROW_SQUARE_OUT))
                            .clicked()
                        {
                            crate::platform::open_accessibility_settings();
                        }
                    });
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("History size limit:");
                let mut max_hist = self.settings.max_history;
                egui::ComboBox::from_id_salt("max_history_combo")
                    .selected_text(format!("{max_hist} items"))
                    .show_ui(ui, |ui| {
                        for limit in [50, 100, 200, 500, 1000] {
                            if ui
                                .selectable_value(&mut max_hist, limit, format!("{limit} items"))
                                .clicked()
                            {
                                self.settings.max_history = limit;
                                let _ = self.settings.save(&self.db);
                                self.refresh_items();
                            }
                        }
                    });
            });
        });
    }

    fn render_security_settings_group(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                egui::RichText::new(format!("{} Security & Encryption", icons::SHIELD_CHECK))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Your clipboard history is encrypted at rest using SQLCipher (256-bit AES-GCM).\nSettings and history are strictly stored on your local machine.",
                )
                .weak()
                .size(11.0),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Clear All History", icons::TRASH))
                    .clicked()
                {
                    let _ = self.db.clear();
                    self.texture_cache.clear();
                    self.refresh_items();
                    self.show_toast(&format!(
                        "{} All clipboard history wiped.",
                        icons::CHECK
                    ));
                }
            });
        });
    }

    fn render_clipboard_item(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        texture_cache: &mut HashMap<i64, egui::TextureHandle>,
        ctx: &egui::Context,
        is_selected: bool,
        shortcut_num: Option<usize>,
    ) -> (ItemAction, bool) {
        let mut action = ItemAction::None;

        let stroke = crate::theme::card_stroke(is_selected);
        let fill = crate::theme::card_fill(is_selected);
        let rounding = crate::theme::card_rounding();

        let frame_resp = egui::Frame::group(ui.style())
            .rounding(rounding)
            .stroke(stroke)
            .fill(fill)
            .inner_margin(8.0_f32)
            .show(ui, |ui| {
                let header_action = Self::render_item_header(ui, item, shortcut_num, is_selected);
                if !matches!(header_action, ItemAction::None) {
                    action = header_action;
                }

                ui.add_space(2.0);

                let content_action = match item.kind {
                    ItemKind::Text => Self::render_text_content(ui, &item.content),
                    ItemKind::Url => Self::render_url_content(ui, &item.content),
                    ItemKind::Code => Self::render_code_content(ui, item),
                    ItemKind::Color => Self::render_color_content(ui, item),
                    ItemKind::Image => Self::render_image_content(ui, item, texture_cache, ctx),
                };

                if !matches!(content_action, ItemAction::None) {
                    action = content_action;
                }

                if item.kind == ItemKind::Text || item.kind == ItemKind::Code {
                    let embedded = item.embedded_colors();
                    if !embedded.is_empty() {
                        ui.add_space(4.0);
                        let color_action = Self::render_embedded_colors(ui, &embedded);
                        if !matches!(color_action, ItemAction::None) {
                            action = color_action;
                        }
                    }
                }
            });

        if is_selected {
            frame_resp.response.scroll_to_me(Some(egui::Align::Center));
        }

        let clicked = frame_resp.response.clicked();
        (action, clicked)
    }

    fn render_text_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        let embedded = item.embedded_colors();
        let color_hint = match embedded.len() {
            0 => String::new(),
            1 => format!(" • {} 1 color", icons::PALETTE),
            n => format!(" • {} {n} colors", icons::PALETTE),
        };
        crate::theme::render_pill_tag(
            ui,
            icons::FILE_TEXT,
            "Text",
            egui::Color32::from_rgb(220, 220, 225),
            egui::Color32::from_rgb(45, 45, 48),
            egui::Color32::from_rgb(70, 70, 74),
        );
        ui.label(
            egui::RichText::new(format!("{color_hint} • {}", item.created_at))
                .size(10.0_f32)
                .color(egui::Color32::from_rgb(175, 175, 182)),
        );
    }

    fn render_url_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        let host = item.url_host().unwrap_or_else(|| "link".to_string());
        crate::theme::render_pill_tag(
            ui,
            icons::LINK,
            "Link",
            egui::Color32::from_rgb(100, 190, 255),
            egui::Color32::from_rgb(18, 44, 75),
            egui::Color32::from_rgb(30, 90, 160),
        );
        ui.label(
            egui::RichText::new(format!("• {host} • {}", item.created_at))
                .size(10.0_f32)
                .color(egui::Color32::from_rgb(175, 175, 182)),
        );
    }

    fn render_code_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        let (icon, lang_name) = item
            .code_language()
            .map_or((icons::CODE, "Code"), |l| (l.icon(), l.name()));
        let line_count = item.content.lines().count();
        let embedded = item.embedded_colors();
        let color_hint = match embedded.len() {
            0 => String::new(),
            1 => format!(" • {} 1 color", icons::PALETTE),
            n => format!(" • {} {n} colors", icons::PALETTE),
        };
        crate::theme::render_pill_tag(
            ui,
            icon,
            lang_name,
            egui::Color32::from_rgb(255, 130, 120),
            egui::Color32::from_rgb(58, 26, 26),
            egui::Color32::from_rgb(130, 48, 48),
        );
        ui.label(
            egui::RichText::new(format!(
                "• {line_count} lines{color_hint} • {}",
                item.created_at
            ))
            .size(10.0_f32)
            .color(egui::Color32::from_rgb(175, 175, 182)),
        );
    }

    fn render_color_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        let hex = item
            .parsed_color()
            .map_or_else(|| item.content.clone(), crate::color::ParsedColor::to_hex);
        crate::theme::render_pill_tag(
            ui,
            icons::PALETTE,
            "Color",
            egui::Color32::from_rgb(220, 150, 255),
            egui::Color32::from_rgb(46, 24, 58),
            egui::Color32::from_rgb(110, 50, 140),
        );
        ui.label(
            egui::RichText::new(format!("• {hex} • {}", item.created_at))
                .size(10.0_f32)
                .color(egui::Color32::from_rgb(175, 175, 182)),
        );
    }

    fn render_image_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        let dims = match (item.width, item.height) {
            (Some(w), Some(h)) => format!("Image ({w}x{h})"),
            _ => "Image".to_string(),
        };
        crate::theme::render_pill_tag(
            ui,
            icons::IMAGE,
            &dims,
            egui::Color32::from_rgb(110, 230, 145),
            egui::Color32::from_rgb(20, 48, 30),
            egui::Color32::from_rgb(40, 110, 65),
        );
        ui.label(
            egui::RichText::new(format!("• {}", item.created_at))
                .size(10.0_f32)
                .color(egui::Color32::from_rgb(175, 175, 182)),
        );
    }

    fn render_item_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        match item.kind {
            ItemKind::Text => Self::render_text_badge(ui, item),
            ItemKind::Url => Self::render_url_badge(ui, item),
            ItemKind::Code => Self::render_code_badge(ui, item),
            ItemKind::Color => Self::render_color_badge(ui, item),
            ItemKind::Image => Self::render_image_badge(ui, item),
        }
    }

    fn render_item_header(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        shortcut_num: Option<usize>,
        is_selected: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        ui.horizontal(|ui| {
            Self::render_item_badge(ui, item);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(num) = shortcut_num {
                    #[cfg(target_os = "macos")]
                    let shortcut_str = format!("Cmd+{num}");
                    #[cfg(not(target_os = "macos"))]
                    let shortcut_str = format!("Ctrl+{num}");

                    let (text_c, bg_c, stroke_c) = if is_selected {
                        (
                            egui::Color32::WHITE,
                            crate::theme::accent_color(),
                            crate::theme::accent_color(),
                        )
                    } else {
                        (
                            egui::Color32::from_rgb(210, 210, 215),
                            egui::Color32::from_rgb(46, 46, 50),
                            egui::Color32::from_rgb(70, 70, 75),
                        )
                    };

                    egui::Frame::none()
                        .rounding(crate::theme::pill_rounding())
                        .fill(bg_c)
                        .stroke(egui::Stroke::new(1.0_f32, stroke_c))
                        .inner_margin(egui::Margin::symmetric(5.0_f32, 1.5_f32))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(shortcut_str)
                                    .size(9.5_f32)
                                    .color(text_c)
                                    .strong(),
                            );
                        });
                }

                if ui
                    .small_button(icons::TRASH)
                    .on_hover_text("Delete entry (Delete or Cmd+Backspace)")
                    .clicked()
                {
                    action = ItemAction::Delete(item.id);
                }
                if ui
                    .small_button(format!("{} Copy", icons::COPY))
                    .on_hover_text("Copy back to clipboard (Enter)")
                    .clicked()
                {
                    match item.kind {
                        ItemKind::Text | ItemKind::Url | ItemKind::Code | ItemKind::Color => {
                            action = ItemAction::CopyText(item.content.clone());
                        }
                        ItemKind::Image => {
                            if let Some(png_bytes) = &item.image_data {
                                action = ItemAction::CopyImage(png_bytes.clone());
                            }
                        }
                    }
                }
                if item.kind == ItemKind::Url
                    && ui
                        .small_button(format!("{} Open", icons::ARROW_SQUARE_OUT))
                        .on_hover_text("Open link in default browser")
                        .clicked()
                {
                    action = ItemAction::OpenBrowser(item.content.trim().to_string());
                }
                if item.code_language() == Some(crate::code::CodeLanguage::Json) {
                    if let Some(pretty) = crate::code::format_json(&item.content) {
                        if ui
                            .small_button(format!("{} Prettify", icons::BRACKETS_CURLY))
                            .on_hover_text("Copy formatted JSON with indentation")
                            .clicked()
                        {
                            action = ItemAction::CopyFormattedJson(pretty);
                        }
                    }
                }
            });
        });

        action
    }

    fn render_code_content(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        let lang = item
            .code_language()
            .unwrap_or(crate::code::CodeLanguage::Rust);

        let mut action = ItemAction::None;

        egui::Frame::canvas(ui.style())
            .rounding(4.0)
            .fill(egui::Color32::from_rgb(26, 27, 34))
            .inner_margin(6.0)
            .show(ui, |ui| {
                let preview: String = item
                    .content
                    .lines()
                    .take(8)
                    .collect::<Vec<&str>>()
                    .join("\n");
                let has_more = item.content.lines().count() > 8;
                let display_text = if has_more {
                    format!("{preview}\n// ... (more lines)")
                } else {
                    preview
                };

                let job = crate::code::highlight_code(ui.ctx(), ui.style(), &display_text, lang);
                let label = ui.add(egui::Label::new(job).sense(egui::Sense::click()));

                if label.clicked() {
                    action = ItemAction::CopyText(item.content.clone());
                } else if label.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
            });

        action
    }

    fn render_color_content(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        let Some(parsed) = item.parsed_color() else {
            return Self::render_text_content(ui, &item.content);
        };

        let mut action = ItemAction::None;

        ui.horizontal(|ui| {
            let response = Self::render_color_swatch(ui, parsed);
            if response.clicked() {
                action = ItemAction::CopyText(item.content.clone());
            } else if response.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            }

            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let hex_label = ui.add(
                        egui::Label::new(egui::RichText::new(parsed.to_hex()).monospace().strong())
                            .sense(egui::Sense::click()),
                    );
                    if hex_label.clicked() {
                        action = ItemAction::CopyColor(parsed.to_hex(), "HEX".to_string());
                    } else if hex_label.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }

                    let opacity_pct = u32::from(parsed.a) * 100 / 255;
                    let opacity_note = if parsed.a < 255 {
                        format!(" • {opacity_pct}% alpha")
                    } else {
                        String::new()
                    };

                    ui.label(
                        egui::RichText::new(format!("{}{}", parsed.to_rgb_string(), opacity_note))
                            .monospace()
                            .size(11.0)
                            .weak(),
                    );
                });

                ui.horizontal(|ui| {
                    if ui
                        .small_button("# HEX")
                        .on_hover_text("Copy HEX code (RGBA order)")
                        .clicked()
                    {
                        action = ItemAction::CopyColor(parsed.to_hex(), "HEX".to_string());
                    }
                    if ui
                        .small_button("RGB")
                        .on_hover_text("Copy RGB / RGBA values")
                        .clicked()
                    {
                        action = ItemAction::CopyColor(parsed.to_rgb_string(), "RGB".to_string());
                    }
                    if ui
                        .small_button("HSL")
                        .on_hover_text("Copy HSL / HSLA values")
                        .clicked()
                    {
                        action = ItemAction::CopyColor(parsed.to_hsl_string(), "HSL".to_string());
                    }

                    if crate::color::is_eight_digit_hex(&item.content) {
                        if let Some(argb) = crate::color::parse_argb_hex(&item.content) {
                            if ui
                                .small_button("ARGB")
                                .on_hover_text("Copy as Android/Flutter ARGB (AARRGGBB)")
                                .clicked()
                            {
                                action =
                                    ItemAction::CopyColor(argb.to_rgb_string(), "ARGB".to_string());
                            }
                        }
                    }
                });
            });
        });

        action
    }

    fn paint_color_swatch(
        painter: &egui::Painter,
        rect: egui::Rect,
        parsed: crate::color::ParsedColor,
        r_radius: f32,
    ) {
        if parsed.a == 255 {
            // Fully opaque color: clean solid rounded fill
            painter.rect_filled(rect, r_radius, parsed.to_color32());
        } else {
            // Semi-transparent color:
            // Left half: Pure opaque base color (clearly identifies true hue)
            // Right half: Translucent color rendered over a standard light checkerboard
            let half_w = rect.width() / 2.0;
            let left_rect = egui::Rect::from_min_size(rect.min, egui::vec2(half_w, rect.height()));
            let right_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(half_w, 0.0),
                egui::vec2(half_w, rect.height()),
            );

            // Left half: Opaque base color
            painter.rect_filled(
                left_rect,
                egui::Rounding {
                    nw: r_radius,
                    ne: 0.0,
                    sw: r_radius,
                    se: 0.0,
                },
                egui::Color32::from_rgb(parsed.r, parsed.g, parsed.b),
            );

            // Right half: Light checkerboard base
            painter.rect_filled(
                right_rect,
                egui::Rounding {
                    nw: 0.0,
                    ne: r_radius,
                    sw: 0.0,
                    se: r_radius,
                },
                egui::Color32::from_gray(245),
            );

            let cell = (rect.height() / 4.0).clamp(3.0, 6.0);
            let q_top_left = egui::Rect::from_min_size(right_rect.min, egui::vec2(cell, cell));
            let q_mid = egui::Rect::from_min_size(
                right_rect.min + egui::vec2(cell, cell),
                egui::vec2(cell, cell),
            );
            painter.rect_filled(q_top_left, 0.0, egui::Color32::from_gray(190));
            painter.rect_filled(q_mid, 0.0, egui::Color32::from_gray(190));

            // Alpha overlay on right half
            painter.rect_filled(
                right_rect,
                egui::Rounding {
                    nw: 0.0,
                    ne: r_radius,
                    sw: 0.0,
                    se: r_radius,
                },
                parsed.to_color32(),
            );

            // Subtle divider line between opaque and alpha preview
            painter.line_segment(
                [
                    egui::pos2(rect.min.x + half_w, rect.min.y),
                    egui::pos2(rect.min.x + half_w, rect.max.y),
                ],
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(50)),
            );
        }

        // Crisp border around entire swatch
        painter.rect_stroke(
            rect,
            r_radius,
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(50)),
        );
    }

    fn render_color_tooltip(ui: &mut egui::Ui, parsed: crate::color::ParsedColor) {
        if parsed.a == 255 {
            ui.label(format!("Color: {}", parsed.to_hex()));
            ui.label(format!("RGB: {}", parsed.to_rgb_string()));
            ui.label(format!("HSL: {}", parsed.to_hsl_string()));
        } else {
            let opacity_pct = u32::from(parsed.a) * 100 / 255;
            ui.label(format!(
                "Left: 100% Opaque (#{:02X}{:02X}{:02X})",
                parsed.r, parsed.g, parsed.b
            ));
            ui.label(format!("Right: {opacity_pct}% Opacity over light canvas"));
            ui.label(format!("RGBA: {}", parsed.to_rgb_string()));
            ui.label(format!("HSLA: {}", parsed.to_hsl_string()));
        }
    }

    fn render_color_swatch(ui: &mut egui::Ui, parsed: crate::color::ParsedColor) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(44.0, 32.0), egui::Sense::click());
        Self::paint_color_swatch(ui.painter(), rect, parsed, 4.0);
        response.on_hover_ui(|ui| {
            Self::render_color_tooltip(ui, parsed);
        })
    }

    fn render_mini_color_swatch(
        ui: &mut egui::Ui,
        parsed: crate::color::ParsedColor,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 16.0), egui::Sense::click());
        Self::paint_color_swatch(ui.painter(), rect, parsed, 3.0);
        response.on_hover_ui(|ui| {
            Self::render_color_tooltip(ui, parsed);
        })
    }

    fn render_embedded_colors(
        ui: &mut egui::Ui,
        colors: &[crate::color::ParsedColor],
    ) -> ItemAction {
        let mut action = ItemAction::None;

        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!("{} Colors:", icons::PALETTE))
                    .size(11.0)
                    .color(egui::Color32::from_rgb(230, 126, 34))
                    .strong(),
            );

            for &color in colors {
                egui::Frame::none()
                    .fill(egui::Color32::from_white_alpha(8))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_white_alpha(24),
                    ))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(5.0, 2.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let swatch_res = Self::render_mini_color_swatch(ui, color);
                            if swatch_res.clicked() {
                                action = ItemAction::CopyColor(color.to_hex(), "HEX".to_string());
                            } else if swatch_res.hovered() {
                                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                            }

                            let hex_label = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(color.to_hex())
                                        .monospace()
                                        .size(11.0)
                                        .strong(),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if hex_label.clicked() {
                                action = ItemAction::CopyColor(color.to_hex(), "HEX".to_string());
                            } else if hex_label.hovered() {
                                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                            }

                            if ui
                                .small_button("# HEX")
                                .on_hover_text(format!("Copy {}", color.to_hex()))
                                .clicked()
                            {
                                action = ItemAction::CopyColor(color.to_hex(), "HEX".to_string());
                            }
                            if ui
                                .small_button("RGB")
                                .on_hover_text(format!("Copy {}", color.to_rgb_string()))
                                .clicked()
                            {
                                action =
                                    ItemAction::CopyColor(color.to_rgb_string(), "RGB".to_string());
                            }
                            if ui
                                .small_button("HSL")
                                .on_hover_text(format!("Copy {}", color.to_hsl_string()))
                                .clicked()
                            {
                                action =
                                    ItemAction::CopyColor(color.to_hsl_string(), "HSL".to_string());
                            }
                            if color.a < 255
                                && ui
                                    .small_button("ARGB")
                                    .on_hover_text("Copy as Android/Flutter ARGB")
                                    .clicked()
                            {
                                let argb_hex = format!(
                                    "#{:02X}{:02X}{:02X}{:02X}",
                                    color.a, color.r, color.g, color.b
                                );
                                action = ItemAction::CopyColor(argb_hex, "ARGB".to_string());
                            }
                        });
                    });
            }
        });

        action
    }

    fn render_url_content(ui: &mut egui::Ui, content: &str) -> ItemAction {
        let preview = if content.len() > 180 {
            format!("{}...", &content[..180])
        } else {
            content.to_string()
        };

        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(preview)
                    .color(egui::Color32::from_rgb(100, 181, 246))
                    .underline(),
            )
            .sense(egui::Sense::click()),
        );

        if label.clicked() {
            ItemAction::CopyText(content.to_string())
        } else {
            if label.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            }
            ItemAction::None
        }
    }

    fn render_text_content(ui: &mut egui::Ui, content: &str) -> ItemAction {
        let preview = if content.len() > 180 {
            format!("{}...", &content[..180])
        } else {
            content.to_string()
        };

        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(preview)
                    .monospace()
                    .color(egui::Color32::from_rgb(235, 235, 240)),
            )
            .sense(egui::Sense::click()),
        );

        if label.clicked() {
            ItemAction::CopyText(content.to_string())
        } else {
            if label.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            }
            ItemAction::None
        }
    }

    fn render_image_content(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        texture_cache: &mut HashMap<i64, egui::TextureHandle>,
        ctx: &egui::Context,
    ) -> ItemAction {
        let Some(png_bytes) = &item.image_data else {
            return ItemAction::None;
        };

        let texture = match texture_cache.entry(item.id) {
            std::collections::hash_map::Entry::Occupied(e) => Some(e.into_mut()),
            std::collections::hash_map::Entry::Vacant(e) => {
                if let Ok((w, h, rgba)) = media::png_to_rgba(png_bytes) {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    let handle = ctx.load_texture(
                        format!("clip_img_{}", item.id),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    Some(e.insert(handle))
                } else {
                    None
                }
            }
        };

        if let Some(texture) = texture {
            let img_widget = egui::Image::new(&*texture)
                .max_size(egui::vec2(340.0, 160.0))
                .sense(egui::Sense::click());
            let response = ui.add(img_widget);

            if let (Some(w), Some(h)) = (item.width, item.height) {
                ui.label(
                    egui::RichText::new(format!("{w} × {h} px"))
                        .size(10.0)
                        .weak(),
                );
            }

            if response.clicked() {
                return ItemAction::CopyImage(png_bytes.clone());
            }
            if response.hovered() {
                ui.output_mut(|o| {
                    o.cursor_icon = egui::CursorIcon::PointingHand;
                });
            }
        } else {
            ui.label(egui::RichText::new("Failed to load image preview").weak());
        }

        ItemAction::None
    }
}

impl eframe::App for ClipboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_window_close_request(ctx);
        self.handle_hotkey_toggle(ctx);
        self.handle_escape_key(ctx);
        self.handle_keyboard_navigation(ctx);
        self.handle_app_activation(ctx);
        self.handle_monitor_updates();

        if !self.is_visible {
            // Keep the event loop alive with a gentle 50ms tick while hidden.
            // When a window is set to Visible(false), OS compositors (Windows DWM,
            // macOS WindowServer, X11) suppress redraw events, which can cause
            // request_repaint() from the background hotkey thread to stall.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }

        self.render_top_panel(ctx);
        self.render_bottom_panel(ctx);
        if self.active_view == ActiveView::Settings {
            self.render_settings_panel(ctx);
        } else {
            self.render_central_panel(ctx);
        }
    }
}
