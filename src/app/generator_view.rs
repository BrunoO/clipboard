use super::{ActiveView, ClipboardApp};
use eframe::egui;
use egui_phosphor::regular as icons;

impl ClipboardApp {
    fn save_generator_preferences(&mut self) {
        self.settings.password_config = self.password_gen.config.clone();
        self.settings.generator_skip_history = self.password_gen.skip_history;
        self.settings.generator_reveal_value = self.password_gen.reveal_value;
        let _ = self.settings.save(&self.db);
    }

    pub(super) fn render_generator_panel(&mut self, ctx: &egui::Context, is_dark: bool) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(
                    egui::RichText::new(format!("{} Password Generator", icons::KEY))
                        .color(crate::theme::text_primary(is_dark))
                        .strong(),
                );
                ui.add_space(8.0);

                self.render_generator_output_card(ui, ctx, is_dark);
                ui.add_space(10.0);

                self.render_generator_mode_and_options(ui, is_dark);
                ui.add_space(10.0);

                ui.group(|ui| {
                    if ui
                        .checkbox(
                            &mut self.password_gen.skip_history,
                            "Don't save generated password to clipboard history",
                        )
                        .changed()
                    {
                        self.save_generator_preferences();
                    }
                    ui.label(
                        egui::RichText::new(
                            "Press Enter to copy & dismiss immediately, or Esc to return to history.",
                        )
                        .color(crate::theme::text_secondary(is_dark))
                        .size(11.0_f32),
                    );
                });

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

    fn render_generator_output_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        is_dark: bool,
    ) {
        ui.group(|ui| {
            let display_text = if self.password_gen.reveal_value {
                self.password_gen.current_value.clone()
            } else {
                "•".repeat(self.password_gen.current_value.chars().count().min(32))
            };

            egui::Frame::none()
                .fill(crate::theme::search_bg(is_dark))
                .stroke(crate::theme::card_stroke(is_dark, true))
                .rounding(6.0_f32)
                .inner_margin(egui::Margin::symmetric(10.0_f32, 10.0_f32))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(display_text)
                                    .monospace()
                                    .size(15.0_f32)
                                    .color(crate::theme::text_primary(is_dark))
                                    .strong(),
                            )
                            .wrap(),
                        );
                    });
                });

            ui.add_space(6.0);
            Self::render_generator_entropy_bar(ui, &self.password_gen.config, is_dark);
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let copy_label = if self.settings.auto_paste {
                    format!("{} Copy & Paste", icons::COPY)
                } else {
                    format!("{} Copy & Dismiss", icons::COPY)
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

                let eye_icon = if self.password_gen.reveal_value {
                    icons::EYE_SLASH
                } else {
                    icons::EYE
                };
                if ui
                    .button(eye_icon)
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
            });
        });
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

        ui.group(|ui| {
            ui.horizontal(|ui| {
                let old_mode = self.password_gen.config.mode;
                ui.selectable_value(
                    &mut self.password_gen.config.mode,
                    GeneratorMode::Password,
                    "Random Password",
                );
                ui.selectable_value(
                    &mut self.password_gen.config.mode,
                    GeneratorMode::Passphrase,
                    "Memorable Passphrase",
                );
                if self.password_gen.config.mode != old_mode {
                    self.password_gen.regenerate();
                    self.save_generator_preferences();
                }
            });

            ui.add_space(6.0);
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
                    egui::Slider::new(&mut self.password_gen.config.word_count, 3..=8)
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
