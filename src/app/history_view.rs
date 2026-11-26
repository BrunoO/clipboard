use super::{ClipboardApp, ContentFilter, ImageTextureCache};
use crate::db::{ClipboardItem, Database, ItemKind};
use crate::icons;
use eframe::egui;
use std::time::{Duration, Instant};

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
    TogglePin(i64),
    EditAnnotation(i64),
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
    is_pulse: bool,
}

impl ClipboardApp {
    pub(super) fn render_central_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let ctx = ui.ctx().clone();
        let filter = self.content_filter;
        let filtered_indices = self.get_filtered_indices();

        egui::CentralPanel::default().show(ui, |ui| {
            if filtered_indices.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    let (empty_icon, empty_title, empty_hint) = match filter {
                        ContentFilter::All => (
                            icons::SHIELD_CHECK,
                            "Your Encrypted Vault is Ready",
                            "Copy text, code, colors, or screenshots in any application to securely save them here.",
                        ),
                        ContentFilter::Text => (
                            icons::FILE_TEXT,
                            "No Plain Text Clippings",
                            "Text clips you copy will appear here with instant formatting tools.",
                        ),
                        ContentFilter::Links => (
                            icons::LINK,
                            "No Links Saved Yet",
                            "URLs you copy will be bookmarked here with 1-click browser launch.",
                        ),
                        ContentFilter::Code => (
                            icons::CODE,
                            "No Code Snippets Found",
                            "Programming snippets will be automatically highlighted with syntax colors.",
                        ),
                        ContentFilter::Colors => (
                            icons::PALETTE,
                            "No Colors Captured",
                            "HEX, RGB, HSL, and CSS color codes will render as interactive enameled swatches.",
                        ),
                        ContentFilter::Vault => (
                            icons::PUSH_PIN,
                            "Your Vault is Empty",
                            "Pin important entries or save secrets here to keep them organized and protected.",
                        ),
                        ContentFilter::Image => (
                            icons::IMAGE,
                            "No Images Captured",
                            "Screenshots and images copied to your clipboard will be previewed here.",
                        ),
                    };

                    let card_fill = crate::theme::card_fill(is_dark, false);
                    let card_stroke = crate::theme::card_stroke(is_dark, false);
                    let card_shadow = crate::theme::card_shadow(is_dark, false);

                    egui::Frame::group(ui.style())
                        .corner_radius(12)
                        .fill(card_fill)
                        .stroke(card_stroke)
                        .shadow(card_shadow)
                        .inner_margin(egui::Margin::symmetric(24, 20))
                        .show(ui, |ui| {
                            ui.set_max_width(320.0);
                            ui.vertical_centered(|ui| {
                                let accent = crate::theme::accent_color(is_dark);
                                let halo_fill = if is_dark {
                                    egui::Color32::from_rgba_premultiplied(35, 45, 65, 120)
                                } else {
                                    egui::Color32::from_rgba_premultiplied(225, 238, 255, 200)
                                };

                                egui::Frame::new()
                                    .corner_radius(22)
                                    .fill(halo_fill)
                                    .stroke(egui::Stroke::new(1.0, accent))
                                    .inner_margin(egui::Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.label(icons::outlined(empty_icon).size(24.0).color(accent));
                                    });

                                ui.add_space(12.0);

                                ui.label(
                                    egui::RichText::new(empty_title)
                                        .size(14.0)
                                        .strong()
                                        .color(crate::theme::text_primary(is_dark)),
                                );

                                ui.add_space(4.0);

                                ui.label(
                                    egui::RichText::new(empty_hint)
                                        .size(11.5)
                                        .line_height(Some(16.0))
                                        .color(crate::theme::text_secondary(is_dark)),
                                );

                                ui.add_space(16.0);

                                ui.horizontal_wrapped(|ui| {
                                    let mod_name = crate::platform::PRIMARY_MODIFIER_NAME;
                                    Self::render_tip_chip(ui, &format!("{mod_name}+C"), "Capture", is_dark);
                                    Self::render_tip_chip(ui, &format!("{mod_name}+1..9"), "Paste", is_dark);
                                    Self::render_tip_chip(ui, "ESC", "Dismiss", is_dark);
                                });
                            });
                        });
                });
                return;
            }

            self.render_items_scroll_area(ui, &filtered_indices, &ctx, is_dark);
        });
    }

    fn render_tip_chip(ui: &mut egui::Ui, key: &str, label: &str, is_dark: bool) {
        let bg = if is_dark {
            egui::Color32::from_white_alpha(10)
        } else {
            egui::Color32::from_black_alpha(8)
        };
        let stroke = egui::Stroke::new(
            1.0,
            if is_dark {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(18)
            },
        );
        egui::Frame::new()
            .corner_radius(crate::theme::pill_rounding())
            .fill(bg)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(key)
                            .monospace()
                            .size(10.0)
                            .strong()
                            .color(crate::theme::accent_color(is_dark)),
                    );
                    ui.label(
                        egui::RichText::new(label)
                            .size(10.0)
                            .color(crate::theme::text_secondary(is_dark)),
                    );
                });
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
                        let is_pulse = self.copied_item_pulse.as_ref().is_some_and(|(id, at)| {
                            *id == item.id && at.elapsed() < Duration::from_millis(600)
                        });

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
                                is_pulse,
                            },
                        );

                        if clicked {
                            clicked_slot = Some(slot);
                        }

                        match item_action {
                            ItemAction::CopyText(_)
                            | ItemAction::CopySecret(_)
                            | ItemAction::CopyTransformed { .. }
                            | ItemAction::CopyColor(_, _)
                            | ItemAction::CopyImage(_) => {
                                self.copied_item_pulse = Some((item.id, Instant::now()));
                                ctx.request_repaint();
                            }
                            _ => {}
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
            ItemAction::TogglePin(id) => match self.db.toggle_item_pinned(id) {
                Ok(pinned) => {
                    self.refresh_items();
                    let msg = if pinned {
                        format!("{} Pinned to Vault.", icons::PUSH_PIN)
                    } else {
                        format!("{} Unpinned from Vault.", icons::PUSH_PIN)
                    };
                    self.show_toast(&msg);
                }
                Err(error) => {
                    self.report_db_write_failure("toggle pin", &error);
                }
            },
            ItemAction::EditAnnotation(id) => {
                if let Some(item) = self.items.iter().find(|i| i.id == id) {
                    self.dialogs.editing_annotation_id = Some(id);
                    self.dialogs.annotation_label_input = item.label.clone().unwrap_or_default();
                    self.dialogs.annotation_notes_input = item.notes.clone().unwrap_or_default();
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

    #[allow(clippy::too_many_lines)]
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

                if let Some(notes) = item.notes.as_deref() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(notes)
                            .size(11.0)
                            .color(crate::theme::text_secondary(state.is_dark)),
                    );
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

        if state.is_pulse {
            let card_rect = frame_resp.response.rect;
            ui.painter().rect_stroke(
                card_rect,
                rounding,
                egui::Stroke::new(2.0, crate::theme::success_color(state.is_dark)),
                egui::StrokeKind::Inside,
            );
            ui.ctx().request_repaint();
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
            if item.is_pinned {
                ui.label(
                    egui::RichText::new(icons::PUSH_PIN)
                        .size(12.0)
                        .color(crate::theme::warning_color(is_dark)),
                )
                .on_hover_text("Pinned to Vault");
            }

            if let Some(label) = item.label.as_deref() {
                ui.label(
                    egui::RichText::new(label)
                        .size(13.5)
                        .strong()
                        .color(crate::theme::text_primary(is_dark)),
                );
                ui.add_space(4.0);
            }

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
            .small_button(icons::button_icon(icons::PENCIL))
            .on_hover_text("Edit annotation (F2)")
            .clicked()
        {
            action = ItemAction::EditAnnotation(item.id);
        }

        let pin_tip = if item.is_pinned {
            "Unpin from Vault"
        } else {
            "Pin to Vault"
        };
        let pin_icon = icons::PUSH_PIN;
        let pin_btn = ui.small_button(icons::button_icon(pin_icon));
        if pin_btn.on_hover_text(pin_tip).clicked() {
            action = ItemAction::TogglePin(item.id);
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

    fn render_card_transform_menu(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        is_revealed: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

        let image_action = Self::render_transform_menu_image_section(ui, item);
        if !matches!(image_action, ItemAction::None) {
            action = image_action;
        }

        let transform_action = Self::render_transform_menu_text_sections(ui, item);
        if !matches!(transform_action, ItemAction::None) {
            action = transform_action;
        }

        let security_action = Self::render_transform_menu_security_section(ui, item, is_revealed);
        if !matches!(security_action, ItemAction::None) {
            action = security_action;
        }

        let footer_action = Self::render_transform_menu_footer_actions(ui, item);
        if !matches!(footer_action, ItemAction::None) {
            action = footer_action;
        }

        action
    }

    fn render_transform_menu_image_section(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        if item.kind != ItemKind::Image || !crate::platform::supports_image_ocr() {
            return ItemAction::None;
        }

        let action = if ui
            .button(format!("{} Extract Text", icons::FILE_TEXT))
            .on_hover_text("Extract text from image using on-device OCR")
            .clicked()
        {
            ui.close();
            ItemAction::ExtractTextFromImage(item.id)
        } else {
            ItemAction::None
        };
        ui.separator();
        action
    }

    fn render_transform_menu_text_sections(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        if item.kind == ItemKind::Image {
            return ItemAction::None;
        }

        let mut action = ItemAction::None;
        let applicable: Vec<crate::transform::TextTransform> = crate::transform::TextTransform::ALL
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
            let cat_action = Self::render_transform_menu_category(ui, item, &applicable, cat);
            if !matches!(cat_action, ItemAction::None) {
                action = cat_action;
            }
        }

        ui.separator();
        action
    }

    fn render_transform_menu_category(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        applicable: &[crate::transform::TextTransform],
        cat: crate::transform::TransformCategory,
    ) -> ItemAction {
        let cat_transforms: Vec<crate::transform::TextTransform> = applicable
            .iter()
            .copied()
            .filter(|t| t.category() == cat)
            .collect();

        if cat_transforms.is_empty() {
            return ItemAction::None;
        }

        let mut action = ItemAction::None;
        ui.menu_button(cat.label(), |ui| {
            for t in cat_transforms {
                let transform_action = Self::render_transform_menu_item(ui, item, t);
                if !matches!(transform_action, ItemAction::None) {
                    action = transform_action;
                }
            }
        });
        action
    }

    fn render_transform_menu_item(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        t: crate::transform::TextTransform,
    ) -> ItemAction {
        if !ui.button(t.label()).on_hover_text(t.tooltip()).clicked() {
            return ItemAction::None;
        }

        ui.close();
        match t.apply(&item.content) {
            Ok(res) => ItemAction::CopyTransformed {
                content: res,
                toast: t.toast_success(),
                is_secret: item.kind == ItemKind::Secret,
            },
            Err(e) => ItemAction::ShowWarning(format!("Transformation failed: {e}")),
        }
    }

    fn render_transform_menu_security_section(
        ui: &mut egui::Ui,
        item: &ClipboardItem,
        is_revealed: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;

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
        }

        let pin_label = if item.is_pinned {
            format!("{} Unpin from Vault", icons::PUSH_PIN)
        } else {
            format!("{} Pin to Vault", icons::PUSH_PIN)
        };
        if ui.button(pin_label).clicked() {
            ui.close();
            action = ItemAction::TogglePin(item.id);
        }

        if ui
            .button(format!("{} Edit Annotation (F2)", icons::PENCIL))
            .clicked()
        {
            ui.close();
            action = ItemAction::EditAnnotation(item.id);
        }

        ui.separator();
        action
    }

    fn render_transform_menu_footer_actions(ui: &mut egui::Ui, item: &ClipboardItem) -> ItemAction {
        let copy_action = if ui.button(format!("{} Copy (Enter)", icons::COPY)).clicked() {
            ui.close();
            Self::item_copy_action(item)
        } else {
            ItemAction::None
        };

        if ui
            .button(format!("{} Delete Entry (Delete)", icons::TRASH))
            .clicked()
        {
            ui.close();
            ItemAction::Delete(item.id)
        } else {
            copy_action
        }
    }

    fn render_secret_content(
        ui: &mut egui::Ui,
        content: &str,
        is_revealed: bool,
        is_dark: bool,
    ) -> ItemAction {
        let mut action = ItemAction::None;
        let warn_c = crate::theme::warning_color(is_dark);

        let bg_fill = themed_rgba(is_dark, [36, 28, 16, 140], [254, 249, 238, 220]);
        let stroke = egui::Stroke::new(
            1.0,
            themed_rgba(is_dark, [120, 90, 40, 140], [230, 200, 140, 200]),
        );

        egui::Frame::new()
            .fill(bg_fill)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                if is_revealed {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} Sensitive", icons::WARNING))
                                .size(10.5)
                                .color(warn_c)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new("• Click content to copy")
                                .size(10.0)
                                .color(crate::theme::text_secondary(is_dark)),
                        );
                    });
                    ui.add_space(4.0);

                    let display = Self::format_display_preview(content, 180);
                    let label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(display)
                                .monospace()
                                .size(12.0)
                                .color(crate::theme::text_primary(is_dark)),
                        )
                        .wrap()
                        .sense(egui::Sense::click()),
                    );

                    if label.clicked() {
                        action = ItemAction::CopySecret(content.to_string());
                    } else if label.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                } else {
                    let char_count = content.chars().count().clamp(8, 24);
                    let mut bullet_groups = Vec::new();
                    let mut remaining = char_count;
                    while remaining > 0 {
                        let chunk = remaining.min(4);
                        bullet_groups.push("•".repeat(chunk));
                        remaining -= chunk;
                    }
                    let bullets_display = bullet_groups.join("  ");

                    let mut resp = ui
                        .horizontal(|ui| {
                            ui.label(icons::outlined(icons::LOCK).size(14.0).color(warn_c));
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(bullets_display)
                                    .monospace()
                                    .size(13.0)
                                    .strong()
                                    .color(warn_c),
                            );
                        })
                        .response;

                    ui.add_space(2.0);
                    let hint = ui.label(
                        egui::RichText::new("Encrypted vault entry • Click capsule to copy")
                            .size(10.0)
                            .color(crate::theme::text_secondary(is_dark)),
                    );

                    resp = resp.union(hint);
                    let clickable_resp = ui.interact(
                        resp.rect,
                        ui.scope_id().with("vault_capsule"),
                        egui::Sense::click(),
                    );
                    if clickable_resp.clicked() {
                        action = ItemAction::CopySecret(content.to_string());
                    } else if clickable_resp.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                }
            });

        action
    }

    #[allow(clippy::too_many_lines)]
    fn render_code_content(ui: &mut egui::Ui, item: &ClipboardItem, is_dark: bool) -> ItemAction {
        let lang = item
            .code_language()
            .unwrap_or(crate::code::CodeLanguage::Rust);

        let mut action = ItemAction::None;

        let canvas_fill = if is_dark {
            egui::Color32::from_rgb(20, 21, 26)
        } else {
            egui::Color32::from_rgb(243, 244, 248)
        };
        let canvas_stroke = if is_dark {
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(45, 48, 58))
        } else {
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(218, 220, 228))
        };

        egui::Frame::canvas(ui.style())
            .corner_radius(6)
            .fill(canvas_fill)
            .stroke(canvas_stroke)
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 8,
            })
            .show(ui, |ui| {
                // Mini-IDE window bar
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(egui::vec2(36.0, 10.0), egui::Sense::hover());
                    let p = ui.painter();
                    let cy = dot_rect.center().y;
                    let r = 3.5_f32;
                    p.circle_filled(
                        egui::pos2(dot_rect.min.x + 4.0, cy),
                        r,
                        egui::Color32::from_rgb(255, 95, 86),
                    );
                    p.circle_filled(
                        egui::pos2(dot_rect.min.x + 15.0, cy),
                        r,
                        egui::Color32::from_rgb(255, 189, 46),
                    );
                    p.circle_filled(
                        egui::pos2(dot_rect.min.x + 26.0, cy),
                        r,
                        egui::Color32::from_rgb(39, 201, 63),
                    );

                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(lang.name())
                            .monospace()
                            .size(10.0)
                            .color(crate::theme::text_secondary(is_dark)),
                    );

                    if lang == crate::code::CodeLanguage::Json {
                        if let Some(pretty) = crate::code::format_json(&item.content) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button(format!("{} Prettify", icons::BRACKETS_CURLY))
                                        .on_hover_text("Copy formatted JSON with indentation")
                                        .clicked()
                                    {
                                        action = ItemAction::CopyTransformed {
                                            content: pretty,
                                            toast: format!(
                                                "{} Copied formatted JSON to clipboard!",
                                                icons::CHECK
                                            ),
                                            is_secret: item.kind == ItemKind::Secret,
                                        };
                                    }
                                },
                            );
                        }
                    }
                });

                ui.add_space(3.0);
                let sep_color = if is_dark {
                    egui::Color32::from_rgb(38, 40, 48)
                } else {
                    egui::Color32::from_rgb(228, 230, 236)
                };
                let (sep_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(sep_rect, 0, sep_color);
                ui.add_space(4.0);

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

        // Tactile enameled tile sheen on top 38%
        if rect.height() >= 12.0 && rect.width() >= 12.0 {
            let sheen_h = (rect.height() * 0.38).round().max(3.0);
            let sheen_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(1.0, 1.0),
                egui::vec2(rect.width() - 2.0, sheen_h),
            );
            painter.rect_filled(
                sheen_rect,
                egui::CornerRadius {
                    nw: r_radius.saturating_sub(1),
                    ne: r_radius.saturating_sub(1),
                    sw: 0,
                    se: 0,
                },
                egui::Color32::from_white_alpha(32),
            );
        }

        // Crisp double border: dark inner shadow stroke + soft white outer highlight
        painter.rect_stroke(
            rect,
            r_radius,
            egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(35)),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            rect,
            r_radius,
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(45)),
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
        let (rect, response) = ui.allocate_exact_size(egui::vec2(48.0, 34.0), egui::Sense::click());
        Self::paint_color_swatch(ui.painter(), rect, parsed, 6);
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
        let trimmed = content.trim();
        let (host, path_display) = url::Url::parse(trimmed).map_or_else(
            |_| ("link".to_string(), trimmed.to_string()),
            |parsed_url| {
                let h = parsed_url.host_str().unwrap_or("link").to_string();
                let mut p = parsed_url.path().to_string();
                if let Some(q) = parsed_url.query() {
                    p.push('?');
                    p.push_str(q);
                }
                if p == "/" {
                    p = String::new();
                }
                (h, p)
            },
        );

        let mut action = ItemAction::None;
        let link_color = crate::theme::link_color(is_dark);

        let bg_fill = themed_rgba(is_dark, [20, 35, 60, 60], [235, 243, 255, 180]);
        let stroke = egui::Stroke::new(
            1.0,
            themed_rgba(is_dark, [40, 70, 120, 70], [180, 210, 250, 180]),
        );

        egui::Frame::new()
            .fill(bg_fill)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(icons::outlined(icons::LINK).size(13.0).color(link_color));

                    let host_label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&host)
                                .size(12.5)
                                .strong()
                                .color(link_color),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if host_label.clicked() {
                        action = ItemAction::CopyText(content.to_string());
                    } else if host_label.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(format!("{} Open", icons::ARROW_SQUARE_OUT))
                            .on_hover_text("Open link in default browser")
                            .clicked()
                        {
                            action = ItemAction::OpenBrowser(trimmed.to_string());
                        }
                    });
                });

                if !path_display.is_empty() {
                    ui.add_space(2.0);
                    let display_path = Self::format_display_preview(&path_display, 120);
                    let path_label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(display_path)
                                .size(11.0)
                                .line_height(Some(15.0))
                                .color(crate::theme::text_secondary(is_dark)),
                        )
                        .wrap()
                        .sense(egui::Sense::click()),
                    );

                    if path_label.clicked() {
                        action = ItemAction::CopyText(content.to_string());
                    } else if path_label.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                }
            });

        action
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

const fn themed_rgba(is_dark: bool, dark: [u8; 4], light: [u8; 4]) -> egui::Color32 {
    let [r, g, b, a] = if is_dark { dark } else { light };
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
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
