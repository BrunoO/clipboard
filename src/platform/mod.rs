//! Cross-platform focus restoration and simulated paste automation.
//!
//! When `CipherClipboard` is summoned via a global hotkey, it takes OS window focus.
//! This module captures the active application/window *before* `CipherClipboard` takes
//! focus, and restores focus and simulates `Cmd+V` (macOS) or `Ctrl+V` (Windows/Linux)
//! when an item is chosen.

#![allow(clippy::missing_const_for_fn)]

use std::time::Duration;

pub(crate) mod fs_security;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(target_os = "linux")]
pub(crate) use linux::QuoteHotkeyFallback;

/// Native shell path quoting style for the current operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ShellPathStyle {
    /// POSIX `sh`/`zsh`/`bash` single-quoted path (`'...'`, escaping `'` as `'\''`).
    Posix,
    /// Windows `cmd` / PowerShell double-quoted path (`"..."`).
    Windows,
}

/// Default shell path quoting style for the current operating system.
#[cfg(target_os = "windows")]
pub(crate) const DEFAULT_SHELL_PATH_STYLE: ShellPathStyle = ShellPathStyle::Windows;
#[cfg(not(target_os = "windows"))]
pub(crate) const DEFAULT_SHELL_PATH_STYLE: ShellPathStyle = ShellPathStyle::Posix;

/// No-op Quote-key global hotkey fallback on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub(crate) struct QuoteHotkeyFallback;

// This type mirrors the Linux fallback's instance API so callers stay platform-agnostic.
#[cfg(not(target_os = "linux"))]
#[allow(clippy::unused_self, clippy::needless_pass_by_ref_mut)]
impl QuoteHotkeyFallback {
    #[must_use]
    pub(crate) fn new(
        _hotkey_tx: std::sync::mpsc::SyncSender<()>,
        _paste_next_tx: crossbeam_channel::Sender<()>,
        _ctx: eframe::egui::Context,
    ) -> Self {
        Self
    }

    #[must_use]
    pub(crate) const fn try_register(
        &self,
        _hk: global_hotkey::hotkey::HotKey,
        _action: crate::hotkey::HotkeyAction,
    ) -> bool {
        false
    }

    pub(crate) const fn clear(&self) {}

    pub(crate) const fn shutdown(&mut self) {}
}

/// Platform-backed system tray icon handle (implemented on Windows, no-op elsewhere).
pub(crate) struct SystemTrayBackend {
    #[cfg(target_os = "windows")]
    tray: windows::WindowsTray,
}

impl std::fmt::Debug for SystemTrayBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemTrayBackend").finish_non_exhaustive()
    }
}

impl SystemTrayBackend {
    /// Initializes the OS system tray icon where supported (Windows) and wires click/menu
    /// events to `tray_tx` + `ctx.request_repaint()`.
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
    pub(crate) fn new(
        tray_tx: &std::sync::mpsc::Sender<crate::tray::TrayAction>,
        ctx: &eframe::egui::Context,
    ) -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            match Self::init_windows(tray_tx.clone(), ctx.clone()) {
                Ok(service) => Some(service),
                Err(e) => {
                    eprintln!("⚠️ Failed to initialize Windows system tray icon: {e}");
                    crate::logging::log_message(
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
        tray_tx: std::sync::mpsc::Sender<crate::tray::TrayAction>,
        ctx: eframe::egui::Context,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (rgba, width, height) = crate::tray::load_tray_icon_rgba()?;
        let tray = windows::WindowsTray::new(&rgba, width, height, tray_tx, ctx)
            .map_err(std::io::Error::other)?;
        Ok(Self { tray })
    }

    /// True only while the shell still has a registered notification-area icon.
    /// Rechecked when hiding in case Explorer restarted or the icon was removed.
    #[cfg(target_os = "windows")]
    pub(crate) fn is_live(&self) -> bool {
        self.tray.is_live()
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code, clippy::unused_self)]
    pub(crate) fn is_live(&self) -> bool {
        false
    }
}

/// Stores a clone of the `egui::Context` to immediately trigger repaints on OS activation.
pub(crate) fn set_repaint_context(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "macos")]
    macos::set_repaint_context(ctx);
    #[cfg(target_os = "windows")]
    windows::set_repaint_context(ctx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = ctx;
}

/// Registers the root viewport's native window handle from `eframe::CreationContext`
/// so platform helpers (e.g. Windows tray wakeup and activation checks) never need to
/// rediscover the main window by title enumeration.
pub(crate) fn register_main_window(cc: &eframe::CreationContext<'_>) {
    #[cfg(target_os = "windows")]
    windows::register_main_window(cc);
    #[cfg(not(target_os = "windows"))]
    let _ = cc;
}

/// Returns true if the application currently has OS focus/activation.
#[cfg(test)]
#[must_use]
pub(crate) fn is_app_active() -> bool {
    is_app_active_state().unwrap_or(false)
}

