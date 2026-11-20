use super::{ActiveView, ClipboardApp};
use crate::settings::HotkeyPreset;
use eframe::egui;
use egui_phosphor::regular as icons;

impl ClipboardApp {
    pub(super) fn render_settings_panel(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(
                    egui::RichText::new(format!("{} Preferences", icons::GEAR))
                        .color(crate::theme::text_primary(is_dark))
                        .strong(),
                );
                ui.add_space(8.0);

                self.render_appearance_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_hotkey_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_behavior_settings_group(ui, is_dark);
                ui.add_space(10.0);

                self.render_security_settings_group(ui, is_dark);
                ui.add_space(10.0);

                Self::render_about_settings_group(ui, is_dark);
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

    fn render_appearance_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.group(|ui| {
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
                                if let Err(e) = self.settings.save(&self.db) {
                                    eprintln!("Failed to save settings: {e}");
                                }
                                let new_is_dark = selected_theme.resolve(ui.ctx());
                                crate::theme::setup_theme(ui.ctx(), new_is_dark);
                                self.current_is_dark = Some(new_is_dark);
                                ui.ctx().request_repaint();
                                self.show_toast(&format!(
                                    "{} Theme changed to {}",
                                    icons::CHECK,
                                    selected_theme.display_name()
                                ));
                            }
                        }
                    });
            });
        });
    }

    #[allow(clippy::too_many_lines)]
    fn render_hotkey_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.group(|ui| {
            ui.label(
                egui::RichText::new(format!("{} Global Shortcut", icons::KEYBOARD))
                    .color(crate::theme::text_primary(is_dark))
                    .strong(),
            );
            ui.label(
                egui::RichText::new(
                    "Select the keyboard shortcut used to summon the clipboard manager from anywhere.",
                )
                .color(crate::theme::text_secondary(is_dark))
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
                    if self.update_hotkeys_from_settings() {
                        self.show_toast(&format!(
                            "{} Shortcut set to {}",
                            icons::CHECK,
                            preset.badge()
                        ));
                    } else {
                        self.show_toast(&format!(
                            "{} Could not register {} (may be in use by OS)",
                            icons::WARNING,
                            preset.badge()
                        ));
                    }
                }
            }

            #[cfg(target_os = "windows")]
            {
                ui.add_space(6.0);
                if ui.button("Re-detect keyboard layout").clicked() {
                    if self.update_hotkeys_from_settings() {
                        self.show_toast(&format!(
                            "{} Hotkeys re-registered for current keyboard layout",
                            icons::CHECK
                        ));
                    } else {
                        self.show_toast(&format!(
                            "{} Could not re-register hotkeys (may be in use by OS)",
                            icons::WARNING
                        ));
                    }
                }
            }

            // Custom hotkey recorder
            if self.settings.hotkey == HotkeyPreset::Custom {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                if self.recording_custom_hotkey {
                    // Recording mode
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Press desired key combination…")
                                .color(crate::theme::accent_color(is_dark))
                                .strong(),
                        );
                        if ui.button("Cancel").clicked() {
                            self.recording_custom_hotkey = false;
                            self.pending_custom_hotkey.clear();
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Press Esc to cancel, or Backspace to clear")
                            .color(crate::theme::text_secondary(is_dark))
                            .size(10.0),
                    );
                } else {
                    // Display/edit mode
                    ui.horizontal(|ui| {
                        let current = if self.settings.custom_hotkey.is_empty() {
                            "(not set)".to_string()
                        } else {
                            self.settings.custom_hotkey.clone()
                        };
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.pending_custom_hotkey)
                                .hint_text(current)
                                .desired_width(200.0),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.apply_custom_hotkey();
                        }
                        if ui.button(if self.pending_custom_hotkey.is_empty() {
                            "Record"
                        } else {
                            "Apply"
                        }).clicked() {
                            if self.pending_custom_hotkey.is_empty() {
                                self.start_recording_custom_hotkey();
                            } else {
                                self.apply_custom_hotkey();
                            }
                        }
                        if !self.settings.custom_hotkey.is_empty() && ui.button("Clear").clicked() {
                            self.settings.custom_hotkey.clear();
                            let _ = self.settings.save(&self.db);
                            self.update_hotkeys_from_settings();
                            self.pending_custom_hotkey.clear();
                        }
                    });
                    if !self.settings.custom_hotkey.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("Current: {}", self.settings.custom_hotkey))
                                .color(crate::theme::text_secondary(is_dark))
                                .size(10.0),
                        );
                    }
                }
            }

            // Registration status
            if let Some(service) = &self.hotkey_service {
                let results = service.registration_results();
                if !results.is_empty() {
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
                            let display = format!("{}", result.hotkey);
                            ui.label(egui::RichText::new(display).color(color));
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
            }
        });
    }

    /// Updates hotkeys from current settings (handles Custom preset).
    /// Returns `true` if all requested hotkeys registered successfully (or none were requested).
    pub(super) fn update_hotkeys_from_settings(&mut self) -> bool {
        if let Some(service) = &mut self.hotkey_service {
            let hotkeys = self.settings.effective_hotkeys();
            let results = service.update_hotkeys(&hotkeys);
            results.iter().all(|r| r.success)
        } else {
            false
        }
    }

    /// Starts recording a custom hotkey.
    pub(super) fn start_recording_custom_hotkey(&mut self) {
        self.recording_custom_hotkey = true;
        self.pending_custom_hotkey.clear();
    }

    /// Applies the pending custom hotkey.
    pub(super) fn apply_custom_hotkey(&mut self) {
        if let Some(parsed) = crate::settings::ParsedHotkey::parse(&self.pending_custom_hotkey) {
            self.settings.custom_hotkey = parsed.format();
            if let Err(e) = self.settings.save(&self.db) {
                eprintln!("Failed to save settings: {e}");
            }
            if self.update_hotkeys_from_settings() {
                self.show_toast(&format!(
                    "{} Custom shortcut set to {}",
                    icons::CHECK,
                    self.settings.custom_hotkey
                ));
            } else {
                self.show_toast(&format!(
                    "{} Could not register {} (may be in use by OS)",
                    icons::WARNING,
                    self.settings.custom_hotkey
                ));
            }
        } else {
            self.show_toast(&format!("{} Invalid hotkey format", icons::WARNING));
        }
        self.recording_custom_hotkey = false;
        self.pending_custom_hotkey.clear();
    }

    fn render_behavior_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.group(|ui| {
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
                        .color(crate::theme::warning_color(is_dark)),
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
                                self.db.set_max_history(limit);
                                let _ = self.db.prune_history(limit);
                                let _ = self.settings.save(&self.db);
                                self.refresh_items();
                            }
                        }
                    });
            });
        });
    }

    fn render_security_settings_group(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.group(|ui| {
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
                let _ = self.settings.save(&self.db);
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
                let _ = self.settings.save(&self.db);
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Auto-clear copied secrets after:");
                let mut clear_secs = self.settings.auto_clear_secret_secs;
                let selected_label = if clear_secs == 0 {
                    "Off".to_string()
                } else {
                    format!("{clear_secs}s")
                };
                egui::ComboBox::from_id_salt("auto_clear_secret_combo")
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for secs in [0_u64, 15, 30, 45, 60, 120] {
                            let label = if secs == 0 {
                                "Off".to_string()
                            } else {
                                format!("{secs}s")
                            };
                            if ui.selectable_value(&mut clear_secs, secs, label).clicked() {
                                self.settings.auto_clear_secret_secs = secs;
                                let _ = self.settings.save(&self.db);
                            }
                        }
                    });
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Clear All History", icons::TRASH))
                    .clicked()
                {
                    self.show_clear_confirm_dialog = true;
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Keyring Info", icons::SHIELD_CHECK))
                    .clicked()
                {
                    self.settings.keyring_explanation_shown = false;
                    self.show_keyring_explanation = true;
                }
            });
        });
    }

    fn render_about_settings_group(ui: &mut egui::Ui, is_dark: bool) {
        ui.group(|ui| {
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
