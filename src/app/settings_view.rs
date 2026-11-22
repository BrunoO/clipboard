use super::{ActiveView, ClipboardApp};
use crate::icons;
use crate::settings::HotkeyPreset;
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

            self.render_hotkey_preset_selector(ui);

            if crate::platform::supports_hotkey_layout_redetect() {
                self.render_keyboard_layout_redetect_button(ui);
            }

            if self.settings.hotkey == HotkeyPreset::Custom {
                self.render_custom_hotkey_editor(ui, is_dark);
            }

            self.render_hotkey_registration_status(ui, is_dark);
        });
    }

    fn render_hotkey_preset_selector(&mut self, ui: &mut egui::Ui) {
        let current_preset = self.settings.hotkey;
        for &preset in HotkeyPreset::all_presets() {
            let is_selected = current_preset == preset;
            let mut selected = is_selected;
            if ui
                .radio_value(&mut selected, true, preset.display_name())
                .clicked()
                && !is_selected
            {
                self.settings.hotkey = preset;
                let saved = self.persist_settings();
                let registered = self.update_hotkeys_from_settings();
                if !registered {
                    self.show_warning_toast(&format!(
                        "{} Could not register {} (may be in use by OS)",
                        icons::WARNING,
                        preset.badge()
                    ));
                } else if saved {
                    self.show_toast(&format!(
                        "{} Shortcut set to {}",
                        icons::CHECK,
                        preset.badge()
                    ));
                }
            }
        }
    }

    fn render_keyboard_layout_redetect_button(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        if ui.button("Re-detect keyboard layout").clicked() {
            if self.update_hotkeys_from_settings() {
                self.show_toast(&format!(
                    "{} Hotkeys re-registered for current keyboard layout",
                    icons::CHECK
                ));
            } else {
                self.show_warning_toast(&format!(
                    "{} Could not re-register hotkeys (may be in use by OS)",
                    icons::WARNING
                ));
            }
        }
    }

    fn render_custom_hotkey_editor(&mut self, ui: &mut egui::Ui, is_dark: bool) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if self.dialogs.recording_custom_hotkey {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Press desired key combination…")
                        .color(crate::theme::accent_color(is_dark))
                        .strong(),
                );
                if ui.button("Cancel").clicked() {
                    self.dialogs.recording_custom_hotkey = false;
                    self.dialogs.pending_custom_hotkey.clear();
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Press Esc to cancel, or Backspace to clear")
                    .color(crate::theme::text_secondary(is_dark))
                    .size(10.0),
            );
            return;
        }

        ui.horizontal(|ui| {
            let current = if self.settings.custom_hotkey.is_empty() {
                "(not set)".to_string()
            } else {
                self.settings.custom_hotkey.clone()
            };
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.dialogs.pending_custom_hotkey)
                    .hint_text(current)
                    .desired_width(200.0),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.apply_custom_hotkey();
            }
            let btn_label = if self.dialogs.pending_custom_hotkey.is_empty() {
                "Record"
            } else {
                "Apply"
            };
            if ui.button(btn_label).clicked() {
                if self.dialogs.pending_custom_hotkey.is_empty() {
                    self.start_recording_custom_hotkey();
                } else {
                    self.apply_custom_hotkey();
                }
            }
            if !self.settings.custom_hotkey.is_empty() && ui.button("Clear").clicked() {
                self.settings.custom_hotkey.clear();
                self.persist_settings();
                self.update_hotkeys_from_settings();
                self.dialogs.pending_custom_hotkey.clear();
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
                ui.label(egui::RichText::new(format!("{}", result.hotkey)).color(color));
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

    /// Updates hotkeys from current settings (handles Custom preset).
    /// Returns `true` if all requested hotkeys registered successfully (or none were requested).
    pub(super) fn update_hotkeys_from_settings(&mut self) -> bool {
        if let Some(service) = &mut self.services.hotkey_service {
            let hotkeys = self.settings.effective_hotkeys();
            let results = service.update_hotkeys(&hotkeys);
            results.iter().all(|r| r.success)
        } else {
            false
        }
    }

    /// Starts recording a custom hotkey.
    pub(super) fn start_recording_custom_hotkey(&mut self) {
        self.dialogs.recording_custom_hotkey = true;
        self.dialogs.pending_custom_hotkey.clear();
    }

    /// Applies the pending custom hotkey.
    pub(super) fn apply_custom_hotkey(&mut self) {
        if let Some(parsed) =
            crate::settings::ParsedHotkey::parse(&self.dialogs.pending_custom_hotkey)
        {
            self.settings.custom_hotkey = parsed.format();
            let saved = self.persist_settings();
            let registered = self.update_hotkeys_from_settings();
            if !registered {
                self.show_warning_toast(&format!(
                    "{} Could not register {} (may be in use by OS)",
                    icons::WARNING,
                    self.settings.custom_hotkey
                ));
            } else if saved {
                self.show_toast(&format!(
                    "{} Custom shortcut set to {}",
                    icons::CHECK,
                    self.settings.custom_hotkey
                ));
            }
        } else {
            self.show_warning_toast(&format!("{} Invalid hotkey format", icons::WARNING));
        }
        self.dialogs.recording_custom_hotkey = false;
        self.dialogs.pending_custom_hotkey.clear();
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

            ui.add_space(4.0);
            if ui
                .checkbox(
                    &mut self.settings.preserve_secrets_on_prune,
                    "Preserve secrets during history pruning",
                )
                .on_hover_text(
                    "Prevents passwords, API keys, and generated secrets from being automatically deleted when history reaches the item limit.",
                )
                .changed()
            {
                self.db
                    .set_preserve_secrets_on_prune(self.settings.preserve_secrets_on_prune);
                self.persist_settings();
                self.refresh_items();
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