/// Returns `Some(true)` if this application is currently active at the OS/WM level,
/// `Some(false)` if another application is active, or `None` if the platform/session
/// cannot determine active-window ownership (e.g. pure Wayland without X11).
#[must_use]
#[cfg_attr(
    any(target_os = "macos", target_os = "windows"),
    allow(clippy::unnecessary_wraps)
)]
pub(crate) fn is_app_active_state() -> Option<bool> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Some(backend::is_active())
    }
    #[cfg(target_os = "linux")]
    {
        backend::is_active()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// Returns `true` on Windows when the current keyboard layout produces `ù` for the
/// physical key in the US `Quote` position (French/Belgian AZERTY layouts).
#[must_use]
pub(crate) fn quote_key_is_ugrave() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::quote_key_is_ugrave()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Returns `true` when the platform requires explicit OS Accessibility permission for
/// simulated auto-paste keystrokes and that permission has not yet been granted (macOS).
#[must_use]
pub(crate) fn auto_paste_permission_missing() -> bool {
    #[cfg(target_os = "macos")]
    {
        !macos::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Returns `true` if the platform natively supports extracting text from images (Image OCR).
/// Supported on macOS (Apple Vision framework) and Windows 10/11 (`WinRT` `Windows.Media.Ocr`).
#[must_use]
pub(crate) fn supports_image_ocr() -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        backend::supports_image_ocr()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Extracts plain text from PNG-encoded image bytes using the platform's native OCR framework.
///
/// # Errors
/// Returns an error message if OCR is unsupported on the platform, if the image cannot
/// be decoded, or if OCR processing fails.
pub(crate) fn extract_text_from_png(png_bytes: &[u8]) -> Result<String, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        backend::extract_text_from_png(png_bytes)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = png_bytes;
        Err("Image OCR is only supported on macOS and Windows".to_string())
    }
}

/// Prompts the user for OS Accessibility permission required by auto-paste (macOS).
pub(crate) fn prompt_auto_paste_permission() {
    #[cfg(target_os = "macos")]
    macos::prompt_permission();
}

/// Opens the OS system settings pane for Accessibility permissions (macOS).
pub(crate) fn open_auto_paste_permission_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}

/// Applies the platform title-bar dark mode attribute where supported (Windows DWM).
pub(crate) fn set_title_bar_dark_mode(is_dark: bool) {
    #[cfg(target_os = "windows")]
    {
        windows::set_title_bar_dark_mode(is_dark);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = is_dark;
    }
}

/// Displays a native OS error dialog on startup failure where supported (Windows `MessageBoxW`).
pub(crate) fn show_startup_error_dialog(message: &str) {
    #[cfg(target_os = "windows")]
    windows::show_error_dialog(message);
    #[cfg(not(target_os = "windows"))]
    let _ = message;
}

/// Displays a native OS crash dialog inside the panic hook on release builds where supported (Windows).
pub(crate) fn show_panic_error_dialog(summary: &str) {
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    windows::show_error_dialog(&format!(
        "CipherClipboard encountered an unexpected error and must close.\n\n\
         {summary}\n\n\
         Diagnostics were written to %LOCALAPPDATA%\\cipherclipboard\\app.log."
    ));
    #[cfg(not(all(target_os = "windows", not(debug_assertions))))]
    let _ = summary;
}

/// Attempts to bring an already-running `CipherClipboard` instance to the foreground.
pub(crate) fn activate_existing_instance(pid: Option<u32>) {
    #[cfg(target_os = "windows")]
    {
        let _ = windows::activate_existing_instance(pid);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = pid;
}

/// Applies platform-specific window chrome options to the root `egui::ViewportBuilder`
/// (macOS full-size content view with hidden titlebar).
#[must_use]
pub(crate) fn configure_viewport_chrome(
    viewport: eframe::egui::ViewportBuilder,
) -> eframe::egui::ViewportBuilder {
    #[cfg(target_os = "macos")]
    {
        viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        viewport
    }
}

/// Returns the platform-appropriate `wgpu` backend set when `WGPU_BACKEND` is not set.
///
/// On Windows, `wgpu::Backends::PRIMARY` tries Vulkan before Direct3D 12; several Windows
/// Vulkan drivers (notably Intel iGPU Vulkan) flicker the window on every repaint (such as
/// mouse movement). Defaulting to `DX12` on Windows avoids Vulkan swapchain flicker while
/// still allowing `WGPU_BACKEND` overrides.
#[must_use]
const fn default_wgpu_backends() -> eframe::wgpu::Backends {
    #[cfg(target_os = "windows")]
    {
        eframe::wgpu::Backends::DX12
    }
    #[cfg(not(target_os = "windows"))]
    {
        eframe::wgpu::Backends::PRIMARY.union(eframe::wgpu::Backends::GL)
    }
}

/// Builds the `eframe` `wgpu` configuration tailored for a lightweight 2D desktop utility.
///
/// Prefers `LowPower` (the integrated GPU driving the display) across platforms to avoid
/// hybrid-GPU cross-adapter DWM presentation flicker and unnecessary discrete GPU wakeups,
/// and prefers `DX12` on Windows unless overridden via `WGPU_BACKEND` / `WGPU_POWER_PREF`.
#[must_use]
pub(crate) fn configure_wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut create_new) = config.wgpu_setup {
        create_new.instance_descriptor.backends =
            eframe::wgpu::Backends::from_env().unwrap_or_else(default_wgpu_backends);
        create_new.power_preference = eframe::wgpu::PowerPreference::from_env()
            .unwrap_or(eframe::wgpu::PowerPreference::LowPower);
    }
    config
}

/// Returns `true` if `eframe::App::on_exit` must terminate the process immediately after
/// deterministic resource cleanup to avoid `AppKit` Touch Bar KVO teardown crashes (macOS).
#[must_use]
pub(crate) const fn requires_immediate_exit_on_quit() -> bool {
    cfg!(target_os = "macos")
}

