use crate::db::{ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::media;
use arboard::Clipboard;
use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

enum ItemAction {
    None,
    CopyText(String),
    CopyImage(Vec<u8>),
    Delete(i64),
}

pub(crate) struct ClipboardApp {
    db: Database,
    search_query: String,
    items: Vec<ClipboardItem>,
    clipboard_writer: Option<Clipboard>,
    toast: Option<(String, Instant)>,
    last_query_cached: String,
    notify_rx: Receiver<()>,
    hotkey_rx: Receiver<()>,
    _hotkey_service: Option<HotkeyService>,
    texture_cache: HashMap<i64, egui::TextureHandle>,
    is_visible: bool,
    hide_on_copy: bool,
    focus_search_input: bool,
}

impl ClipboardApp {
    pub(crate) fn new(
        _cc: &eframe::CreationContext<'_>,
        db: Database,
        notify_rx: Receiver<()>,
        hotkey_rx: Receiver<()>,
        hotkey_service: Option<HotkeyService>,
    ) -> Self {
        let initial_items = db.get_recent(100, None).unwrap_or_default();
        let clipboard_writer = Clipboard::new().ok();

        Self {
            db,
            search_query: String::new(),
            items: initial_items,
            clipboard_writer,
            toast: None,
            last_query_cached: String::new(),
            notify_rx,
            hotkey_rx,
            _hotkey_service: hotkey_service,
            texture_cache: HashMap::new(),
            is_visible: true,
            hide_on_copy: true,
            focus_search_input: true,
        }
    }

    fn refresh_items(&mut self) {
        let filter = if self.search_query.trim().is_empty() {
            None
        } else {
            Some(self.search_query.as_str())
        };
        self.items = self.db.get_recent(100, filter).unwrap_or_default();
    }

    fn show_toast(&mut self, message: &str) {
        self.toast = Some((message.to_string(), Instant::now()));
    }

    fn copy_text_to_clipboard(&mut self, content: &str, ctx: &egui::Context) {
        if let Some(cb) = &mut self.clipboard_writer {
            if let Err(e) = cb.set_text(content.to_string()) {
                eprintln!("Failed to write text to clipboard: {e}");
            } else {
                self.show_toast("✓ Copied text to clipboard!");
                if self.hide_on_copy {
                    self.is_visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
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
                        self.show_toast("✓ Copied image to clipboard!");
                        if self.hide_on_copy {
                            self.is_visible = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
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

        self.is_visible = !self.is_visible;
        if self.is_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.refresh_items();
            self.focus_search_input = true;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn handle_escape_key(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.is_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
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
        }
    }

    fn render_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(6.0);
            self.render_header_row(ui, ctx);
            ui.add_space(4.0);
            self.render_search_bar(ui);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.hide_on_copy, "Close window after copy");
            });
            ui.add_space(4.0);
        });
    }

    fn render_header_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("🔒 Clipboard");

            #[cfg(target_os = "macos")]
            let shortcut_hint = "⌘ ù";
            #[cfg(not(target_os = "macos"))]
            let shortcut_hint = "Ctrl+ù";

            ui.label(
                egui::RichText::new(format!("({shortcut_hint})"))
                    .weak()
                    .size(11.0),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Hide (Esc)").clicked() {
                    self.is_visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }

                if ui
                    .small_button("🗑 Clear")
                    .on_hover_text("Permanently deletes all history")
                    .clicked()
                {
                    let _ = self.db.clear();
                    self.texture_cache.clear();
                    self.refresh_items();
                    self.show_toast("History cleared.");
                }
            });
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("🔍");
            let search_box = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search clipboard history... (Esc to hide)")
                    .desired_width(f32::INFINITY),
            );

            if self.focus_search_input {
                search_box.request_focus();
                self.focus_search_input = false;
            }
        });
    }

    fn render_bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
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
                            "{} entries • SQLCipher encrypted",
                            self.items.len()
                        ))
                        .weak()
                        .size(11.0),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Press Esc to hide").weak().size(11.0));
                });
            });
        });
    }

    fn render_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("No clipboard history found.").weak());
                    ui.label(
                        egui::RichText::new("Copy text or screenshots in any app to save here!")
                            .size(12.0)
                            .weak(),
                    );
                });
                return;
            }

            self.render_items_scroll_area(ui, ctx);
        });
    }

    fn render_items_scroll_area(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut action = ItemAction::None;

                for item in &self.items {
                    let item_action =
                        Self::render_clipboard_item(ui, item, &mut self.texture_cache, ctx);
                    if !matches!(item_action, ItemAction::None) {
                        action = item_action;
                    }
                    ui.add_space(4.0);
                }

                match action {
                    ItemAction::CopyText(content) => self.copy_text_to_clipboard(&content, ctx),
                    ItemAction::CopyImage(png_bytes) => {
                        self.copy_image_to_clipboard(&png_bytes, ctx);
                    }
                    ItemAction::Delete(id) => {
                        let _ = self.db.delete(id);
                        self.texture_cache.remove(&id);
                        self.refresh_items();
                        self.show_toast("Entry deleted.");
                    }
                    ItemAction::None => {}
                }
            });
    }

    fn render_clipboard_item(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        texture_cache: &mut HashMap<i64, egui::TextureHandle>,
        ctx: &egui::Context,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        egui::Frame::group(ui.style())
            .rounding(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let type_badge = match item.kind {
                        ItemKind::Text => "📝 Text",
                        ItemKind::Image => "🖼️ Image",
                    };
                    ui.label(
                        egui::RichText::new(format!("{} • {}", type_badge, item.created_at))
                            .size(10.0)
                            .weak(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Delete entry").clicked() {
                            action = ItemAction::Delete(item.id);
                        }
                        if ui
                            .small_button("📋 Copy")
                            .on_hover_text("Copy back to clipboard")
                            .clicked()
                        {
                            match item.kind {
                                ItemKind::Text => {
                                    action = ItemAction::CopyText(item.content.clone());
                                }
                                ItemKind::Image => {
                                    if let Some(png_bytes) = &item.image_data {
                                        action = ItemAction::CopyImage(png_bytes.clone());
                                    }
                                }
                            }
                        }
                    });
                });

                ui.add_space(2.0);

                let content_action = match item.kind {
                    ItemKind::Text => Self::render_text_content(ui, &item.content),
                    ItemKind::Image => Self::render_image_content(ui, item, texture_cache, ctx),
                };

                if !matches!(content_action, ItemAction::None) {
                    action = content_action;
                }
            });

        action
    }

    fn render_text_content(ui: &mut egui::Ui, content: &str) -> ItemAction {
        let preview = if content.len() > 180 {
            format!("{}...", &content[..180])
        } else {
            content.to_string()
        };

        let label = ui.add(
            egui::Label::new(egui::RichText::new(preview).monospace()).sense(egui::Sense::click()),
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
        self.handle_hotkey_toggle(ctx);
        self.handle_escape_key(ctx);
        self.handle_monitor_updates();

        self.render_top_panel(ctx);
        self.render_bottom_panel(ctx);
        self.render_central_panel(ctx);
    }
}
