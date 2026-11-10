use crate::db::{ClipboardItem, Database};
use crate::hotkey::HotkeyService;
use arboard::Clipboard;
use eframe::egui;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub struct ClipboardApp {
    db: Database,
    search_query: String,
    items: Vec<ClipboardItem>,
    clipboard_writer: Option<Clipboard>,
    toast: Option<(String, Instant)>,
    last_query_cached: String,
    notify_rx: Receiver<()>,
    hotkey_rx: Receiver<()>,
    _hotkey_service: Option<HotkeyService>,
    is_visible: bool,
    hide_on_copy: bool,
    focus_search_input: bool,
}

impl ClipboardApp {
    pub fn new(
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

    fn copy_item_to_clipboard(&mut self, content: &str, ctx: &egui::Context) {
        if let Some(cb) = &mut self.clipboard_writer {
            if let Err(e) = cb.set_text(content.to_string()) {
                eprintln!("Failed to write to clipboard: {e}");
            } else {
                self.show_toast("✓ Copied to clipboard!");
                if self.hide_on_copy {
                    self.is_visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }
    }
}

impl eframe::App for ClipboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Check for global hotkey toggle trigger
        let mut toggle_requested = false;
        while self.hotkey_rx.try_recv().is_ok() {
            toggle_requested = true;
        }

        if toggle_requested {
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

        // 2. Hide window if Escape key is pressed
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.is_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 3. Drain new clipboard entries from background monitor
        let mut new_items_detected = false;
        while self.notify_rx.try_recv().is_ok() {
            new_items_detected = true;
        }

        if new_items_detected || self.search_query != self.last_query_cached {
            self.refresh_items();
            self.last_query_cached = self.search_query.clone();
        }

        // Top Panel: Header, Actions, and Search Bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(6.0);
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

                    if ui.small_button("🗑 Clear").on_hover_text("Permanently deletes all history").clicked() {
                        let _ = self.db.clear();
                        self.refresh_items();
                        self.show_toast("History cleared.");
                    }
                });
            });

            ui.add_space(4.0);

            // Search bar with auto-focus support
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

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.hide_on_copy, "Close window after copy");
            });

            ui.add_space(4.0);
        });

        // Bottom Panel: Status / Toast Bar
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some((msg, time)) = &self.toast {
                    if time.elapsed() < Duration::from_secs(2) {
                        ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(46, 204, 113)).strong());
                    } else {
                        self.toast = None;
                    }
                } else {
                    ui.label(
                        egui::RichText::new(format!("{} entries • SQLCipher encrypted", self.items.len()))
                            .weak()
                            .size(11.0),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Press Esc to hide")
                            .weak()
                            .size(11.0),
                    );
                });
            });
        });

        // Central Panel: Clipboard History List
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("No clipboard history found.").weak());
                    ui.label(egui::RichText::new("Press Cmd+C in any app to save text here!").size(12.0).weak());
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut item_to_delete: Option<i64> = None;
                    let mut item_to_copy: Option<String> = None;

                    for item in &self.items {
                        egui::Frame::group(ui.style())
                            .rounding(6.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Timestamp
                                    ui.label(egui::RichText::new(&item.created_at).size(10.0).weak());

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("✕").on_hover_text("Delete entry").clicked() {
                                            item_to_delete = Some(item.id);
                                        }
                                        if ui.small_button("📋 Copy").on_hover_text("Copy back to clipboard").clicked() {
                                            item_to_copy = Some(item.content.clone());
                                        }
                                    });
                                });

                                ui.add_space(2.0);

                                // Truncated preview
                                let preview = if item.content.len() > 180 {
                                    format!("{}...", &item.content[..180])
                                } else {
                                    item.content.clone()
                                };

                                let label = ui.add(
                                    egui::Label::new(egui::RichText::new(preview).monospace())
                                        .sense(egui::Sense::click()),
                                );

                                if label.clicked() {
                                    item_to_copy = Some(item.content.clone());
                                }
                                if label.hovered() {
                                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                                }
                            });

                        ui.add_space(4.0);
                    }

                    // Handle deferred actions outside iteration
                    if let Some(content) = item_to_copy {
                        self.copy_item_to_clipboard(&content, ctx);
                    }
                    if let Some(id) = item_to_delete {
                        let _ = self.db.delete(id);
                        self.refresh_items();
                        self.show_toast("Entry deleted.");
                    }
                });
        });
    }
}