/// Returns `true` when the `Cmd` / `Super` modifier should be recorded in custom hotkeys
/// from `egui::Modifiers` (macOS `mac_cmd` / `command`).
#[must_use]
pub(crate) const fn has_recorded_command_modifier(modifiers: eframe::egui::Modifiers) -> bool {
    cfg!(target_os = "macos") && (modifiers.mac_cmd || modifiers.command)
}

/// Selects a platform-specific constant at compile time (`macos`, `windows`, or `other`).
#[must_use]
pub(crate) const fn platform_select<T: Copy>(macos: T, windows: T, other: T) -> T {
    let _ = (macos, windows, other);
    #[cfg(target_os = "macos")]
    {
        macos
    }
    #[cfg(target_os = "windows")]
    {
        windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        other
    }
}

/// Platform-specific guidance for locating the `SQLCipher` encryption key in the OS keyring.
pub(crate) const KEYRING_RECOVERY_HINT: &str = platform_select(
    "Key (sensitive): Keychain Access or `security find-generic-password -s cipherclipboard -a db_encryption_key -w`",
    "Key (sensitive): Credential Manager → Windows Credentials → service `cipherclipboard`, account `db_encryption_key`",
    "Key (sensitive): Secret Service/Seahorse or `secret-tool lookup service cipherclipboard username db_encryption_key`",
);

/// Human-readable name of the primary shortcut modifier key (`Cmd` on macOS, `Ctrl` elsewhere).
pub(crate) const PRIMARY_MODIFIER_NAME: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else {
    "Ctrl"
};

/// Human-readable name of the OS `Meta` / `Super` modifier key (`Cmd` on macOS, `Win` elsewhere).
pub(crate) const META_MODIFIER_NAME: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else {
    "Win"
};

/// Re-entrancy guard set while `paste-next-worker` is injecting synthetic paste keystrokes,
/// so the global hotkey listener ignores any self-triggered Paste Next events.
static PASTE_NEXT_IN_FLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Returns `true` while `paste-next-worker` is simulating a paste keystroke.
#[must_use]
pub(crate) fn is_paste_next_in_flight() -> bool {
    PASTE_NEXT_IN_FLIGHT.load(std::sync::atomic::Ordering::Acquire)
}

/// Sets or clears the `PASTE_NEXT_IN_FLIGHT` re-entrancy guard.
pub(crate) fn set_paste_next_in_flight(in_flight: bool) {
    PASTE_NEXT_IN_FLIGHT.store(in_flight, std::sync::atomic::Ordering::Release);
}

/// Returns the platform default for `hide_on_blur` (`false` on Windows where clicking outside
/// while interacting with the taskbar/tray is common, `true` elsewhere).
#[must_use]
pub(crate) const fn default_hide_on_blur() -> bool {
    !cfg!(target_os = "windows")
}

/// Returns the platform default for `minimize_to_taskbar` (`true` on Windows so
/// the app remains reachable via Taskbar & Alt+Tab by default, `false` elsewhere).
#[must_use]
pub(crate) const fn default_minimize_to_taskbar() -> bool {
    cfg!(target_os = "windows")
}

/// Returns true if the current platform supports minimizing to the taskbar/Alt+Tab on dismiss.
#[must_use]
pub(crate) const fn supports_minimize_to_taskbar() -> bool {
    cfg!(target_os = "windows")
}

/// Pure decision helper for whether window dismissal should minimize rather than hide.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[must_use]
pub(crate) const fn should_minimize_on_hide(
    has_live_tray: bool,
    minimize_to_taskbar: bool,
) -> bool {
    minimize_to_taskbar || !has_live_tray
}

/// Returns the user-facing navigation hint displayed in the bottom bar.
#[must_use]
pub(crate) const fn navigation_hint_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "↑/↓ or Tab • Enter • Cmd+1..9 • Esc"
    } else {
        "↑/↓ or Tab • Enter • Ctrl+1..9 • Esc"
    }
}

/// Returns the tooltip for the header Quit button.
#[must_use]
#[allow(dead_code)]
pub(crate) const fn quit_shortcut_tooltip() -> &'static str {
    if cfg!(target_os = "macos") {
        "Quit CipherClipboard (Cmd+Q)"
    } else {
        "Quit CipherClipboard (Ctrl+Q)"
    }
}

/// Formats an item quick-copy shortcut badge (e.g. `Cmd+1` on macOS, `Ctrl+1` elsewhere).
#[must_use]
pub(crate) fn format_item_shortcut(num: usize) -> String {
    format!("{PRIMARY_MODIFIER_NAME}+{num}")
}

/// Returns `true` when the platform's primary shortcut modifier is pressed in `egui`.
#[must_use]
pub(crate) const fn is_primary_shortcut_modifier(modifiers: eframe::egui::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.command
    } else {
        modifiers.ctrl
    }
}

/// Builds candidate font paths under the Windows `Fonts` directory.
#[cfg(any(target_os = "windows", test))]
#[must_use]
pub(crate) fn windows_font_candidates(windir: Option<&str>) -> Vec<std::path::PathBuf> {
    let base = windir
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(r"C:\Windows");
    let fonts_dir = std::path::PathBuf::from(base).join("Fonts");
    ["segoeui.ttf", "segoeuil.ttf", "arial.ttf"]
        .into_iter()
        .map(|name| fonts_dir.join(name))
        .collect()
}

