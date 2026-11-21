use crate::platform::SystemTrayBackend;
use eframe::egui;
use std::sync::mpsc::Sender;

/// Actions triggered by the OS system tray icon or its context menu.
#[allow(dead_code)]
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
#[derive(Debug)]
pub(crate) struct TrayService {
    backend: SystemTrayBackend,
}

impl TrayService {
    /// Initializes the system tray icon where supported by the OS and wires click/menu
    /// events to `tray_tx` + `ctx.request_repaint()`.
    ///
    /// Must be called on the main UI thread while the `winit` event loop is active.
    /// Returns `None` on unsupported platforms or if tray initialization fails.
    pub(crate) fn new(tray_tx: &Sender<TrayAction>, ctx: &egui::Context) -> Option<Self> {
        SystemTrayBackend::new(tray_tx, ctx).map(|backend| Self { backend })
    }

    /// True only while the shell still has a registered notification-area icon.
    /// Rechecked when hiding in case Explorer restarted or the icon was removed.
    pub(crate) fn is_live(&self) -> bool {
        self.backend.is_live()
    }
}

/// Decodes the bundled application PNG into `(rgba_bytes, width, height)` for the tray icon.
///
/// # Errors
/// Returns an image decoding error if the bundled PNG cannot be read.
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
