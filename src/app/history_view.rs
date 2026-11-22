use super::{ClipboardApp, ContentFilter, ImageTextureCache};
use crate::db::{ClipboardItem, Database, ItemKind};
use crate::icons;
use eframe::egui;

pub(super) enum ItemAction {
    None,
    CopyText(String),
    CopySecret(String),
    CopyTransformed {
        content: String,
        toast: String,
        is_secret: bool,
    },
    CopyColor(String, String),
    CopyImage(i64),
    ExtractTextFromImage(i64),
    OpenBrowser(String),
    ToggleSecret(i64, bool),
    ToggleRevealSecret(i64),
    Delete(i64),
    ShowWarning(String),
}

#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct ItemRenderState {
    is_selected: bool,
    shortcut_num: Option<usize>,
    should_scroll: bool,
    is_revealed: bool,
    is_dark: bool,
}

impl ClipboardApp {
    pub(super) fn render_central_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let ctx = ui.ctx().clone();
        let filter = self.content_filter;
        let filtered_indices = self.get_filtered_indices();

        egui::CentralPanel::default().show(ui, |ui| {
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
                        ContentFilter::Secrets => {
                            (icons::LOCK, "No secret entries found in clipboard history.")
                        }
                        ContentFilter::Image => (icons::IMAGE, "No image entries found."),
                    };
                    ui.label(
                        icons::outlined(empty_icon)
                            .size(28.0)
                            .color(crate::theme::text_secondary(is_dark)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(empty_hint)
                            .color(crate::theme::text_primary(is_dark))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Copy text, links, or screenshots in any app to save here!",
                        )
                        .size(12.0)
                        .color(crate::theme::text_secondary(is_dark)),
                    );
                });
                return;
            }

            self.render_items_scroll_area(ui, &filtered_indices, &ctx, is_dark);
        });
    }

    fn render_items_scroll_area(
        &mut self,
        ui: &mut egui::Ui,
        filtered_indices: &[usize],
        ctx: &egui::Context,
        is_dark: bool,
    ) {
        let should_scroll_selected = self.scroll_to_selected;
        self.scroll_to_selected = false;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut action = ItemAction::None;
                let mut clicked_slot: Option<usize> = None;

                for (slot, &idx) in filtered_indices.iter().enumerate() {
                    if let Some(item) = self.items.get(idx) {
                        let is_selected = slot == self.selected_index;
                        let shortcut_num = if slot < 9 { Some(slot + 1) } else { None };
                        let should_scroll = is_selected && should_scroll_selected;
                        let is_revealed = self.revealed_secrets.contains(&item.id);

                        let (item_action, clicked) = Self::render_clipboard_item(
                            ui,
                            item,
                            &self.db,
                            &mut self.texture_cache,
                            ctx,
                            ItemRenderState {
                                is_selected,
                                shortcut_num,
                                should_scroll,
                                is_revealed,
                                is_dark,
                            },
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

                self.dispatch_item_action(action, ctx);
            });
    }

    pub(super) fn dispatch_item_action(&mut self, action: ItemAction, ctx: &egui::Context) {
        match action {
            ItemAction::CopyText(content) => self.copy_text_to_clipboard(&content, ctx),
            ItemAction::CopySecret(secret) => self.copy_secret_to_clipboard(&secret, ctx),
            ItemAction::CopyTransformed {
                content,
                toast,
                is_secret,
            } => {
                self.copy_transformed_to_clipboard(&content, &toast, is_secret, ctx);
            }
            ItemAction::CopyColor(val, name) => {
                self.copy_text_to_clipboard(&val, ctx);
                self.show_toast(&format!("{} Copied {name}: {val}", icons::CHECK));
            }
            ItemAction::CopyImage(id) => self.copy_image_by_id(id, ctx),
            ItemAction::ExtractTextFromImage(id) => self.start_image_ocr(id, ctx),
            ItemAction::OpenBrowser(url) => self.open_url_in_browser(&url),
            ItemAction::ToggleSecret(id, make_secret) => {
                let operation = if make_secret {
                    "mark entry as secret"
                } else {
                    "unmark entry as secret"
                };
                if let Err(error) = self.db.set_item_secret(id, make_secret) {
                    self.report_db_write_failure(operation, &error);
                    return;
                }
                self.revealed_secrets.remove(&id);
                self.refresh_items();
                let msg = if make_secret {
                    format!("{} Marked entry as Secret.", icons::LOCK)
                } else {
                    format!("{} Unmarked entry as Secret.", icons::LOCK_OPEN)
                };
                self.show_toast(&msg);
            }
            ItemAction::ToggleRevealSecret(id) => {
                if !self.revealed_secrets.remove(&id) {
                    self.revealed_secrets.insert(id);
                }
            }
            ItemAction::Delete(id) => {
                if let Err(error) = self.db.delete(id) {
                    self.report_db_write_failure("delete entry", &error);
                    return;
                }
                self.revealed_secrets.remove(&id);
                self.texture_cache.remove(id);
                self.refresh_items();
                self.show_toast(&format!("{} Entry deleted.", icons::TRASH));
            }
            ItemAction::ShowWarning(msg) => self.show_warning_toast(&msg),
            ItemAction::None => {}
        }
    }

    fn open_url_in_browser(&mut self, raw_url: &str) {
        let Some(safe_url) = sanitize_browser_url(raw_url) else {
            self.show_warning_toast(&format!(
                "{} Blocked invalid or unsupported URL scheme.",
                icons::WARNING
            ));
            return;
        };
        if let Err(e) = webbrowser::open(&safe_url) {
            eprintln!("Failed to open link in browser: {e}");
            self.show_warning_toast(&format!("{} Failed to open browser.", icons::WARNING));
        } else {
            self.show_toast(&format!(
                "{} Opened in default browser!",
                icons::ARROW_SQUARE_OUT
            ));
        }
    }

    fn render_clipboard_item(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        db: &Database,
        texture_cache: &mut ImageTextureCache,
        ctx: &egui::Context,
        state: ItemRenderState,
    ) -> (ItemAction, bool) {
        let mut action = ItemAction::None;

        let stroke = crate::theme::card_stroke(state.is_dark, state.is_selected);
        let fill = crate::theme::card_fill(state.is_dark, state.is_selected);
        let rounding = crate::theme::card_rounding();
        let shadow = crate::theme::card_shadow(state.is_dark, state.is_selected);

        let frame_resp = egui::Frame::group(ui.style())
            .corner_radius(rounding)
            .stroke(stroke)
            .fill(fill)
            .shadow(shadow)
            .inner_margin(8)
            .show(ui, |ui| {
                let header_action = Self::render_item_header(
                    ui,
                    item,
                    state.shortcut_num,
                    state.is_selected,
                    state.is_revealed,
                    state.is_dark,
                );
                if !matches!(header_action, ItemAction::None) {
                    action = header_action;
                }

                ui.add_space(2.0);

                let content_action = match item.kind {
                    ItemKind::Text => Self::render_text_content(ui, &item.content, state.is_dark),
                    ItemKind::Url => Self::render_url_content(ui, &item.content, state.is_dark),
                    ItemKind::Code => Self::render_code_content(ui, item, state.is_dark),
                    ItemKind::Color => Self::render_color_content(ui, item, state.is_dark),
                    ItemKind::Secret => Self::render_secret_content(
                        ui,
                        &item.content,
                        state.is_revealed,
                        state.is_dark,
                    ),
                    ItemKind::Image => {
                        Self::render_image_content(ui, item, db, texture_cache, ctx, state.is_dark)
                    }
                };

                if !matches!(content_action, ItemAction::None) {
                    action = content_action;
                }

                if item.kind == ItemKind::Text || item.kind == ItemKind::Code {
                    let embedded = item.embedded_colors();
                    if !embedded.is_empty() {
                        ui.add_space(4.0);
                        let color_action =
                            Self::render_embedded_colors(ui, embedded, state.is_dark);
                        if !matches!(color_action, ItemAction::None) {
                            action = color_action;
                        }
                    }
                }
            });

        if state.should_scroll {
            frame_resp.response.scroll_to_me(Some(egui::Align::Center));
        }

        let mut context_action = ItemAction::None;
        frame_resp.response.context_menu(|ui| {
            let menu_act = Self::render_card_transform_menu(ui, item, state.is_revealed);
            if !matches!(menu_act, ItemAction::None) {
                context_action = menu_act;
            }
        });
        if !matches!(context_action, ItemAction::None) {
            action = context_action;
        }

        let clicked = frame_resp.response.clicked();

        if state.is_selected {
            // Crisp vertical accent indicator along the left edge of the selected card
            let card_rect = frame_resp.response.rect;
            let strip_rect = egui::Rect::from_min_size(
                egui::pos2(card_rect.min.x, card_rect.min.y + 3.0),
                egui::vec2(3.0, (card_rect.height() - 6.0).max(4.0)),
            );
            ui.painter().rect_filled(
                strip_rect,
                egui::CornerRadius {
                    nw: 2,
                    ne: 0,
                    sw: 2,
                    se: 0,
                },
                crate::theme::accent_color(state.is_dark),
            );
        }

        (action, clicked)
    }

    fn render_text_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        let embedded = item.embedded_colors();
        let color_hint = match embedded.len() {
            0 => String::new(),
            1 => format!(" • {} 1 color", icons::PALETTE),
            n => format!(" • {} {n} colors", icons::PALETTE),
        };
        let (text_c, bg_c, stroke_c) = crate::theme::category_pill_colors(is_dark, ItemKind::Text);
        crate::theme::render_pill_tag(ui, icons::FILE_TEXT, "Text", text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!("{color_hint} • {}", item.created_at))
                .size(10.0_f32)
                .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_url_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        let host = item.url_host().unwrap_or_else(|| "link".to_string());
        let (text_c, bg_c, stroke_c) = crate::theme::category_pill_colors(is_dark, ItemKind::Url);
        crate::theme::render_pill_tag(ui, icons::LINK, "Link", text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!("• {host} • {}", item.created_at))
                .size(10.0_f32)
                .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_code_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
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
        let (text_c, bg_c, stroke_c) = crate::theme::category_pill_colors(is_dark, ItemKind::Code);
        crate::theme::render_pill_tag(ui, icon, lang_name, text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!(
                "• {line_count} lines{color_hint} • {}",
                item.created_at
            ))
            .size(10.0_f32)
            .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_color_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        let hex = item
            .parsed_color()
            .map_or_else(|| item.content.clone(), crate::color::ParsedColor::to_hex);
        let (text_c, bg_c, stroke_c) = crate::theme::category_pill_colors(is_dark, ItemKind::Color);
        crate::theme::render_pill_tag(ui, icons::PALETTE, "Color", text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!("• {hex} • {}", item.created_at))
                .size(10.0_f32)
                .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_secret_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        let (text_c, bg_c, stroke_c) =
            crate::theme::category_pill_colors(is_dark, ItemKind::Secret);
        crate::theme::render_pill_tag(ui, icons::LOCK, "Secret", text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!("• {}", item.created_at))
                .size(10.0_f32)
                .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_image_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        let dims = match (item.width, item.height) {
            (Some(w), Some(h)) => format!("Image ({w}x{h})"),
            _ => "Image".to_string(),
        };
        let (text_c, bg_c, stroke_c) = crate::theme::category_pill_colors(is_dark, ItemKind::Image);
        crate::theme::render_pill_tag(ui, icons::IMAGE, &dims, text_c, bg_c, stroke_c);
        ui.label(
            egui::RichText::new(format!("• {}", item.created_at))
                .size(10.0_f32)
                .color(crate::theme::text_secondary(is_dark)),
        );
    }

    fn render_item_badge(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) {
        match item.kind {
            ItemKind::Text => Self::render_text_badge(ui, item, is_dark),
            ItemKind::Url => Self::render_url_badge(ui, item, is_dark),
            ItemKind::Code => Self::render_code_badge(ui, item, is_dark),
            ItemKind::Color => Self::render_color_badge(ui, item, is_dark),
            ItemKind::Secret => Self::render_secret_badge(ui, item, is_dark),
            ItemKind::Image => Self::render_image_badge(ui, item, is_dark),
        }
    }

    fn render_item_header(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        shortcut_num: Option<usize>,
        is_selected: bool,
        is_revealed: bool,
        is_dark: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        ui.horizontal(|ui| {
            Self::render_item_badge(ui, item, is_dark);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(num) = shortcut_num {
                    Self::render_shortcut_badge(ui, num, is_selected, is_dark);
                }

                action = Self::render_item_header_actions(ui, item, is_selected, is_revealed);
            });
        });

        action
    }

    fn render_shortcut_badge(ui: &mut egui::Ui, num: usize, is_selected: bool, is_dark: bool) {
        let shortcut_str = crate::platform::format_item_shortcut(num);

        let (text_c, bg_c, stroke_c) = crate::theme::shortcut_badge_colors(is_dark, is_selected);

        egui::Frame::new()
            .corner_radius(crate::theme::pill_rounding())
            .fill(bg_c)
            .stroke(egui::Stroke::new(1.0_f32, stroke_c))
            .inner_margin(egui::Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(shortcut_str)
                        .size(9.5_f32)
                        .color(text_c)
                        .strong(),
                );
            });
    }

    fn render_item_header_actions(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        is_selected: bool,
        is_revealed: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        let mut overflow_action = ItemAction::None;
        ui.menu_button("⋯", |ui| {
            let menu_action = Self::render_card_transform_menu(ui, item, is_revealed);
            if !matches!(menu_action, ItemAction::None) {
                overflow_action = menu_action;
            }
        });
        if !matches!(overflow_action, ItemAction::None) {
            action = overflow_action;
        }

        let eye_action = Self::render_secret_reveal_button(ui, item, is_revealed);
        if !matches!(eye_action, ItemAction::None) {
            action = eye_action;
        }

        if !is_selected {
            return action;
        }

        if ui
            .small_button(icons::button_icon(icons::TRASH))
            .on_hover_text("Delete entry (Delete or Cmd+Backspace)")
            .clicked()
        {
            action = ItemAction::Delete(item.id);
        }

        let lock_action = Self::render_secret_toggle_button(ui, item);
        if !matches!(lock_action, ItemAction::None) {
            action = lock_action;
        }

        if ui
            .small_button(format!("{} Copy", icons::COPY))
            .on_hover_text("Copy back to clipboard (Enter)")
            .clicked()
        {
            action = Self::item_copy_action(item);
        }

        let extra_action = Self::render_kind_extra_buttons(ui, item);
        if !matches!(extra_action, ItemAction::None) {
            action = extra_action;
        }

        action
    }

    fn render_secret_reveal_button(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        is_revealed: bool,
    ) -> ItemAction {
        if item.kind == ItemKind::Secret {
            let (eye_icon, eye_tip) = if is_revealed {
                (icons::EYE_SLASH, "Mask secret")
            } else {
                (icons::EYE, "Reveal secret")
            };
            if ui
                .small_button(icons::button_icon(eye_icon))
                .on_hover_text(eye_tip)
                .clicked()
            {
                return ItemAction::ToggleRevealSecret(item.id);
            }
        }
        ItemAction::None
    }

    fn render_secret_toggle_button(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        if item.kind != ItemKind::Image {
            let is_secret = item.kind == ItemKind::Secret;
            let (lock_icon, lock_tip) = if is_secret {
                (
                    icons::LOCK_OPEN,
                    format!(
                        "Unmark as Secret ({}+L)",
                        crate::platform::PRIMARY_MODIFIER_NAME
                    ),
                )
            } else {
                (
                    icons::LOCK,
                    format!(
                        "Mark as Secret (mask content, {}+L)",
                        crate::platform::PRIMARY_MODIFIER_NAME
                    ),
                )
            };
            if ui
                .small_button(icons::button_icon(lock_icon))
                .on_hover_text(lock_tip)
                .clicked()
            {
                return ItemAction::ToggleSecret(item.id, !is_secret);
            }
        }
        ItemAction::None
    }

    fn item_copy_action(item: &ClipboardItem) -> ItemAction {
        match item.kind {
            ItemKind::Text | ItemKind::Url | ItemKind::Code | ItemKind::Color => {
                ItemAction::CopyText(item.content.clone())
            }
            ItemKind::Secret => ItemAction::CopySecret(item.content.clone()),
            ItemKind::Image => ItemAction::CopyImage(item.id),
        }
    }

    fn render_kind_extra_buttons(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        let mut action = if item.kind == ItemKind::Url
            && ui
                .small_button(format!("{} Open", icons::ARROW_SQUARE_OUT))
                .on_hover_text("Open link in default browser")
                .clicked()
        {
            ItemAction::OpenBrowser(item.content.trim().to_string())
        } else {
            ItemAction::None
        };

        if item.code_language() == Some(crate::code::CodeLanguage::Json) {
            if let Some(pretty) = crate::code::format_json(&item.content) {
                if ui
                    .small_button(format!("{} Prettify", icons::BRACKETS_CURLY))
                    .on_hover_text("Copy formatted JSON with indentation")
                    .clicked()
                {
                    action = ItemAction::CopyTransformed {
                        content: pretty,
                        toast: format!("{} Copied formatted JSON to clipboard!", icons::CHECK),
                        is_secret: item.kind == ItemKind::Secret,
                    };
                }
            }
        }

        if crate::transform::has_strippable_formatting(item)
            && ui
                .small_button(format!("{} Raw", icons::FILE_TEXT))
                .on_hover_text(
                    "Copy stripped plain text (no HTML, Markdown, or ANSI formatting, Shift+Enter)",
                )
                .clicked()
        {
            let raw = crate::transform::strip_source_formatting(&item.content);
            action = ItemAction::CopyTransformed {
                content: raw,
                toast: format!("{} Copied raw plain text to clipboard!", icons::CHECK),
                is_secret: item.kind == ItemKind::Secret,
            };
        }

        if item.kind == ItemKind::Image
            && crate::platform::supports_image_ocr()
            && ui
                .small_button(format!("{} Extract Text", icons::FILE_TEXT))
                .on_hover_text("Extract text from image using on-device OCR")
                .clicked()
        {
            action = ItemAction::ExtractTextFromImage(item.id);
        }

        action
    }

    #[allow(clippy::too_many_lines)]
    fn render_card_transform_menu(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        is_revealed: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        if item.kind == ItemKind::Image && crate::platform::supports_image_ocr() {
            if ui
                .button(format!("{} Extract Text", icons::FILE_TEXT))
                .on_hover_text("Extract text from image using on-device OCR")
                .clicked()
            {
                ui.close();
                action = ItemAction::ExtractTextFromImage(item.id);
            }
            ui.separator();
        }

        if item.kind != ItemKind::Image {
            let applicable: Vec<crate::transform::TextTransform> =
                crate::transform::TextTransform::ALL
                    .iter()
                    .copied()
                    .filter(|t| t.applies_to(item))
                    .collect();

            let categories = [
                crate::transform::TransformCategory::Cleanup,
                crate::transform::TransformCategory::Case,
                crate::transform::TransformCategory::Lines,
                crate::transform::TransformCategory::Developer,
                crate::transform::TransformCategory::Url,
            ];

            for cat in categories {
                let cat_transforms: Vec<crate::transform::TextTransform> = applicable
                    .iter()
                    .copied()
                    .filter(|t| t.category() == cat)
                    .collect();

                if !cat_transforms.is_empty() {
                    ui.menu_button(cat.label(), |ui| {
                        for t in cat_transforms {
                            if ui.button(t.label()).on_hover_text(t.tooltip()).clicked() {
                                ui.close();
                                match t.apply(&item.content) {
                                    Ok(res) => {
                                        action = ItemAction::CopyTransformed {
                                            content: res,
                                            toast: t.toast_success(),
                                            is_secret: item.kind == ItemKind::Secret,
                                        };
                                    }
                                    Err(e) => {
                                        action = ItemAction::ShowWarning(format!(
                                            "Transformation failed: {e}"
                                        ));
                                    }
                                }
                            }
                        }
                    });
                }
            }

            ui.separator();
        }

        // Security / Secret classification
        if item.kind != ItemKind::Image {
            let is_secret = item.kind == ItemKind::Secret;
            let secret_label = if is_secret {
                format!(
                    "{} Unmark as Secret ({}+L)",
                    icons::LOCK_OPEN,
                    crate::platform::PRIMARY_MODIFIER_NAME
                )
            } else {
                format!(
                    "{} Mark as Secret ({}+L)",
                    icons::LOCK,
                    crate::platform::PRIMARY_MODIFIER_NAME
                )
            };
            if ui.button(secret_label).clicked() {
                ui.close();
                action = ItemAction::ToggleSecret(item.id, !is_secret);
            }

            if is_secret {
                let reveal_label = if is_revealed {
                    format!("{} Mask Secret Content", icons::EYE_SLASH)
                } else {
                    format!("{} Reveal Secret Content", icons::EYE)
                };
                if ui.button(reveal_label).clicked() {
                    ui.close();
                    action = ItemAction::ToggleRevealSecret(item.id);
                }
            }

            ui.separator();
        }

        if ui.button(format!("{} Copy (Enter)", icons::COPY)).clicked() {
            ui.close();
            action = Self::item_copy_action(item);
        }

        if ui
            .button(format!("{} Delete Entry (Delete)", icons::TRASH))
            .clicked()
        {
            ui.close();
            action = ItemAction::Delete(item.id);
        }

        action
    }

    fn render_secret_content(
        ui: &mut egui::Ui,
        content: &str,
        is_revealed: bool,
        is_dark: bool,
    ) -> ItemAction {
        let display = if is_revealed {
            Self::format_display_preview(content, 180)
        } else {
            let bullet_len = content.chars().count().clamp(8, 24);
            "•".repeat(bullet_len)
        };

        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(display)
                    .monospace()
                    .color(crate::theme::text_preview(is_dark)),
            )
            .sense(egui::Sense::click()),
        );

        if label.clicked() {
            ItemAction::CopySecret(content.to_string())
        } else {
            if label.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            }
            ItemAction::None
        }
    }

    fn render_code_content(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) -> ItemAction {
        let lang = item
            .code_language()
            .unwrap_or(crate::code::CodeLanguage::Rust);

        let mut action = ItemAction::None;

        let canvas_fill = if is_dark {
            egui::Color32::from_rgb(26, 27, 34)
        } else {
            egui::Color32::from_rgb(244, 244, 247)
        };
        let canvas_stroke = if is_dark {
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(50, 50, 56))
        } else {
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(220, 220, 226))
        };

        egui::Frame::canvas(ui.style())
            .corner_radius(4)
            .fill(canvas_fill)
            .stroke(canvas_stroke)
            .inner_margin(6)
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

    fn render_color_content(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) -> ItemAction {
        let Some(parsed) = item.parsed_color() else {
            return Self::render_text_content(ui, &item.content, is_dark);
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
                let row_action = Self::render_color_hex_row(ui, parsed, is_dark);
                if !matches!(row_action, ItemAction::None) {
                    action = row_action;
                }

                let btn_action = Self::render_color_format_buttons(ui, parsed, &item.content);
                if !matches!(btn_action, ItemAction::None) {
                    action = btn_action;
                }
            });
        });

        action
    }

    fn render_color_hex_row(
        ui: &mut egui::Ui,
        parsed: crate::color::ParsedColor,
        is_dark: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;
        ui.horizontal(|ui| {
            let hex_label = ui.add(
                egui::Label::new(
                    egui::RichText::new(parsed.to_hex())
                        .monospace()
                        .strong()
                        .color(crate::theme::text_primary(is_dark)),
                )
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
                    .color(crate::theme::text_secondary(is_dark)),
            );
        });
        action
    }

    fn render_color_format_buttons(
        ui: &mut egui::Ui,
        parsed: crate::color::ParsedColor,
        raw_content: &str,
    ) -> ItemAction {
        let mut action = ItemAction::None;
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

            if crate::color::is_eight_digit_hex(raw_content) {
                if let Some(argb) = crate::color::parse_argb_hex(raw_content) {
                    if ui
                        .small_button("ARGB")
                        .on_hover_text("Copy as Android/Flutter ARGB (AARRGGBB)")
                        .clicked()
                    {
                        action = ItemAction::CopyColor(argb.to_rgb_string(), "ARGB".to_string());
                    }
                }
            }
        });
        action
    }

    fn paint_color_swatch(
        painter: &egui::Painter,
        rect: egui::Rect,
        parsed: crate::color::ParsedColor,
        r_radius: u8,
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
                egui::CornerRadius {
                    nw: r_radius,
                    ne: 0,
                    sw: r_radius,
                    se: 0,
                },
                egui::Color32::from_rgb(parsed.r, parsed.g, parsed.b),
            );

            // Right half: Light checkerboard base
            painter.rect_filled(
                right_rect,
                egui::CornerRadius {
                    nw: 0,
                    ne: r_radius,
                    sw: 0,
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
            painter.rect_filled(q_top_left, 0, egui::Color32::from_gray(190));
            painter.rect_filled(q_mid, 0, egui::Color32::from_gray(190));

            // Alpha overlay on right half
            painter.rect_filled(
                right_rect,
                egui::CornerRadius {
                    nw: 0,
                    ne: r_radius,
                    sw: 0,
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
            egui::StrokeKind::Inside,
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
        Self::paint_color_swatch(ui.painter(), rect, parsed, 4);
        response.on_hover_ui(|ui| {
            Self::render_color_tooltip(ui, parsed);
        })
    }

    fn render_mini_color_swatch(
        ui: &mut egui::Ui,
        parsed: crate::color::ParsedColor,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 16.0), egui::Sense::click());
        Self::paint_color_swatch(ui.painter(), rect, parsed, 3);
        response.on_hover_ui(|ui| {
            Self::render_color_tooltip(ui, parsed);
        })
    }

    fn render_embedded_colors(
        ui: &mut egui::Ui,
        colors: &[crate::color::ParsedColor],
        is_dark: bool,
    ) -> ItemAction {
        const VISIBLE: usize = 3;
        let mut action = ItemAction::None;

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!("{} Colors:", icons::PALETTE))
                    .size(11.0)
                    .color(crate::theme::warning_color(is_dark))
                    .strong(),
            );

            let (bg, stroke) = if is_dark {
                (
                    egui::Color32::from_white_alpha(8),
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(24)),
                )
            } else {
                (
                    egui::Color32::from_black_alpha(10),
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(25)),
                )
            };

            for &color in colors.iter().take(VISIBLE) {
                let chip_action = Self::render_embedded_color_chip(ui, color, bg, stroke, is_dark);
                if !matches!(chip_action, ItemAction::None) {
                    action = chip_action;
                }
            }
            if colors.len() > VISIBLE {
                egui::CollapsingHeader::new(format!("{} more colors", colors.len() - VISIBLE))
                    .id_salt(("more_colors", colors.len(), colors[0].to_hex()))
                    .show(ui, |ui| {
                        for &color in &colors[VISIBLE..] {
                            let chip_action =
                                Self::render_embedded_color_chip(ui, color, bg, stroke, is_dark);
                            if !matches!(chip_action, ItemAction::None) {
                                action = chip_action;
                            }
                        }
                    });
            }
        });

        action
    }

    fn render_embedded_color_chip(
        ui: &mut egui::Ui,
        color: crate::color::ParsedColor,
        bg: egui::Color32,
        stroke: egui::Stroke,
        is_dark: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;
        egui::Frame::new()
            .fill(bg)
            .stroke(stroke)
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
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
                                .strong()
                                .color(crate::theme::text_primary(is_dark)),
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
                        action = ItemAction::CopyColor(color.to_rgb_string(), "RGB".to_string());
                    }
                    if ui
                        .small_button("HSL")
                        .on_hover_text(format!("Copy {}", color.to_hsl_string()))
                        .clicked()
                    {
                        action = ItemAction::CopyColor(color.to_hsl_string(), "HSL".to_string());
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
        action
    }

    pub(super) fn format_display_preview(content: &str, max_chars: usize) -> String {
        let mut out = String::with_capacity(content.len().min(max_chars * 4));
        let mut count = 0;
        let mut truncated = false;
        for ch in content.chars() {
            // Strip invisible Unicode variation selectors and zero-width chars that
            // egui's font rasterizer renders as missing-glyph tofu boxes (□).
            if matches!(
                ch,
                '\u{FE0E}' | '\u{FE0F}' | '\u{200B}' | '\u{200D}' | '\u{FEFF}'
            ) {
                continue;
            }
            if count >= max_chars {
                truncated = true;
                break;
            }
            out.push(ch);
            count += 1;
        }
        if truncated {
            out.push_str("...");
        }
        out
    }

    fn render_url_content(ui: &mut egui::Ui, content: &str, is_dark: bool) -> ItemAction {
        let preview = Self::format_display_preview(content, 180);
        let link_color = crate::theme::link_color(is_dark);

        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(preview)
                    .size(12.0)
                    .line_height(Some(16.0))
                    .color(link_color)
                    .underline(),
            )
            .wrap()
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

    fn render_text_content(ui: &mut egui::Ui, content: &str, is_dark: bool) -> ItemAction {
        let preview = Self::format_display_preview(content, 180);

        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(preview)
                    .size(12.5)
                    .line_height(Some(17.0))
                    .color(crate::theme::text_primary(is_dark)),
            )
            .wrap()
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
        db: &Database,
        texture_cache: &mut ImageTextureCache,
        ctx: &egui::Context,
        is_dark: bool,
    ) -> ItemAction {
        let texture = texture_cache.get_or_load(item.id, db, ctx);

        if let Some(texture) = texture {
            let img_widget = egui::Image::new(texture)
                .max_size(egui::vec2(340.0, 160.0))
                .sense(egui::Sense::click());
            let response = ui.add(img_widget);

            if let (Some(w), Some(h)) = (item.width, item.height) {
                ui.label(
                    egui::RichText::new(format!("{w} × {h} px"))
                        .size(10.0)
                        .color(crate::theme::text_secondary(is_dark)),
                );
            }

            if response.clicked() {
                return ItemAction::CopyImage(item.id);
            }
            if response.hovered() {
                ui.output_mut(|o| {
                    o.cursor_icon = egui::CursorIcon::PointingHand;
                });
            }
        } else {
            ui.label(
                egui::RichText::new("Failed to load image preview")
                    .color(crate::theme::text_secondary(is_dark)),
            );
        }

        ItemAction::None
    }
}