/// Returns candidate paths for native system UI fonts on the current platform
/// (San Francisco on macOS, Segoe UI on Windows, `DejaVu` Sans on Linux).
#[must_use]
pub(crate) fn system_font_candidates() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/SFNSText.ttf",
            "/System/Library/Fonts/SFCompact.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "/System/Library/Fonts/Helvetica.ttc",
            "/Library/Fonts/Arial.ttf",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect()
    }
    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var("WINDIR")
            .ok()
            .or_else(|| std::env::var("SystemRoot").ok());
        windows_font_candidates(windir.as_deref())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect()
    }
}

use crate::logging::log_message;

/// Returns a monotonic OS clipboard change counter when supported by the platform
/// (macOS `NSPasteboard.changeCount`, Windows `GetClipboardSequenceNumber`), allowing
/// the monitor thread to skip reading/copying unchanged clipboard payloads.
#[must_use]
pub(crate) fn clipboard_change_token() -> Option<u64> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        backend::clipboard_change_token()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Checks if the current clipboard content is marked as concealed or transient
/// by password managers (e.g. 1Password, Bitwarden, `KeePass`, `KeePassXC`, macOS Keychain).
#[must_use]
pub(crate) fn is_clipboard_concealed() -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        backend::is_clipboard_concealed()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// Which half of the concealed-clipboard contract a platform can honor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcealedMarkerSupport {
    /// Incoming markers are detected and outgoing secrets are marked natively.
    DetectAndEmit,
    /// Incoming markers are detected, but outgoing secrets cannot be marked natively.
    DetectOnly,
}

/// Returns the platform's concealed-marker capability.
#[must_use]
pub(crate) const fn concealed_marker_support() -> ConcealedMarkerSupport {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        ConcealedMarkerSupport::DetectAndEmit
    } else {
        ConcealedMarkerSupport::DetectOnly
    }
}

/// Returns a user-facing explanation when native marker emission is unavailable.
#[must_use]
pub(crate) const fn concealed_marker_limitation() -> Option<&'static str> {
    match concealed_marker_support() {
        ConcealedMarkerSupport::DetectAndEmit => None,
        ConcealedMarkerSupport::DetectOnly => Some(
            "Concealed clipboard items from other apps are detected, but this platform cannot advertise the same marker for secrets CipherClipboard copies. Copied secrets are still masked and auto-cleared.",
        ),
    }
}

/// Windows clipboard formats whose presence marks incoming content as concealed.
#[cfg(any(target_os = "windows", test))]
pub(crate) const WINDOWS_CONCEALED_DETECT_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "Clipboard Viewer Ignore",
    "CanIncludeInClipboardHistory",
];

/// Windows clipboard formats emitted to mark outgoing secrets as concealed.
#[cfg(any(target_os = "windows", test))]
pub(crate) const WINDOWS_CONCEALED_EMIT_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadClipboardContentToCloud",
];

/// Pure incoming-marker decision shared by Windows FFI and host-agnostic tests.
#[cfg(any(target_os = "windows", test))]
pub(crate) const fn windows_concealed_detected(
    exclude_available: bool,
    ignore_available: bool,
    history_excluded: bool,
) -> bool {
    exclude_available || ignore_available || history_excluded
}

/// Pure UIPI decision: an unqueryable or higher-integrity target must skip auto-paste.
#[cfg(any(target_os = "windows", test))]
pub(crate) const fn uipi_should_skip_paste(
    target_pid: u32,
    target_level: Option<u32>,
    self_level: u32,
) -> bool {
    if target_pid == 0 {
        return false;
    }
    match target_level {
        None => true,
        Some(level) => level > self_level,
    }
}

/// Attaches OS-native privacy/concealed markers to the current clipboard entry
/// so OS clipboard history, cloud sync, Universal Clipboard, and other clipboard
/// managers do not record or broadcast secrets copied by `CipherClipboard`.
pub(crate) fn mark_clipboard_concealed() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    backend::mark_clipboard_concealed();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = concealed_marker_support();
}

/// Hides the entire application at the OS level (macOS `[NSApp hide:]`).
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn hide_application() {
    #[cfg(target_os = "macos")]
    macos::hide_app();
}

/// Applies the platform-specific window dismissal policy when hiding the UI:
/// - On Windows, minimizes to taskbar if `minimize_to_taskbar` is enabled or if no live tray icon exists;
///   otherwise hides the viewport (`Visible(false)`) cleanly to the system tray.
/// - On macOS, invokes OS-level application hiding (`NSApp.hide`) while keeping the winit viewport
///   marked visible so `eframe` avoids the 100ms invisible-window repaint throttle and can render
///   a fresh Metal frame before unhiding.
/// - On Linux, hides the viewport (`Visible(false)`) and invokes OS-level application hiding.
pub(crate) fn hide_window_viewport(
    ctx: &eframe::egui::Context,
    has_live_tray: bool,
    minimize_to_taskbar: bool,
) {
    #[cfg(target_os = "windows")]
    {
        if should_minimize_on_hide(has_live_tray, minimize_to_taskbar) {
            if !has_live_tray && !minimize_to_taskbar {
                log_message(
                    "WARN",
                    "No live tray icon; minimizing to taskbar instead of hiding",
                );
            }
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Minimized(true));
        } else {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(false));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (ctx, has_live_tray, minimize_to_taskbar);
        hide_application();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (has_live_tray, minimize_to_taskbar);
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(false));
        hide_application();
    }
}

