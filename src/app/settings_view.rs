use super::{ActiveView, ClipboardApp};
use crate::icons;
use eframe::egui;

impl ClipboardApp {
    fn settings_card<R>(
        ui: &mut egui::Ui,
        is_dark: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let fill = crate::theme::card_fill(is_dark, false);
        let stroke = crate::theme::card_stroke(is_dark, false);
        let shadow = crate::theme::card_shadow(is_dark, false);

        egui::Frame::new()
            .corner_radius(crate::theme::card_rounding())
            .fill(fill)
            .stroke(stroke)
            .shadow(shadow)
            .inner_margin(12)
            .show(ui, add_contents)
            .inner
    }

    pub(super) fn render_settings_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} Preferences", icons::GEAR))
                            .size(16.0)
                            .color(crate::theme::text_primary(is_dark))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!("{} Back (ESC)", icons::ARROW_LEFT))
                            .on_hover_text("Return to clipboard history")
                            .clicked()
                        {
                            self.active_view = ActiveView::History;
                        }
                    });
                });
                ui.add_space(10.0);

                self.render_appearance_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_hotkey_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_behavior_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_security_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_logging_settings_group(ui, is_dark);
                ui.add_space(10.0);

                Self::render_about_settings_group(ui, is_dark);
                ui.add_space(8.0);
            });
        });
    }

    fn render_appearance_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} Appearance & Theme", icons::PALETTE))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.label(
                egui::RichText::new(
                    "Choose between Light Mode, Dark Mode, or follow your operating system appearance.",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
            );
            ui.add_space(6.0);

            let current_theme = self.settings.theme;
            let mut selected_theme = current_theme;
            ui.horizontal(|ui| {
                ui.label("Theme:");
                egui::ComboBox::from_id_salt("appearance_theme_combo")
                    .selected_text(selected_theme.display_name())
                    .show_ui(ui, |ui| {
                        for mode in [
                            crate::settings::AppThemeMode::System,
                            crate::settings::AppThemeMode::Dark,
                            crate::settings::AppThemeMode::Light,
                        ] {
                            if ui
                                .selectable_value(&mut selected_theme, mode, mode.display_name())
                                .clicked()
                                && selected_theme != current_theme
                            {
                                self.settings.theme = selected_theme;
                                let saved = self.persist_settings();
                                let new_is_dark = selected_theme.resolve(ui.ctx());
                                crate::theme::setup_theme(ui.ctx(), new_is_dark);
                                self.current_is_dark = Some(new_is_dark);
                                ui.ctx().request_repaint();
                                if saved {
                                    self.show_toast(&format!(
                                        "{} Theme changed to {}",
                                        icons::CHECK,
                                        selected_theme.display_name()
                                    ));
                                }
                            }
                        }
                    });
            });
        });
    }

    fn render_hotkey_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} Global Shortcuts", icons::KEYBOARD))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.label(
                egui::RichText::new(
                    "Click a shortcut box to record a new combination, or clear it to disable.",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
            );
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Toggle Window")
                    .color(crate::theme::text_primary(is_dark))
                    .size(12.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("Summon or hide the clipboard manager from anywhere.")
                    .color(crate::theme::text_secondary(is_dark))
                    .size(10.5),
            );
            ui.add_space(4.0);
            self.render_toggle_hotkey_editor(ui, is_dark);
            if let Some(warn) = self
                .settings
                .toggle_hotkey_parsed()
                .and_then(|p| p.soft_conflict_warning())
            {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{} {warn}", icons::WARNING))
                        .color(crate::theme::warning_color(is_dark))
                        .size(10.5),
                );
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Paste Next (Paste Stack)")
                    .color(crate::theme::text_primary(is_dark))
                    .size(12.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(
                    "Paste history entries sequentially without opening the window (LIFO by default, or FIFO from the NEXT marker).",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(10.5),
            );
            ui.add_space(4.0);
            self.render_paste_next_hotkey_editor(ui, is_dark);
            if let Some(warn) = self
                .settings
                .paste_next_hotkey_parsed()
                .and_then(|p| p.soft_conflict_warning())
            {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{} {warn}", icons::WARNING))
                        .color(crate::theme::warning_color(is_dark))
                        .size(10.5),
                );
            }

            if self.settings.has_hotkey_collision() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} Paste Next is paused because it matches Toggle Window. Record a different binding to re-enable it.",
                        icons::WARNING
                    ))
                    .color(crate::theme::warning_color(is_dark))
                    .size(10.5),
                );
            } else if let Some(err) = &self.dialogs.hotkey_conflict_error {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{} {err}", icons::WARNING))
                        .color(crate::theme::warning_color(is_dark))
                        .size(10.5),
                );
            }

            self.render_hotkey_registration_status(ui, is_dark);
        });
    }

    fn render_keycap_pill(ui: &mut egui::Ui, label: &str, is_dark: bool, accented: bool) {
        let accent = crate::theme::accent_color(is_dark);
        let fill = if accented {
            accent.gamma_multiply(if is_dark { 0.22 } else { 0.12 })
        } else if is_dark {
            egui::Color32::from_rgb(56, 56, 62)
        } else {
            egui::Color32::from_rgb(236, 236, 241)
        };
        let stroke = if accented {
            egui::Stroke::new(1.0, accent.gamma_multiply(0.65))
        } else {
            crate::theme::card_stroke(is_dark, false)
        };
        let text_color = if accented {
            accent
        } else {
            crate::theme::text_primary(is_dark)
        };

        egui::Frame::new()
            .corner_radius(egui::CornerRadius::same(5))
            .fill(fill)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(7, 2))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(11.5)
                        .color(text_color)
                        .strong(),
                );
            });
    }

    fn render_manual_toggle_hotkey_editor(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.dialogs.pending_toggle_hotkey)
                    .hint_text("e.g. Ctrl+Shift+V or Ctrl+ù")
                    .desired_width(190.0),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.apply_toggle_hotkey();
            }
            if ui.button("Save").clicked() {
                self.apply_toggle_hotkey();
            }
            if ui.button("Cancel").clicked() {
                self.cancel_recording_toggle_hotkey();
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Format: Modifier+Key (Ctrl, Alt, Shift, Cmd)")
                .color(crate::theme::text_secondary(is_dark))
                .size(10.0),
        );
    }

    fn render_hotkey_recorder_box_contents(
        ui: &mut egui::Ui,
        current_binding: &str,
        is_dark: bool,
        is_recording: bool,
    ) {
        let accent = crate::theme::accent_color(is_dark);
        ui.set_min_width(180.0);
        ui.set_min_height(20.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if is_recording {
                let held_mods = super::active_modifier_labels(ui.input(|i| i.modifiers));
                if held_mods.is_empty() {
                    ui.label(
                        egui::RichText::new("Press shortcut…")
                            .color(accent)
                            .size(12.0)
                            .strong(),
                    );
                } else {
                    for m in held_mods {
                        Self::render_keycap_pill(ui, m, is_dark, true);
                        ui.label(
                            egui::RichText::new("+")
                                .size(11.0)
                                .color(crate::theme::text_secondary(is_dark)),
                        );
                    }
                    Self::render_keycap_pill(ui, "…", is_dark, true);
                }
            } else if current_binding.is_empty() {
                ui.label(
                    egui::RichText::new("Disabled — click to record…")
                        .color(crate::theme::text_secondary(is_dark))
                        .size(12.0),
                );
            } else {
                for (idx, part) in current_binding
                    .split('+')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .enumerate()
                {
                    if idx > 0 {
                        ui.label(
                            egui::RichText::new("+")
                                .size(11.0)
                                .color(crate::theme::text_secondary(is_dark)),
                        );
                    }
                    Self::render_keycap_pill(ui, part, is_dark, false);
                }
            }
        });
    }

    fn render_toggle_hotkey_editor(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        if self.dialogs.manual_toggle_hotkey_edit {
            self.render_manual_toggle_hotkey_editor(ui, is_dark);
            return;
        }

        let is_recording = self.dialogs.recording_toggle_hotkey;
        let accent = crate::theme::accent_color(is_dark);
        let box_fill = if is_recording {
            accent.gamma_multiply(if is_dark { 0.12 } else { 0.07 })
        } else {
            crate::theme::card_fill(is_dark, false)
        };
        let box_stroke = if is_recording {
            egui::Stroke::new(1.5, accent)
        } else {
            crate::theme::card_stroke(is_dark, false)
        };

        let mut start_recording = false;
        let default_binding = crate::settings::default_toggle_hotkey();
        let is_default = self.settings.toggle_hotkey == default_binding;
        ui.horizontal(|ui| {
            let frame_resp = egui::Frame::new()
                .corner_radius(egui::CornerRadius::same(7))
                .fill(box_fill)
                .stroke(box_stroke)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    Self::render_hotkey_recorder_box_contents(
                        ui,
                        &self.settings.toggle_hotkey,
                        is_dark,
                        is_recording,
                    );
                })
                .response;

            let click_resp = ui
                .interact(
                    frame_resp.rect,
                    ui.scope_id().with("toggle_hotkey_recorder_box"),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if is_recording {
                    "Listening for a key combination…"
                } else {
                    "Click to record a new shortcut"
                });

            if click_resp.clicked() && !is_recording {
                start_recording = true;
            }

            if is_recording {
                if ui.button("Cancel").clicked() {
                    self.cancel_recording_toggle_hotkey();
                }
            } else {
                if !self.settings.toggle_hotkey.is_empty() && ui.button("Clear").clicked() {
                    self.clear_toggle_hotkey();
                }
                if !is_default
                    && ui
                        .button("Reset")
                        .on_hover_text(format!("Reset to default ({default_binding})"))
                        .clicked()
                {
                    self.reset_toggle_hotkey();
                }
                if ui
                    .button("Type")
                    .on_hover_text("Enter shortcut text manually")
                    .clicked()
                {
                    self.dialogs.manual_toggle_hotkey_edit = true;
                    self.dialogs.recording_toggle_hotkey = false;
                    self.dialogs.hotkey_recording_hint = None;
                    self.dialogs
                        .pending_toggle_hotkey
                        .clone_from(&self.settings.toggle_hotkey);
                }
            }
        });

        if start_recording {
            self.start_recording_toggle_hotkey();
        }

        Self::render_hotkey_recorder_hint(
            ui,
            is_dark,
            self.dialogs.recording_toggle_hotkey,
            self.dialogs.hotkey_recording_hint.as_deref(),
        );
    }

    fn render_hotkey_recorder_hint(
        ui: &mut egui::Ui,
        is_dark: bool,
        is_recording: bool,
        recording_hint: Option<&str>,
    ) {
        ui.add_space(4.0);
        if is_recording {
            let accent = crate::theme::accent_color(is_dark);
            let (hint_text, hint_color) = recording_hint.map_or_else(
                || {
                    (
                        "Esc to cancel · Backspace to clear",
                        crate::theme::text_secondary(is_dark),
                    )
                },
                |hint| (hint, accent),
            );
            ui.label(egui::RichText::new(hint_text).color(hint_color).size(10.5));
        } else {
            ui.label(
                egui::RichText::new(
                    "Click the box to record a shortcut, or use Type for manual entry",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(10.0),
            );
        }
    }

    fn render_hotkey_registration_status(&self, ui: &mut egui::Ui, is_dark: bool) {
        let Some(service) = &self.services.hotkey_service else {
            return;
        };
        let results = service.registration_results();
        if results.is_empty() {
            return;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Registration Status:")
                .color(crate::theme::text_primary(is_dark))
                .strong(),
        );
        for result in results {
            ui.horizontal(|ui| {
                let (icon, color) = if result.success {
                    (icons::CHECK, crate::theme::text_secondary(is_dark))
                } else {
                    (icons::WARNING, crate::theme::warning_color(is_dark))
                };
                ui.label(egui::RichText::new(icon).color(color));
                ui.label(
                    egui::RichText::new(format!("{}: {}", result.action.label(), result.hotkey))
                        .color(color),
                );
                if let Some(err) = &result.error {
                    ui.label(
                        egui::RichText::new(format!("({err})"))
                            .color(crate::theme::text_secondary(is_dark))
                            .size(10.0),
                    );
                }
            });
        }
    }

    /// Updates hotkeys from current settings.
    /// Returns `true` if all requested hotkeys registered successfully (or none were requested).
    pub(super) fn update_hotkeys_from_settings(&mut self) -> bool {
        if let Some(service) = &mut self.services.hotkey_service {
            let toggle = self.settings.active_toggle_hotkey();
            let paste_next = self.settings.active_paste_next_hotkey();
            let results = service.update_hotkeys(toggle, paste_next);
            results.iter().all(|r| r.success)
        } else {
            false
        }
    }

    /// Starts recording a toggle hotkey (cancels any active Paste Next recorder).
    pub(super) fn start_recording_toggle_hotkey(&mut self) {
        self.cancel_recording_paste_next_hotkey();
        self.dialogs.recording_toggle_hotkey = true;
        self.dialogs.manual_toggle_hotkey_edit = false;
        self.dialogs.hotkey_recording_hint = None;
        self.dialogs.hotkey_conflict_error = None;
        self.dialogs.pending_toggle_hotkey.clear();
    }

    /// Cancels toggle hotkey recording or manual text editing.
    pub(super) fn cancel_recording_toggle_hotkey(&mut self) {
        self.dialogs.recording_toggle_hotkey = false;
        self.dialogs.manual_toggle_hotkey_edit = false;
        self.dialogs.hotkey_recording_hint = None;
        self.dialogs.pending_toggle_hotkey.clear();
    }

    /// Clears the saved toggle hotkey and unregisters it.
    pub(super) fn clear_toggle_hotkey(&mut self) {
        self.settings.toggle_hotkey.clear();
        self.dialogs.hotkey_conflict_error = None;
        let _ = self.persist_settings();
        self.update_hotkeys_from_settings();
        self.cancel_recording_toggle_hotkey();
    }

    /// Resets the toggle hotkey back to the platform default.
    pub(super) fn reset_toggle_hotkey(&mut self) {
        self.dialogs.pending_toggle_hotkey = crate::settings::default_toggle_hotkey();
        self.apply_toggle_hotkey();
    }

    /// Applies the pending toggle hotkey after validating hard conflict rules.
    pub(super) fn apply_toggle_hotkey(&mut self) {
        let Some(parsed) =
            crate::settings::ParsedHotkey::parse(&self.dialogs.pending_toggle_hotkey)
        else {
            self.show_warning_toast(&format!("{} Invalid hotkey format", icons::WARNING));
            self.cancel_recording_toggle_hotkey();
            return;
        };

        let other = self.settings.paste_next_hotkey_parsed();
        if let Err(err) = parsed.validate_binding(other.as_ref()) {
            self.dialogs.hotkey_conflict_error = Some(err.message().to_string());
            self.show_warning_toast(&format!("{} {}", icons::WARNING, err.message()));
            self.cancel_recording_toggle_hotkey();
            return;
        }

        self.dialogs.hotkey_conflict_error = None;
        self.settings.toggle_hotkey = parsed.format();
        let saved = self.persist_settings();
        let registered = self.update_hotkeys_from_settings();
        if !registered {
            self.show_warning_toast(&format!(
                "{} Could not register {} (may be in use by OS)",
                icons::WARNING,
                self.settings.toggle_hotkey
            ));
        } else if saved {
            self.show_toast(&format!(
                "{} Shortcut set to {}",
                icons::CHECK,
                self.settings.toggle_hotkey
            ));
        }
        self.cancel_recording_toggle_hotkey();
    }

    fn render_manual_paste_next_hotkey_editor(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.dialogs.pending_paste_next_hotkey)
                    .hint_text("e.g. Cmd+Option+V or Ctrl+Alt+V")
                    .desired_width(190.0),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.apply_paste_next_hotkey();
            }
            if ui.button("Save").clicked() {
                self.apply_paste_next_hotkey();
            }
            if ui.button("Cancel").clicked() {
                self.cancel_recording_paste_next_hotkey();
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Format: Modifier+Key (Ctrl, Alt, Shift, Cmd)")
                .color(crate::theme::text_secondary(is_dark))
                .size(10.0),
        );
    }

    fn render_paste_next_hotkey_editor(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        if self.dialogs.manual_paste_next_hotkey_edit {
            self.render_manual_paste_next_hotkey_editor(ui, is_dark);
            return;
        }

        let is_recording = self.dialogs.recording_paste_next_hotkey;
        let accent = crate::theme::accent_color(is_dark);
        let box_fill = if is_recording {
            accent.gamma_multiply(if is_dark { 0.12 } else { 0.07 })
        } else {
            crate::theme::card_fill(is_dark, false)
        };
        let box_stroke = if is_recording {
            egui::Stroke::new(1.5, accent)
        } else {
            crate::theme::card_stroke(is_dark, false)
        };

        let mut start_recording = false;
        let default_binding = crate::settings::default_paste_next_hotkey();
        let is_default = self.settings.paste_next_hotkey == default_binding;
        ui.horizontal(|ui| {
            let frame_resp = egui::Frame::new()
                .corner_radius(egui::CornerRadius::same(7))
                .fill(box_fill)
                .stroke(box_stroke)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    Self::render_hotkey_recorder_box_contents(
                        ui,
                        &self.settings.paste_next_hotkey,
                        is_dark,
                        is_recording,
                    );
                })
                .response;

            let click_resp = ui
                .interact(
                    frame_resp.rect,
                    ui.scope_id().with("paste_next_hotkey_recorder_box"),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if is_recording {
                    "Listening for a key combination…"
                } else {
                    "Click to record a new shortcut"
                });

            if click_resp.clicked() && !is_recording {
                start_recording = true;
            }

            if is_recording {
                if ui.button("Cancel").clicked() {
                    self.cancel_recording_paste_next_hotkey();
                }
            } else {
                if !self.settings.paste_next_hotkey.is_empty() && ui.button("Clear").clicked() {
                    self.clear_paste_next_hotkey();
                }
                if !is_default
                    && ui
                        .button("Reset")
                        .on_hover_text(format!("Reset to default ({default_binding})"))
                        .clicked()
                {
                    self.reset_paste_next_hotkey();
                }
                if ui
                    .button("Type")
                    .on_hover_text("Enter shortcut text manually")
                    .clicked()
                {
                    self.cancel_recording_toggle_hotkey();
                    self.dialogs.manual_paste_next_hotkey_edit = true;
                    self.dialogs.recording_paste_next_hotkey = false;
                    self.dialogs.hotkey_recording_hint = None;
                    self.dialogs
                        .pending_paste_next_hotkey
                        .clone_from(&self.settings.paste_next_hotkey);
                }
            }
        });

        if start_recording {
            self.start_recording_paste_next_hotkey();
        }

        Self::render_hotkey_recorder_hint(
            ui,
            is_dark,
            self.dialogs.recording_paste_next_hotkey,
            self.dialogs.hotkey_recording_hint.as_deref(),
        );
    }

    /// Starts recording a Paste Next hotkey (cancels any active Toggle Window recorder).
    pub(super) fn start_recording_paste_next_hotkey(&mut self) {
        self.cancel_recording_toggle_hotkey();
        self.dialogs.recording_paste_next_hotkey = true;
        self.dialogs.manual_paste_next_hotkey_edit = false;
        self.dialogs.hotkey_recording_hint = None;
        self.dialogs.hotkey_conflict_error = None;
        self.dialogs.pending_paste_next_hotkey.clear();
    }

    /// Cancels Paste Next hotkey recording or manual text editing.
    pub(super) fn cancel_recording_paste_next_hotkey(&mut self) {
        self.dialogs.recording_paste_next_hotkey = false;
        self.dialogs.manual_paste_next_hotkey_edit = false;
        self.dialogs.hotkey_recording_hint = None;
        self.dialogs.pending_paste_next_hotkey.clear();
    }

    /// Clears the saved Paste Next hotkey and unregisters it.
    pub(super) fn clear_paste_next_hotkey(&mut self) {
        self.settings.paste_next_hotkey.clear();
        self.dialogs.hotkey_conflict_error = None;
        let _ = self.persist_settings();
        self.update_hotkeys_from_settings();
        self.cancel_recording_paste_next_hotkey();
    }

    /// Resets the Paste Next hotkey back to the platform default.
    pub(super) fn reset_paste_next_hotkey(&mut self) {
        self.dialogs.pending_paste_next_hotkey = crate::settings::default_paste_next_hotkey();
        self.apply_paste_next_hotkey();
    }

    /// Applies the pending Paste Next hotkey after validating hard conflict rules.
    pub(super) fn apply_paste_next_hotkey(&mut self) {
        let Some(parsed) =
            crate::settings::ParsedHotkey::parse(&self.dialogs.pending_paste_next_hotkey)
        else {
            self.show_warning_toast(&format!("{} Invalid hotkey format", icons::WARNING));
            self.cancel_recording_paste_next_hotkey();
            return;
        };

        let other = self.settings.toggle_hotkey_parsed();
        if let Err(err) = parsed.validate_binding(other.as_ref()) {
            self.dialogs.hotkey_conflict_error = Some(err.message().to_string());
            self.show_warning_toast(&format!("{} {}", icons::WARNING, err.message()));
            self.cancel_recording_paste_next_hotkey();
            return;
        }

        let was_cleared = self.settings.paste_next_hotkey.is_empty();
        self.dialogs.hotkey_conflict_error = None;
        self.settings.paste_next_hotkey = parsed.format();
        if was_cleared {
            self.reset_paste_stack_to_newest();
        }
        let saved = self.persist_settings();
        let registered = self.update_hotkeys_from_settings();
        if !registered {
            self.show_warning_toast(&format!(
                "{} Could not register {} (may be in use by OS)",
                icons::WARNING,
                self.settings.paste_next_hotkey
            ));
        } else if saved {
            self.show_toast(&format!(
                "{} Paste Next shortcut set to {}",
                icons::CHECK,
                self.settings.paste_next_hotkey
            ));
        }
        self.cancel_recording_paste_next_hotkey();
    }

    fn render_behavior_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} General Behavior", icons::SLIDERS))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add_space(4.0);

            let mut hide_on_copy = self.settings.hide_on_copy;
            if ui
                .checkbox(&mut hide_on_copy, "Close window immediately after copying")
                .changed()
            {
                self.settings.hide_on_copy = hide_on_copy;
                self.persist_settings();
            }

            let mut hide_on_blur = self.settings.hide_on_blur;
            if ui
                .checkbox(&mut hide_on_blur, "Dismiss window when clicking outside")
                .changed()
            {
                self.settings.hide_on_blur = hide_on_blur;
                self.persist_settings();
            }

            if crate::platform::supports_minimize_to_taskbar() {
                let mut minimize_to_taskbar = self.settings.minimize_to_taskbar;
                if ui
                    .checkbox(
                        &mut minimize_to_taskbar,
                        "Keep in Taskbar & Alt+Tab when dismissed (minimize instead of hide)",
                    )
                    .changed()
                {
                    self.settings.minimize_to_taskbar = minimize_to_taskbar;
                    self.persist_settings();
                }
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
                self.persist_settings();
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
                self.persist_settings();
            }

            if auto_paste {
                Self::render_auto_paste_accessibility_notice(ui, is_dark);
            }

            ui.add_space(6.0);
            self.render_max_history_selector(ui);
        });
    }

    fn render_auto_paste_accessibility_notice(ui: &mut egui::Ui, is_dark: bool) {
        if !crate::platform::auto_paste_permission_missing() {
            return;
        }
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(format!(
                "{} macOS note: Direct auto-paste requires Accessibility permissions in System Settings > Privacy & Security > Accessibility.\nNote: When running via terminal, grant permission to your terminal app as well.",
                icons::WARNING
            ))
            .size(11.0)
            .color(crate::theme::warning_color(is_dark)),
        );
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .small_button(format!("{} Prompt Permission", icons::SHIELD_CHECK))
                .clicked()
            {
                crate::platform::prompt_auto_paste_permission();
            }
            if ui
                .small_button(format!("{} Open System Settings", icons::ARROW_SQUARE_OUT))
                .clicked()
            {
                crate::platform::open_auto_paste_permission_settings();
            }
        });
    }

    fn render_max_history_selector(&mut self, ui: &mut egui::Ui) {
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
                            self.db.set_max_history(limit);
                            self.persist_settings();
                            if let Err(err) = self.db.prune_history(limit) {
                                self.report_db_write_failure("prune history", &err);
                            }
                            self.refresh_items();
                        }
                    }
                });
        });
    }

    fn render_security_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} Security & Privacy", icons::SHIELD_CHECK))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Your clipboard history is encrypted at rest using SQLCipher (AES-256-CBC with HMAC-SHA512).\nSettings and history are strictly stored on your local machine.",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
            );
            Self::render_keyring_fallback_notice(ui, self.key_storage_mode, is_dark);

            ui.add_space(6.0);
            if ui
                .checkbox(
                    &mut self.settings.ignore_concealed,
                    "Ignore passwords & concealed items (1Password, Bitwarden, KeePass)",
                )
                .on_hover_text(
                    "Detects OS privacy flags and prevents sensitive credentials from being recorded into history.",
                )
                .changed()
            {
                self.db.set_ignore_concealed(self.settings.ignore_concealed);
                self.persist_settings();
            }

            if let Some(limitation) = crate::platform::concealed_marker_limitation() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(limitation)
                        .color(crate::theme::text_secondary(is_dark))
                        .size(11.0),
                );
            }

            ui.add_space(4.0);
            if ui
                .checkbox(
                    &mut self.settings.auto_detect_secrets,
                    "Automatically detect & mask API keys, tokens, and secrets",
                )
                .on_hover_text(
                    "Automatically classifies copied API keys, JWTs, private keys, and high-entropy passwords as Secrets so they are masked by default.",
                )
                .changed()
            {
                self.db
                    .set_auto_detect_secrets(self.settings.auto_detect_secrets);
                self.persist_settings();
            }

            ui.add_space(6.0);
            self.render_auto_clear_secret_selector(ui);

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Clear All History", icons::TRASH))
                    .clicked()
                {
                    self.dialogs.show_clear_confirm_dialog = true;
                }
            });

            ui.add_space(6.0);
            self.render_backup_restore_group(ui, is_dark);

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Keyring Info", icons::SHIELD_CHECK))
                    .clicked()
                {
                    self.settings.keyring_explanation_shown = false;
                    self.dialogs.show_keyring_explanation = true;
                }
            });
        });
    }

    fn render_backup_restore_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.horizontal(|ui| {
            let key_hint = crate::platform::KEYRING_RECOVERY_HINT;
            if ui
                .button(format!("{} Export Encrypted Backup…", icons::DATABASE))
                .on_hover_text(format!(
                    "Export a consistent encrypted SQLCipher backup.\nRestore requires the original encryption key on the target machine.\n{key_hint}"
                ))
                .clicked()
            {
                self.export_encrypted_backup();
            }
        });
        ui.label(
            egui::RichText::new(
                "Backup stays encrypted with the same key. Restoring on another machine requires the original OS keyring key or local key file (service `cipherclipboard`, account `db_encryption_key`).",
            )
            .color(crate::theme::text_secondary(is_dark))
            .size(11.0),
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Restore Backup…", icons::ARROWS_CLOCKWISE))
                .on_hover_text(
                    "Replace current history from an encrypted backup on next restart. A pre-restore backup is created automatically before applying.",
                )
                .clicked()
            {
                self.stage_restore_backup();
            }
        });
        ui.label(
            egui::RichText::new(
                "Restore is replace-only and applies on next restart. The selected backup must decrypt with the current encryption key; current DB is backed up first.",
            )
            .color(crate::theme::text_secondary(is_dark))
            .size(11.0),
        );
    }

    fn render_keyring_fallback_notice(
        ui: &mut egui::Ui,
        key_storage_mode: crate::crypto::KeyStorageMode,
        is_dark: bool,
    ) {
        if matches!(
            key_storage_mode,
            crate::crypto::KeyStorageMode::LocalFileFallback
                | crate::crypto::KeyStorageMode::LocalFileFallbackDegraded
        ) {
            let permissions_degraded =
                key_storage_mode == crate::crypto::KeyStorageMode::LocalFileFallbackDegraded;
            let warning = if permissions_degraded {
                "Keyring fallback active, but key file permissions could not be restricted (Windows ACL hardening failed or USERNAME/USERDOMAIN was unavailable). Unlock the keyring and restart to migrate the key."
            } else {
                "Keyring fallback active: the encryption key is stored in a restricted local file (.encryption_key) because the OS Keyring was unavailable."
            };
            let color = if permissions_degraded {
                crate::theme::error_color(is_dark)
            } else {
                crate::theme::warning_color(is_dark)
            };
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("{} {warning}", icons::WARNING))
                    .color(color)
                    .size(11.0),
            );
        }
    }

    fn render_auto_clear_secret_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Auto-clear copied secrets after:");
            let mut clear_secs = self.settings.auto_clear_secret_secs;
            let selected_label = format_secret_clear_secs(clear_secs);
            egui::ComboBox::from_id_salt("auto_clear_secret_combo")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for secs in [0_u64, 15, 30, 45, 60, 120] {
                        let label = format_secret_clear_secs(secs);
                        if ui.selectable_value(&mut clear_secs, secs, label).clicked() {
                            self.settings.auto_clear_secret_secs = secs;
                            self.persist_settings();
                        }
                    }
                });
        });
    }

    fn render_logging_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} Diagnostics & Logs", icons::FILE_TEXT))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Application events, errors, and crash diagnostics are recorded to a local log file.",
                )
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
            );
            ui.add_space(6.0);

            let log_path = crate::logging::log_file_path();
            let path_str =
                log_path.map_or_else(|| "(unavailable)".to_string(), |p| p.display().to_string());

            ui.label(
                egui::RichText::new("Log file path:")
                    .color(crate::theme::text_secondary(is_dark))
                    .size(11.0),
            );
            ui.add_space(2.0);

            egui::Frame::new()
                .fill(crate::theme::search_bg(is_dark))
                .stroke(crate::theme::card_stroke(is_dark, false))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&path_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(crate::theme::text_primary(is_dark)),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    });
                });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let copy_enabled = log_path.is_some();
                if ui
                    .add_enabled(
                        copy_enabled,
                        egui::Button::new(format!("{} Copy Path", icons::COPY)),
                    )
                    .on_hover_text("Copy the full path of the log file to clipboard")
                    .clicked()
                {
                    self.copy_plain_text(
                        &path_str,
                        &format!("{} Copied log path to clipboard!", icons::CHECK),
                        ui.ctx(),
                    );
                }
            });
        });
    }

    fn render_about_settings_group(ui: &mut egui::Ui, is_dark: bool) {
        Self::settings_card(ui, is_dark, |ui| {
            ui.label(
                egui::RichText::new(format!("{} About CipherClipboard", icons::INFO))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(concat!(
                    "CipherClipboard v",
                    env!("CARGO_PKG_VERSION"),
                    "\nA secure, platform-native clipboard manager with SQLCipher 256-bit AES encryption."
                ))
                .color(crate::theme::text_secondary(is_dark))
                .size(11.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .small_button(format!("{} GitHub Repository", icons::ARROW_SQUARE_OUT))
                    .clicked()
                {
                    let _ = webbrowser::open("https://github.com/BrunoO/clipboard");
                }
            });
        });
    }
}

fn format_secret_clear_secs(secs: u64) -> String {
    if secs == 0 {
        "Off".to_string()
    } else {
        format!("{secs}s")
    }
}