/// Validates and normalizes a URL before passing it to the OS browser launcher.
/// Only `http` and `https` URLs with a non-empty host are permitted.
fn sanitize_browser_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if matches!(parsed.scheme(), "http" | "https") && parsed.has_host() {
        Some(parsed.into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_browser_url;

    #[test]
    fn test_sanitize_browser_url_allows_http_and_https() {
        assert_eq!(
            sanitize_browser_url("  https://github.com/BrunoO/clipboard  ").as_deref(),
            Some("https://github.com/BrunoO/clipboard")
        );
        assert_eq!(
            sanitize_browser_url("http://localhost:8080/path?a=1").as_deref(),
            Some("http://localhost:8080/path?a=1")
        );
    }

    #[test]
    fn test_sanitize_browser_url_blocks_non_http_schemes() {
        assert!(sanitize_browser_url("file:///etc/passwd").is_none());
        assert!(sanitize_browser_url("javascript:alert(1)").is_none());
        assert!(sanitize_browser_url("ms-msdt:/id PCWDiagnostic").is_none());
        assert!(sanitize_browser_url("ftp://files.example.com/archive.zip").is_none());
        assert!(sanitize_browser_url("--new-window https://example.com").is_none());
        assert!(sanitize_browser_url("https://").is_none());
    }

    #[test]
    fn test_extract_text_from_image_action_dispatch() {
        let (mut app, _notify_tx, _dir) = crate::app::tests::test_app_with_channels();
        let ctx = eframe::egui::Context::default();
        // Dispatching ExtractTextFromImage for an image that does not exist surfaces warning
        app.dispatch_item_action(super::ItemAction::ExtractTextFromImage(999), &ctx);
        assert!(app.toast.is_some());
    }
}