/// Whether `show_window` should let `App::ui()` paint and present one fresh frame before
/// ordering the OS window front (macOS hides via `NSApp.hide`, so painting first eliminates
/// the blank-frame flash when unhiding).
#[must_use]
pub(crate) const fn defer_order_front_until_painted() -> bool {
    cfg!(target_os = "macos")
}

/// Checks whether the captured auto-paste target blocks simulated keystrokes (e.g. due to
/// Windows UIPI when the target runs elevated as Administrator) and returns a user-facing
/// warning toast message when blocked.
#[must_use]
pub(crate) fn auto_paste_blocked_warning() -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    {
        if windows::target_is_elevated() {
            log_message(
                "WARN",
                "Auto-paste blocked: target app runs as Administrator",
            );
            return Some(
                "Auto-paste blocked: target app runs as Administrator. Paste manually (Ctrl+V).",
            );
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Unhides and orders the main window front at the OS level.
pub(crate) fn order_front_main_window() {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    backend::order_front_main_window();
}

/// Checks and clears any pending reopen requests triggered by OS events (e.g. Dock icon click, Windows second instance).
#[must_use]
pub(crate) fn take_reopen_requested() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::take_reopen_requested()
    }
    #[cfg(target_os = "windows")]
    {
        windows::take_reopen_requested()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Registers OS-level activation and reopen observers (e.g. macOS Dock click, Windows second-instance listener).
pub(crate) fn setup_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::setup_reopen_observer();
    #[cfg(target_os = "windows")]
    windows::setup_reopen_observer();
}

/// Removes the OS reopen observer to prevent leaks or deadlocks on app termination.
pub(crate) fn teardown_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::teardown_reopen_observer();
    #[cfg(target_os = "windows")]
    windows::teardown_reopen_observer();
}

/// Captures the current frontmost application/window before `CipherClipboard` gains focus.
pub(crate) fn capture_active_window() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    backend::capture_frontmost();
}

#[derive(Clone, Copy)]
struct PasteRequest {
    restore_focus: bool,
    auto_paste: bool,
    auto_paste_delay_ms: u64,
}

fn restore_focus_platform() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    backend::restore_focus();
}

/// Serializes synthetic paste sequences between `focus-paste-worker` and `paste-next-worker`
/// so their clipboard writes and keystrokes never interleave.
static PASTE_SIMULATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn simulate_paste_if_foreground() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let _guard = PASTE_SIMULATION_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !backend::is_target_foreground() {
            log_message(
                "WARN",
                "Auto-paste skipped: captured target window/application is not in the foreground",
            );
            return;
        }
        #[cfg(target_os = "windows")]
        let _ = backend::simulate_paste();
        #[cfg(not(target_os = "windows"))]
        backend::simulate_paste();
    }
}

fn wait_for_modifiers_released(modifiers: global_hotkey::hotkey::Modifiers, timeout: Duration) {
    if modifiers.is_empty() {
        return;
    }
    let deadline = std::time::Instant::now() + timeout;
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    while backend::are_modifiers_held(modifiers) {
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = deadline;
}

fn simulate_paste_foreground(
    release_modifiers: global_hotkey::hotkey::Modifiers,
) -> Result<(), &'static str> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        backend::simulate_paste_foreground(release_modifiers)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = release_modifiers;
        Err("paste simulation unsupported on this platform")
    }
}

// The sender lives only in `PasteWorker`; shutdown drops it so both the `recv`
// and `recv_timeout` Disconnected arms below become reachable.
fn paste_worker_loop(rx: &std::sync::mpsc::Receiver<PasteRequest>) {
    let mut pending: Option<PasteRequest> = None;
    loop {
        let mut req = if let Some(r) = pending.take() {
            r
        } else {
            let Ok(r) = rx.recv() else {
                break;
            };
            r
        };
        // Coalesce any burst of queued requests, keeping only the latest.
        // A disconnected channel means shutdown already dropped the sender, so
        // do not begin new focus/paste side effects for this request.
        loop {
            match rx.try_recv() {
                Ok(newer) => req = newer,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // If auto_paste is active, focus MUST be switched to the target application
        // so the simulated keystrokes are received by that application.
        if req.restore_focus || req.auto_paste {
            restore_focus_platform();
        }

        if req.auto_paste {
            // Give the OS window server time to switch key focus to the target
            // application before dispatching keystrokes. If a newer request arrives
            // during this window, supersede this paste immediately.
            match rx.recv_timeout(Duration::from_millis(req.auto_paste_delay_ms)) {
                Ok(newer) => {
                    pending = Some(newer);
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            // Shutdown may have dropped the sender just after the delay elapsed.
            // Also honor a request that arrived in that narrow window.
            match rx.try_recv() {
                Ok(newer) => {
                    pending = Some(newer);
                    continue;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }

            simulate_paste_if_foreground();
        }
    }
}

/// Owned paste-worker state. Dropping the sender lets the worker exit through
/// its existing `Disconnected` break path; the join handle is taken exactly once.
#[derive(Debug)]
struct PasteWorker {
    sender: std::sync::Mutex<Option<std::sync::mpsc::Sender<PasteRequest>>>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl PasteWorker {
    fn send(&self, request: PasteRequest) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(request);
        }
    }

    /// Drops the only sender, then joins the worker so in-flight OS side effects complete.
    fn shutdown(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(sender);

        let handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            if handle.join().is_err() {
                log_message("ERROR", "focus-paste worker panicked during shutdown");
            }
        }
    }
}

static PASTE_WORKER: std::sync::OnceLock<PasteWorker> = std::sync::OnceLock::new();

fn paste_worker() -> &'static PasteWorker {
    PASTE_WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<PasteRequest>();
        let handle = std::thread::Builder::new()
            .name("focus-paste-worker".to_string())
            .spawn(move || paste_worker_loop(&rx));
        if let Err(e) = &handle {
            log_message("ERROR", &format!("Failed to spawn focus-paste worker: {e}"));
        }
        PasteWorker {
            sender: std::sync::Mutex::new(Some(tx)),
            handle: std::sync::Mutex::new(handle.ok()),
        }
    })
}

