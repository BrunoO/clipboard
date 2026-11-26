use super::{ActiveView, ClipboardApp};
use crate::icons;
use eframe::egui;

impl ClipboardApp {
    fn generator_card<R>(
        ui: &mut egui::Ui,
        is_dark: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let fill = crate::theme::card_fill(is_dark, false);
        let stroke = crate::theme::card_stroke(is_dark, false);
        let shadow = crate::theme::card_shadow(is_dark, false);

        egui::Frame::group(ui.style())
            .corner_radius(crate::theme::card_rounding())
            .fill(fill)
            .stroke(stroke)
            .shadow(shadow)
            .inner_margin(12)
            .show(ui, add_contents)
            .inner
    }

    fn save_generator_preferences(&mut self) {
        self.settings.password_config = self.password_gen.config.clone();
        self.settings.generator_skip_history = self.password_gen.skip_history;
        self.settings.generator_reveal_value = self.password_gen.reveal_value;
        self.persist_settings();
    }

    pub(super) fn render_generator_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} Password Forge", icons::KEY))
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

                self.render_generator_output_card(ui, &ctx, is_dark);
                ui.add_space(10.0);

                self.render_generator_mode_and_options(ui, is_dark);
                ui.add_space(10.0);

                Self::generator_card(ui, is_dark, |ui| {
                    if ui
                        .checkbox(
                            &mut self.password_gen.skip_history,
                            "Don't save generated password to clipboard history",
                        )
                        .changed()
                    {
                        self.save_generator_preferences();
                    }
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(
                            "Press Enter to copy & dismiss immediately, or Esc to return to history.",
                        )
                        .color(crate::theme::text_secondary(is_dark))
                        .size(11.0_f32),
                    );
                });
            });
        });
    }

    fn render_generator_output_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        is_dark: bool,
    ) {
        Self::generator_card(ui, is_dark, |ui| {
            let display_text = if self.password_gen.reveal_value {
                self.password_gen.current_value.as_str().to_owned()
            } else {
                "•".repeat(self.password_gen.current_value.chars().count().min(32))
            };

            egui::Frame::new()
                .fill(if is_dark {
                    egui::Color32::from_rgb(20, 21, 26)
                } else {
                    egui::Color32::from_rgb(243, 244, 248)
                })
                .stroke(crate::theme::card_stroke(is_dark, true))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&display_text)
                                    .monospace()
                                    .size(15.0_f32)
                                    .color(crate::theme::text_primary(is_dark))
                                    .strong(),
                            )
                            .wrap(),
                        );
                    });
                });

            ui.add_space(8.0);
            Self::render_generator_entropy_bar(ui, &self.password_gen.config, is_dark);
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let copy_label = if self.settings.auto_paste {
                    format!("{} Copy & Paste (↵)", icons::COPY)
                } else {
                    format!("{} Copy & Dismiss (↵)", icons::COPY)
                };
                if ui.button(copy_label).clicked() {
                    self.copy_generated_password(ctx, true);
                }

                if ui.button(format!("{} Copy", icons::COPY)).clicked() {
                    self.copy_generated_password(ctx, false);
                }

                if ui
                    .button(format!("{} Regenerate", icons::ARROWS_CLOCKWISE))
                    .clicked()
                {
                    self.password_gen.regenerate();
                }

                if ui
                    .button(format!("{} Save Secret", icons::LOCK))
                    .on_hover_text(
                        "Save this password directly as a pinned secret (without copying to clipboard)",
                    )
                    .clicked()
                {
                    let secret = self.password_gen.current_value.to_string();
                    self.open_new_secret_dialog_prefilled(secret);
                }

                self.render_reveal_toggle(ui);
            });
        });
    }

    /// Renders the reveal/hide password toggle and persists the preference on click.
    fn render_reveal_toggle(&mut self, ui: &mut egui::Ui) {
        let eye_icon = if self.password_gen.reveal_value {
            icons::EYE_SLASH
        } else {
            icons::EYE
        };
        if ui
            .button(icons::button_icon(eye_icon))
            .on_hover_text(if self.password_gen.reveal_value {
                "Hide password"
            } else {
                "Reveal password"
            })
            .clicked()
        {
            self.password_gen.reveal_value = !self.password_gen.reveal_value;
            self.save_generator_preferences();
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn render_generator_entropy_bar(
        ui: &mut egui::Ui,
        config: &crate::password::PasswordConfig,
        is_dark: bool,
    ) {
        use crate::password::{classify_strength, estimate_entropy_bits, StrengthLevel};

        let bits = estimate_entropy_bits(config);
        let level = classify_strength(bits);
        let fraction = ((bits / 128.0) as f32).clamp(0.05, 1.0);
        let bar_color = match level {
            StrengthLevel::Weak => egui::Color32::from_rgb(220, 68, 55),
            StrengthLevel::Fair => egui::Color32::from_rgb(230, 159, 0),
            StrengthLevel::Strong => egui::Color32::from_rgb(46, 160, 67),
            StrengthLevel::VeryStrong => egui::Color32::from_rgb(16, 185, 129),
        };

        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(180.0)
                    .fill(bar_color),
            );
            ui.label(
                egui::RichText::new(format!("{} • {:.0} bits", level.label(), bits))
                    .color(crate::theme::ensure_contrast(
                        bar_color,
                        crate::theme::panel_background(is_dark),
                        4.5,
                    ))
                    .size(11.5_f32)
                    .strong(),
            );
        });
    }

    fn render_generator_mode_and_options(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        use crate::password::GeneratorMode;

        Self::generator_card(ui, is_dark, |ui| {
            ui.horizontal(|ui| {
                let old_mode = self.password_gen.config.mode;
                let is_pwd = self.password_gen.config.mode == GeneratorMode::Password;
                let is_pass = self.password_gen.config.mode == GeneratorMode::Passphrase;

                if crate::theme::render_segmented_tab(
                    ui,
                    is_dark,
                    is_pwd,
                    icons::KEY,
                    "Random Password",
                    0,
                )
                .clicked()
                {
                    self.password_gen.config.mode = GeneratorMode::Password;
                }
                if crate::theme::render_segmented_tab(
                    ui,
                    is_dark,
                    is_pass,
                    icons::FILE_TEXT,
                    "Passphrase",
                    0,
                )
                .clicked()
                {
                    self.password_gen.config.mode = GeneratorMode::Passphrase;
                }

                if self.password_gen.config.mode != old_mode {
                    self.password_gen.regenerate();
                    self.save_generator_preferences();
                }
            });

            ui.add_space(8.0);
            let changed = match self.password_gen.config.mode {
                GeneratorMode::Password => self.render_generator_password_options(ui, is_dark),
                GeneratorMode::Passphrase => self.render_generator_passphrase_options(ui, is_dark),
            };

            if changed {
                self.password_gen.regenerate();
                self.save_generator_preferences();
            }
        });
    }

    fn render_generator_password_options(&mut self, ui: &mut egui::Ui, is_dark: bool) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Length:").color(crate::theme::text_primary(is_dark)));
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.password_gen.config.length, 8..=64)
                        .suffix(" chars"),
                )
                .changed();
        });
        ui.add_space(4.0);
        changed |= ui
            .checkbox(&mut self.password_gen.config.uppercase, "Uppercase (A-Z)")
            .changed();
        changed |= ui
            .checkbox(&mut self.password_gen.config.lowercase, "Lowercase (a-z)")
            .changed();
        changed |= ui
            .checkbox(&mut self.password_gen.config.numbers, "Numbers (0-9)")
            .changed();
        changed |= ui
            .checkbox(&mut self.password_gen.config.symbols, "Symbols (!@#$...)")
            .changed();
        changed |= ui
            .checkbox(
                &mut self.password_gen.config.exclude_ambiguous,
                "Avoid ambiguous characters (0, O, o, 1, l, I)",
            )
            .changed();
        changed
    }

    fn render_generator_passphrase_options(&mut self, ui: &mut egui::Ui, is_dark: bool) -> bool {
        use crate::password::PassphraseSeparator;

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Words:").color(crate::theme::text_primary(is_dark)));
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.password_gen.config.word_count, 4..=10)
                        .suffix(" words"),
                )
                .changed();
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Separator:");
            egui::ComboBox::from_id_salt("passphrase_sep_combo")
                .selected_text(self.password_gen.config.separator.label())
                .show_ui(ui, |ui| {
                    for &sep in PassphraseSeparator::all() {
                        if ui
                            .selectable_value(
                                &mut self.password_gen.config.separator,
                                sep,
                                sep.label(),
                            )
                            .clicked()
                        {
                            changed = true;
                        }
                    }
                });
        });
        changed |= ui
            .checkbox(&mut self.password_gen.config.capitalize, "Capitalize words")
            .changed();
        changed |= ui
            .checkbox(
                &mut self.password_gen.config.include_number,
                "Include a random number",
            )
            .changed();
        changed
    }
}
