use eframe::egui;
use std::sync::mpsc::Sender;

/// Actions triggered by the Windows system tray icon or its context menu.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Show,
    OpenSettings,
    Quit,
}

/// Manages the OS system tray icon lifecycle.
///
/// Currently implemented on Windows so users can always restore or quit the app
/// even when the window is hidden or global hotkey registration fails.
pub(crate) struct TrayService {
    #[cfg(target_os = "windows")]
    _tray_icon: tray_icon::TrayIcon,
}

impl std::fmt::Debug for TrayService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrayService").finish_non_exhaustive()
    }
}

impl TrayService {
    /// Initializes the system tray icon on Windows and wires click/menu events
    /// to `tray_tx` + `ctx.request_repaint()`.
    ///
    /// Must be called on the main UI thread while the `winit` event loop is active.
    /// Returns `None` on non-Windows platforms or if tray initialization fails.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub(crate) fn new(tray_tx: &Sender<TrayAction>, ctx: &egui::Context) -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            match Self::init_windows(tray_tx.clone(), ctx.clone()) {
                Ok(service) => Some(service),
                Err(e) => {
                    eprintln!("⚠️ Failed to initialize Windows system tray icon: {e}");
                    crate::platform::log_message(
                        "WARN",
                        &format!("Failed to initialize Windows system tray icon: {e}"),
                    );
                    None
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    #[cfg(target_os = "windows")]
    fn init_windows(
        tray_tx: Sender<TrayAction>,
        ctx: egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

        let (rgba, width, height) = load_tray_icon_rgba()?;
        let icon = Icon::from_rgba(rgba, width, height)?;

        let menu = Menu::new();
        let show_item = MenuItem::new("Show CipherClipboard", true, None);
        let settings_item = MenuItem::new("Settings...", true, None);
        let separator = PredefinedMenuItem::separator();
        let quit_item = MenuItem::new("Quit CipherClipboard", true, None);

        menu.append_items(&[&show_item, &settings_item, &separator, &quit_item])?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("CipherClipboard")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()?;

        // `build()` ignores Shell_NotifyIconW(NIM_ADD) failures and only retries
        // on TaskbarCreated. Without this check a "successful" tray init can hide
        // the window with no icon and no taskbar button left to restore it.
        // Allow a brief window for the shell to publish the icon rect.
        let mut icon_live = false;
        for _ in 0..5 {
            if tray_icon.rect().is_some() {
                icon_live = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !icon_live {
            return Err("Windows did not accept the tray icon (shell not ready)".into());
        }

        let tx_click = tray_tx.clone();
        let ctx_click = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            let should_show = matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            );
            if should_show {
                // Remember where the user was before we take focus, so focus
                // restore / auto-paste / elevation checks target the right app.
                crate::platform::capture_active_window();
                // Only signal the UI thread. Activation must happen there: this
                // callback runs on tray-icon's helper thread, which does not own
                // the winit window and cannot set it foreground reliably.
                let _ = tx_click.send(TrayAction::Show);
                ctx_click.request_repaint();
            }
        }));

        let show_id = show_item.id().clone();
        let settings_id = settings_item.id().clone();
        let quit_id = quit_item.id().clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == show_id {
                crate::platform::capture_active_window();
                let _ = tray_tx.send(TrayAction::Show);
                ctx.request_repaint();
            } else if event.id == settings_id {
                let _ = tray_tx.send(TrayAction::OpenSettings);
                ctx.request_repaint();
            } else if event.id == quit_id {
                let _ = tray_tx.send(TrayAction::Quit);
                ctx.request_repaint();
            }
        }));

        Ok(Self {
            _tray_icon: tray_icon,
        })
    }

    /// True only while the shell still has a registered notification-area icon.
    /// Rechecked when hiding in case Explorer restarted or the icon was removed.
    #[cfg(target_os = "windows")]
    pub(crate) fn is_live(&self) -> bool {
        self._tray_icon.rect().is_some()
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code, clippy::unused_self)]
    pub(crate) const fn is_live(&self) -> bool {
        false
    }
}

#[cfg(target_os = "windows")]
impl Drop for TrayService {
    fn drop(&mut self) {
        use tray_icon::menu::MenuEvent;
        use tray_icon::TrayIconEvent;
        // These handlers are process-global and hold cloned channels/contexts.
        TrayIconEvent::set_event_handler::<fn(tray_icon::TrayIconEvent)>(None);
        MenuEvent::set_event_handler::<fn(MenuEvent)>(None);
    }
}

/// Decodes the bundled application PNG into `(rgba_bytes, width, height)` for the tray icon.
#[allow(dead_code)]
pub(crate) fn load_tray_icon_rgba() -> Result<(Vec<u8>, u32, u32), image::ImageError> {
    // Windows notification-area icons are 16x16/32x32. Feeding the full 512x512
    // PNG to CreateIcon makes the shell downscale it (blurry, and it can fail).
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))?
        .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    Ok((img.into_raw(), 32, 32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tray_icon_rgba_succeeds() {
        let (rgba, width, height) = load_tray_icon_rgba().expect("bundled icon.png should decode");
        assert!(width > 0 && height > 0);
        assert_eq!(rgba.len(), (width * height * 4) as usize);
    }
}