/// Shuts down both paste workers if they were started. Safe to call more than once.
pub(crate) fn shutdown_paste_worker() {
    if let Some(worker) = PASTE_WORKER.get() {
        worker.shutdown();
    }
    if let Some(worker) = PASTE_NEXT_WORKER.get() {
        worker.shutdown();
    }
}

/// Restores focus to the previous application and optionally simulates a paste keystroke.
pub(crate) fn restore_focus_and_paste(
    restore_focus: bool,
    auto_paste: bool,
    auto_paste_delay_ms: u64,
) {
    if !restore_focus && !auto_paste {
        return;
    }

    paste_worker().send(PasteRequest {
        restore_focus,
        auto_paste,
        auto_paste_delay_ms,
    });
}

/// Payload executed by the ordered `paste-next-worker`.
pub(crate) enum PasteNextPayload {
    Text {
        text: zeroize::Zeroizing<String>,
        conceal: bool,
    },
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

/// A single Paste Next unit of work executed strictly in submission order.
pub(crate) struct PasteNextJob {
    pub(crate) seq: u64,
    pub(crate) payload: PasteNextPayload,
    pub(crate) settle_ms: u64,
    pub(crate) release_modifiers: global_hotkey::hotkey::Modifiers,
    pub(crate) db: crate::db::Database,
    pub(crate) ctx: eframe::egui::Context,
}

/// Completion outcome emitted by `paste-next-worker` for `App::logic()` reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PasteNextOutcome {
    Pasted { seq: u64 },
    Failed { seq: u64, reason: &'static str },
}

/// RAII guard ensuring `PASTE_NEXT_IN_FLIGHT` is always cleared even if simulation unwinds.
struct PasteNextInFlightGuard;

impl PasteNextInFlightGuard {
    fn enter() -> Self {
        set_paste_next_in_flight(true);
        Self
    }
}

impl Drop for PasteNextInFlightGuard {
    fn drop(&mut self) {
        set_paste_next_in_flight(false);
    }
}

trait PasteNextExecutor {
    fn execute_job(&mut self, job: PasteNextJob) -> PasteNextOutcome;
}

struct OsPasteNextExecutor {
    clipboard: Option<arboard::Clipboard>,
}

impl OsPasteNextExecutor {
    const fn new() -> Self {
        Self { clipboard: None }
    }

    fn ensure_clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(cb) => self.clipboard = Some(cb),
                Err(e) => {
                    log_message("WARN", &format!("Paste Next clipboard init failed: {e}"));
                    return None;
                }
            }
        }
        self.clipboard.as_mut()
    }
}

impl PasteNextExecutor for OsPasteNextExecutor {
    fn execute_job(&mut self, job: PasteNextJob) -> PasteNextOutcome {
        const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(500);

        let seq = job.seq;
        // Paste Next never switches window focus, so only a brief clipboard-settle delay is needed.
        let settle_ms = job.settle_ms.clamp(15, 40);
        let release_modifiers = job.release_modifiers;
        let db = job.db;
        let ctx = job.ctx;

        // 1. Wait for physical hotkey modifiers to be released (bounded timeout).
        wait_for_modifiers_released(release_modifiers, MODIFIER_RELEASE_TIMEOUT);

        // 2. Serialize clipboard write + synthetic paste against `focus-paste-worker`.
        let outcome = {
            let _lock = PASTE_SIMULATION_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            // Write payload and arm unified skip marker immediately before OS clipboard write.
            let write_ok = match job.payload {
                PasteNextPayload::Text { text, conceal } => {
                    db.skip_next_capture(crate::db::SkipCaptureTarget::Text(text.clone()));
                    let res = self
                        .ensure_clipboard()
                        .is_some_and(|cb| cb.set_text(text.as_str()).is_ok());
                    if res && conceal {
                        mark_clipboard_concealed();
                    }
                    res
                }
                PasteNextPayload::Image {
                    width,
                    height,
                    rgba,
                } => {
                    let fp = crate::media::image_fingerprint(width, height, &rgba);
                    db.skip_next_capture(crate::db::SkipCaptureTarget::Image(fp));
                    self.ensure_clipboard().is_some_and(|cb| {
                        cb.set_image(arboard::ImageData {
                            width,
                            height,
                            bytes: std::borrow::Cow::Borrowed(&rgba),
                        })
                        .is_ok()
                    })
                }
            };

            if write_ok {
                // 3. Wait settle_ms so the OS pasteboard is updated and readable.
                std::thread::sleep(Duration::from_millis(settle_ms));

                // 4. Simulate paste into the current foreground window with re-entrancy guard.
                let sim_res = {
                    let _guard = PasteNextInFlightGuard::enter();
                    simulate_paste_foreground(release_modifiers)
                };

                match sim_res {
                    Ok(()) => PasteNextOutcome::Pasted { seq },
                    Err(reason) => {
                        db.clear_skip_capture();
                        PasteNextOutcome::Failed { seq, reason }
                    }
                }
            } else {
                // Drop cached clipboard handle so next job retries initialization.
                self.clipboard = None;
                db.clear_skip_capture();
                PasteNextOutcome::Failed {
                    seq,
                    reason: "clipboard write failed",
                }
            }
        };

        ctx.request_repaint();
        outcome
    }
}

