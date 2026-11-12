use crate::db::{ClipboardItem, Database, ItemKind};
use crate::hotkey::HotkeyService;
use crate::media;
use arboard::Clipboard;
use eframe::egui;
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

enum ItemAction {
    None,
    CopyText(String),
    CopyFormattedJson(String),
    CopyColor(String, String),
    CopyImage(Vec<u8>),
    OpenBrowser(String),
    Delete(i64),
}

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
            content_filter: ContentFilter::All,
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
            ui.add_space(4.0);
            self.render_filter_tabs(ui);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.hide_on_copy, "Close window after copy");
            });
            ui.add_space(4.0);
        });
    }

    fn render_filter_tabs(&mut self, ui: &mut egui::Ui) {
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
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::All,
                format!("All ({all_count})"),
            );
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::Text,
                format!("📝 Text ({text_count})"),
            );
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::Links,
                format!("🌐 Links ({link_count})"),
            );
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::Code,
                format!("💻 Code ({code_count})"),
            );
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::Colors,
                format!("🎨 Colors ({color_count})"),
            );
            ui.selectable_value(
                &mut self.content_filter,
                ContentFilter::Image,
                format!("🖼️ Images ({img_count})"),
            );
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
        let filter = self.content_filter;
        let filtered_indices: Vec<usize> = self
            .items
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
            .collect();

        egui::CentralPanel::default().show(ctx, |ui| {
            if filtered_indices.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    let empty_hint = match filter {
                        ContentFilter::All => "No clipboard history found.",
                        ContentFilter::Text => "No text entries found.",
                        ContentFilter::Links => "No links found in clipboard history.",
                        ContentFilter::Code => "No code snippets found in clipboard history.",
                        ContentFilter::Colors => "No colors found in clipboard history.",
                        ContentFilter::Image => "No image entries found.",
                    };
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

                for &idx in filtered_indices {
                    if let Some(item) = self.items.get(idx) {
                        let item_action =
                            Self::render_clipboard_item(ui, item, &mut self.texture_cache, ctx);
                        if !matches!(item_action, ItemAction::None) {
                            action = item_action;
                        }
                    }
                    ui.add_space(4.0);
                }

                match action {
                    ItemAction::CopyText(content) => self.copy_text_to_clipboard(&content, ctx),
                    ItemAction::CopyFormattedJson(formatted) => {
                        self.copy_text_to_clipboard(&formatted, ctx);
                        self.show_toast("✓ Copied formatted JSON to clipboard!");
                    }
                    ItemAction::CopyColor(val, name) => {
                        self.copy_text_to_clipboard(&val, ctx);
                        self.show_toast(&format!("Copied {name}: {val}"));
                    }
                    ItemAction::CopyImage(png_bytes) => {
                        self.copy_image_to_clipboard(&png_bytes, ctx);
                    }
                    ItemAction::OpenBrowser(url) => {
                        if let Err(e) = webbrowser::open(&url) {
                            eprintln!("Failed to open link in browser: {e}");
                            self.show_toast("Failed to open browser.");
                        } else {
                            self.show_toast("Opened in default browser!");
                        }
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
                let header_action = Self::render_item_header(ui, item);
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

        action
    }

    fn render_item_badge(ui: &mut egui::Ui, item: &ClipboardItem) {
        match item.kind {
            ItemKind::Text => {
                let embedded = item.embedded_colors();
                let color_hint = match embedded.len() {
                    0 => String::new(),
                    1 => " • 🎨 1 color".to_string(),
                    n => format!(" • 🎨 {n} colors"),
                };
                ui.label(
                    egui::RichText::new(format!("📝 Text{color_hint} • {}", item.created_at))
                        .size(10.0)
                        .weak(),
                );
            }
            ItemKind::Url => {
                let host = item.url_host().unwrap_or_else(|| "link".to_string());
                ui.label(
                    egui::RichText::new("🌐 Link")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(52, 152, 219))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(format!("• {host} • {}", item.created_at))
                        .size(10.0)
                        .weak(),
                );
            }
            ItemKind::Code => {
                let lang_badge = item
                    .code_language()
                    .map_or("💻 Code", crate::code::CodeLanguage::badge);
                let line_count = item.content.lines().count();
                let embedded = item.embedded_colors();
                let color_hint = match embedded.len() {
                    0 => String::new(),
                    1 => " • 🎨 1 color".to_string(),
                    n => format!(" • 🎨 {n} colors"),
                };
                ui.label(
                    egui::RichText::new(lang_badge)
                        .size(10.0)
                        .color(egui::Color32::from_rgb(224, 108, 117))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "• {line_count} lines{color_hint} • {}",
                        item.created_at
                    ))
                    .size(10.0)
                    .weak(),
                );
            }
            ItemKind::Color => {
                let hex = item
                    .parsed_color()
                    .map_or_else(|| item.content.clone(), crate::color::ParsedColor::to_hex);
                ui.label(
                    egui::RichText::new("🎨 Color")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(230, 126, 34))
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(format!("• {hex} • {}", item.created_at))
                        .size(10.0)
                        .weak(),
                );
            }
            ItemKind::Image => {
                ui.label(
                    egui::RichText::new(format!("🖼️ Image • {}", item.created_at))
                        .size(10.0)
                        .weak(),
                );
            }
        }
    }

    fn render_item_header(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        let mut action = ItemAction::None;

        ui.horizontal(|ui| {
            Self::render_item_badge(ui, item);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(egui::RichText::new("×").size(12.0))
                    .on_hover_text("Delete entry")
                    .clicked()
                {
                    action = ItemAction::Delete(item.id);
                }
                if ui
                    .small_button("📋 Copy")
                    .on_hover_text("Copy back to clipboard")
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
                        .small_button("↗ Open")
                        .on_hover_text("Open link in default browser")
                        .clicked()
                {
                    action = ItemAction::OpenBrowser(item.content.trim().to_string());
                }
                if item.code_language() == Some(crate::code::CodeLanguage::Json) {
                    if let Some(pretty) = crate::code::format_json(&item.content) {
                        if ui
                            .small_button("{ } Prettify")
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
                egui::RichText::new("🎨 Colors:")
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