fn paste_next_worker_loop<E: PasteNextExecutor>(
    rx: &std::sync::mpsc::Receiver<PasteNextJob>,
    outcome_tx: &crossbeam_channel::Sender<PasteNextOutcome>,
    executor: &mut E,
) {
    while let Ok(job) = rx.recv() {
        let outcome = executor.execute_job(job);
        if outcome_tx.send(outcome).is_err() {
            break;
        }
    }
}

struct PasteNextWorker {
    sender: std::sync::Mutex<Option<std::sync::mpsc::Sender<PasteNextJob>>>,
    outcome_rx: crossbeam_channel::Receiver<PasteNextOutcome>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl PasteNextWorker {
    fn send(&self, job: PasteNextJob) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(job);
        }
    }

    fn shutdown(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(sender);

        let handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            if handle.join().is_err() {
                log_message("ERROR", "paste-next-worker panicked during shutdown");
            }
        }
    }
}

static PASTE_NEXT_WORKER: std::sync::OnceLock<PasteNextWorker> = std::sync::OnceLock::new();

fn paste_next_worker() -> &'static PasteNextWorker {
    PASTE_NEXT_WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<PasteNextJob>();
        let (outcome_tx, outcome_rx) = crossbeam_channel::unbounded::<PasteNextOutcome>();
        let handle = std::thread::Builder::new()
            .name("paste-next-worker".to_string())
            .spawn(move || {
                let mut executor = OsPasteNextExecutor::new();
                paste_next_worker_loop(&rx, &outcome_tx, &mut executor);
            });
        if let Err(e) = &handle {
            log_message("ERROR", &format!("Failed to spawn paste-next-worker: {e}"));
        }
        PasteNextWorker {
            sender: std::sync::Mutex::new(Some(tx)),
            outcome_rx,
            handle: std::sync::Mutex::new(handle.ok()),
        }
    })
}

/// Submits a Paste Next job to the ordered `paste-next-worker` without blocking the caller.
pub(crate) fn submit_paste_next(job: PasteNextJob) {
    paste_next_worker().send(job);
}

/// Non-blocking poll for completed Paste Next outcomes (returns `None` if worker not started or empty).
#[must_use]
pub(crate) fn try_recv_paste_next_outcome() -> Option<PasteNextOutcome> {
    PASTE_NEXT_WORKER
        .get()
        .and_then(|worker| worker.outcome_rx.try_recv().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_capture_and_restore_no_panic() {
        // Calling capture and restore should never panic on any platform
        capture_active_window();
        restore_focus_and_paste(false, false, 120);
        let _ = is_app_active();
        let _ = is_clipboard_concealed();
        let _ = take_reopen_requested();
        hide_application();
        order_front_main_window();
        setup_reopen_observer();
    }

    #[test]
    fn test_concealed_marker_support_matches_ui_limitation() {
        let support = concealed_marker_support();
        let limitation = concealed_marker_limitation();

        assert_eq!(
            support == ConcealedMarkerSupport::DetectOnly,
            limitation.is_some()
        );

        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            assert_eq!(support, ConcealedMarkerSupport::DetectAndEmit);
            assert!(limitation.is_none());
        } else {
            assert_eq!(support, ConcealedMarkerSupport::DetectOnly);
            assert!(limitation.is_some_and(|text| text.contains("cannot advertise")));
        }
    }

    #[test]
    fn test_windows_concealed_marker_decision_helper() {
        assert!(!windows_concealed_detected(false, false, false));
        assert!(windows_concealed_detected(true, false, false));
        assert!(windows_concealed_detected(false, true, false));
        assert!(windows_concealed_detected(false, false, true));
        assert!(windows_concealed_detected(true, true, true));
    }

    #[test]
    fn test_windows_concealed_marker_format_names() {
        assert_eq!(
            WINDOWS_CONCEALED_DETECT_FORMATS,
            [
                "ExcludeClipboardContentFromMonitorProcessing",
                "Clipboard Viewer Ignore",
                "CanIncludeInClipboardHistory",
            ]
        );
        assert_eq!(
            WINDOWS_CONCEALED_EMIT_FORMATS,
            [
                "ExcludeClipboardContentFromMonitorProcessing",
                "CanIncludeInClipboardHistory",
                "CanUploadClipboardContentToCloud",
            ]
        );
    }

    #[test]
    fn test_paste_next_worker_executes_ten_rapid_jobs_in_order() {
        struct MockExecutor {
            recorded: Vec<(u64, String)>,
        }
        impl PasteNextExecutor for MockExecutor {
            fn execute_job(&mut self, job: PasteNextJob) -> PasteNextOutcome {
                let text = match job.payload {
                    PasteNextPayload::Text { text, .. } => text.to_string(),
                    PasteNextPayload::Image { .. } => "image".to_string(),
                };
                self.recorded.push((job.seq, text));
                PasteNextOutcome::Pasted { seq: job.seq }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let db =
            crate::db::Database::open(&dir.path().join("mock_pn.db"), &"11".repeat(32)).unwrap();
        let ctx = eframe::egui::Context::default();

        let (tx, rx) = std::sync::mpsc::channel::<PasteNextJob>();
        let (outcome_tx, outcome_rx) = crossbeam_channel::unbounded::<PasteNextOutcome>();

        for seq in 1..=10_u64 {
            tx.send(PasteNextJob {
                seq,
                payload: PasteNextPayload::Text {
                    text: zeroize::Zeroizing::new(format!("item-{seq}")),
                    conceal: false,
                },
                settle_ms: 20,
                release_modifiers: global_hotkey::hotkey::Modifiers::empty(),
                db: db.clone(),
                ctx: ctx.clone(),
            })
            .unwrap();
        }
        drop(tx);

        let mut mock = MockExecutor {
            recorded: Vec::new(),
        };
        paste_next_worker_loop(&rx, &outcome_tx, &mut mock);

        let expected_recorded: Vec<(u64, String)> =
            (1..=10_u64).map(|i| (i, format!("item-{i}"))).collect();
        assert_eq!(mock.recorded, expected_recorded);

        let outcomes: Vec<PasteNextOutcome> = outcome_rx.try_iter().collect();
        let expected_outcomes: Vec<PasteNextOutcome> = (1..=10_u64)
            .map(|seq| PasteNextOutcome::Pasted { seq })
            .collect();
        assert_eq!(outcomes, expected_outcomes);
    }

    #[test]
    fn test_uipi_should_skip_paste_fails_closed() {
        assert!(!uipi_should_skip_paste(0, None, 0x2000));
        assert!(uipi_should_skip_paste(42, None, 0x2000));
        assert!(!uipi_should_skip_paste(42, Some(0x2000), 0x2000));
        assert!(uipi_should_skip_paste(42, Some(0x3000), 0x2000));
        assert!(!uipi_should_skip_paste(42, Some(0x1000), 0x2000));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_mark_clipboard_concealed_runs_explicit_noop() {
        super::linux::reset_mark_called();
        mark_clipboard_concealed();
        assert!(super::linux::mark_called());
    }

    #[test]
    fn test_paste_worker_shutdown_joins_and_is_idempotent() {
        let (tx, rx) = std::sync::mpsc::channel::<PasteRequest>();
        let handle = std::thread::Builder::new()
            .name("test-focus-paste-worker".to_string())
            .spawn(move || paste_worker_loop(&rx))
            .unwrap();
        let worker = PasteWorker {
            sender: std::sync::Mutex::new(Some(tx)),
            handle: std::sync::Mutex::new(Some(handle)),
        };

        worker.send(PasteRequest {
            restore_focus: false,
            auto_paste: false,
            auto_paste_delay_ms: 0,
        });
        worker.shutdown();
        worker.shutdown();

        assert!(worker
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none());
        assert!(worker
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none());
    }

    #[test]
    fn test_image_ocr_helpers_do_not_panic() {
        let _ = supports_image_ocr();
        let res = extract_text_from_png(&[]);
        assert!(res.is_err());
    }

    #[test]
    fn test_should_minimize_on_hide_decision() {
        use super::should_minimize_on_hide;

        // Tray icon live, minimize setting false -> hide to tray (false)
        assert!(!should_minimize_on_hide(true, false));

        // Tray icon live, minimize setting true -> minimize to taskbar (true)
        assert!(should_minimize_on_hide(true, true));

        // No live tray, minimize setting false -> fallback minimize to taskbar (true)
        assert!(should_minimize_on_hide(false, false));

        // No live tray, minimize setting true -> minimize to taskbar (true)
        assert!(should_minimize_on_hide(false, true));
    }

    #[test]
    fn test_configure_wgpu_options_defaults() {
        let backends = super::default_wgpu_backends();
        #[cfg(target_os = "windows")]
        assert_eq!(backends, eframe::wgpu::Backends::DX12);
        #[cfg(not(target_os = "windows"))]
        assert!(backends.contains(eframe::wgpu::Backends::PRIMARY));

        let config = super::configure_wgpu_options();
        match config.wgpu_setup {
            eframe::egui_wgpu::WgpuSetup::CreateNew(create_new) => {
                if std::env::var_os("WGPU_POWER_PREF").is_none() {
                    assert_eq!(
                        create_new.power_preference,
                        eframe::wgpu::PowerPreference::LowPower
                    );
                }
                if std::env::var_os("WGPU_BACKEND").is_none() {
                    assert_eq!(create_new.instance_descriptor.backends, backends);
                }
            }
            eframe::egui_wgpu::WgpuSetup::Existing(_) => {
                panic!("Expected WgpuSetup::CreateNew from configure_wgpu_options");
            }
        }
    }
}
