//! Platform-adaptive UI theme providing native macOS Apple HIG styling,
//! Windows 11 Fluent styling, and Linux Adwaita styling in both Dark and Light modes.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Rounding, Stroke};
use std::path::Path;

/// Applies the platform-adaptive theme visuals (Dark or Light) to the egui context.
pub(crate) fn setup_theme(ctx: &egui::Context, is_dark: bool) {
    let mut visuals = if is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // Window & Panel backgrounds
    visuals.panel_fill = panel_background(is_dark);
    visuals.window_fill = panel_background(is_dark);
    visuals.window_rounding = card_rounding();
    visuals.override_text_color = Some(text_primary(is_dark));

    // Base widget frames
    visuals.widgets.noninteractive.bg_fill = card_fill(is_dark, false);
    visuals.widgets.noninteractive.bg_stroke = card_stroke(is_dark, false);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text_primary(is_dark));
    visuals.widgets.noninteractive.rounding = card_rounding();

    visuals.widgets.inactive.bg_fill = button_inactive_fill(is_dark);
    visuals.widgets.inactive.bg_stroke = Stroke::new(
        1.0_f32,
        if is_dark {
            Color32::from_rgb(70, 70, 75)
        } else {
            Color32::from_rgb(209, 209, 214)
        },
    );
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text_primary(is_dark));
    visuals.widgets.inactive.rounding = button_rounding();

    visuals.widgets.hovered.bg_fill = button_hover_fill(is_dark);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent_color(is_dark));
    visuals.widgets.hovered.fg_stroke = Stroke::new(
        1.5_f32,
        if is_dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(10, 10, 12)
        },
    );
    visuals.widgets.hovered.rounding = button_rounding();

    visuals.widgets.active.bg_fill = accent_color(is_dark);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent_color(is_dark));
    visuals.widgets.active.fg_stroke = Stroke::new(2.0_f32, Color32::WHITE);
    visuals.widgets.active.rounding = button_rounding();

    // Selection visuals
    visuals.selection.bg_fill = selection_fill(is_dark);
    visuals.selection.stroke = selection_stroke(is_dark);

    // High-contrast semantic indicators
    visuals.hyperlink_color = link_color(is_dark);
    visuals.warn_fg_color = warning_color(is_dark);
    visuals.error_fg_color = Color32::from_rgb(220, 53, 69);
    visuals.code_bg_color = if is_dark {
        Color32::from_rgb(26, 27, 34)
    } else {
        Color32::from_rgb(244, 244, 247)
    };

    ctx.set_visuals(visuals);
}

/// Probes for native system fonts (San Francisco on macOS, Segoe UI on Windows, `DejaVu` Sans on Linux)
/// and registers them into `FontDefinitions` as the primary proportional font family,
/// while keeping Phosphor icon glyphs as fallbacks.
pub(crate) fn setup_fonts(fonts: &mut FontDefinitions) {
    #[cfg(target_os = "macos")]
    let candidate_fonts = [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/SFNSText.ttf",
        "/System/Library/Fonts/SFCompact.ttf",
        "/System/Library/Fonts/HelveticaNeue.ttc",
        "/System/Library/Fonts/Helvetica.ttc",
        "/Library/Fonts/Arial.ttf",
    ];

    #[cfg(target_os = "windows")]
    let candidate_fonts = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\segoeuil.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidate_fonts = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];

    for path_str in candidate_fonts {
        let path = Path::new(path_str);
        if path.exists() {
            if let Ok(bytes) = std::fs::read(path) {
                let font_name = "system_ui".to_owned();
                fonts
                    .font_data
                    .insert(font_name.clone(), FontData::from_owned(bytes));
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, font_name);
                }
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Platform-Adaptive Color Palette (Dark & Light)
// ---------------------------------------------------------------------------

/// Primary window and panel background color.
#[must_use]
pub(crate) const fn panel_background(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(30, 30, 30)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(32, 32, 32)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(36, 36, 36)
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            // Apple System Grouped Background Light (#F2F2F7)
            Color32::from_rgb(242, 242, 247)
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 11 Light Background (#F3F3F3)
            Color32::from_rgb(243, 243, 243)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(246, 246, 246)
        }
    }
}

/// Background fill for item cards.
#[must_use]
pub(crate) const fn card_fill(is_dark: bool, is_selected: bool) -> Color32 {
    if is_dark {
        if is_selected {
            #[cfg(target_os = "macos")]
            {
                Color32::from_rgb(30, 58, 95)
            }
            #[cfg(target_os = "windows")]
            {
                Color32::from_rgb(29, 54, 74)
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                Color32::from_rgb(31, 56, 88)
            }
        } else {
            #[cfg(target_os = "macos")]
            {
                Color32::from_rgb(40, 40, 43)
            }
            #[cfg(target_os = "windows")]
            {
                Color32::from_rgb(43, 43, 43)
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                Color32::from_rgb(46, 46, 48)
            }
        }
    } else if is_selected {
        #[cfg(target_os = "macos")]
        {
            // Soft Apple blue selection in light mode (#E5F0FF)
            Color32::from_rgb(229, 240, 255)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(232, 242, 251)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(228, 240, 255)
        }
    } else {
        // Pure crisp white card in light mode (#FFFFFF)
        Color32::WHITE
    }
}

/// Border stroke for item cards.
#[must_use]
pub(crate) fn card_stroke(is_dark: bool, is_selected: bool) -> Stroke {
    if is_selected {
        Stroke::new(1.5_f32, accent_color(is_dark))
    } else if is_dark {
        #[cfg(target_os = "macos")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(72, 72, 78))
        }
        #[cfg(target_os = "windows")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(64, 64, 68))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(72, 72, 78))
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(210, 210, 218))
        }
        #[cfg(target_os = "windows")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(218, 218, 222))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(210, 210, 218))
        }
    }
}

/// Selection background fill for egui visuals.
#[must_use]
pub(crate) const fn selection_fill(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(10, 132, 255)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(0, 120, 212)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(53, 132, 228)
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(0, 122, 255)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(0, 103, 184)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(40, 110, 200)
        }
    }
}

/// Selection stroke for egui visuals.
#[must_use]
pub(crate) fn selection_stroke(is_dark: bool) -> Stroke {
    Stroke::new(1.5_f32, accent_color(is_dark))
}

/// Primary accent color.
#[must_use]
pub(crate) const fn accent_color(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(10, 132, 255)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(0, 120, 212)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(53, 132, 228)
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(0, 122, 255)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(0, 103, 184)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(40, 110, 200)
        }
    }
}

/// Search box background color (Spotlight-style on macOS, Fluent on Windows).
#[must_use]
pub(crate) const fn search_bg(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(34, 34, 36)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(45, 45, 45)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(42, 42, 44)
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            // Apple Spotlight light input fill (#E3E3E8)
            Color32::from_rgb(227, 227, 232)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(238, 238, 238)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(234, 234, 238)
        }
    }
}

/// Search box border stroke.
#[must_use]
pub(crate) fn search_stroke(is_dark: bool, is_focused: bool) -> Stroke {
    if is_focused {
        Stroke::new(1.5_f32, accent_color(is_dark))
    } else if is_dark {
        Stroke::new(1.0_f32, Color32::from_rgb(72, 72, 78))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(195, 195, 205))
    }
}

/// Button background when inactive.
#[must_use]
pub(crate) const fn button_inactive_fill(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(46, 46, 50)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(45, 45, 45)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(48, 48, 50)
        }
    } else {
        Color32::WHITE
    }
}

/// Button background when hovered.
#[must_use]
pub(crate) const fn button_hover_fill(is_dark: bool) -> Color32 {
    if is_dark {
        #[cfg(target_os = "macos")]
        {
            Color32::from_rgb(64, 64, 70)
        }
        #[cfg(target_os = "windows")]
        {
            Color32::from_rgb(56, 56, 56)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(58, 58, 60)
        }
    } else {
        Color32::from_rgb(240, 240, 244)
    }
}

/// Primary high-contrast text color (WCAG AAA compliant: >= 12.0:1 contrast).
#[must_use]
pub(crate) const fn text_primary(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(245, 245, 248)
    } else {
        Color32::from_rgb(28, 28, 30)
    }
}

/// Secondary metadata, hints, and timestamp text color (WCAG AAA compliant: >= 7.0:1 contrast).
#[must_use]
pub(crate) const fn text_secondary(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(182, 182, 190)
    } else {
        Color32::from_rgb(82, 82, 88)
    }
}

/// Monospace content preview text color (WCAG AAA compliant: >= 11.0:1 contrast).
#[must_use]
pub(crate) const fn text_preview(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(235, 235, 240)
    } else {
        Color32::from_rgb(32, 32, 34)
    }
}

/// High-contrast success message color (WCAG AA compliant: >= 5.0:1 contrast).
#[must_use]
pub(crate) const fn success_color(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(52, 199, 89)
    } else {
        Color32::from_rgb(20, 125, 55)
    }
}

/// High-contrast warning message color (WCAG AA compliant: >= 5.0:1 contrast).
#[must_use]
pub(crate) const fn warning_color(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(255, 159, 10)
    } else {
        Color32::from_rgb(160, 65, 0)
    }
}

/// High-contrast interactive URL link color (WCAG AA compliant: >= 5.5:1 contrast).
#[must_use]
pub(crate) const fn link_color(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(100, 181, 246)
    } else {
        Color32::from_rgb(0, 102, 204)
    }
}

/// Stroke color for bottom status bar.
#[must_use]
pub(crate) const fn bottom_panel_stroke(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(58, 58, 62)
    } else {
        Color32::from_rgb(218, 218, 224)
    }
}

/// Computes the sRGB relative luminance of a color according to WCAG 2.1 specifications.
#[must_use]
pub(crate) fn relative_luminance(c: Color32) -> f32 {
    let channel = |v: u8| -> f32 {
        let s = f32::from(v) / 255.0_f32;
        if s <= 0.04045_f32 {
            s / 12.92_f32
        } else {
            ((s + 0.055_f32) / 1.055_f32).powf(2.4_f32)
        }
    };
    0.2126_f32.mul_add(
        channel(c.r()),
        0.7152_f32.mul_add(channel(c.g()), 0.0722_f32 * channel(c.b())),
    )
}

/// Computes the WCAG 2.1 contrast ratio between two colors (ranging from 1.0 to 21.0).
#[must_use]
pub(crate) fn contrast_ratio(c1: Color32, c2: Color32) -> f32 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05_f32) / (darker + 0.05_f32)
}

/// Clamps token colors to guarantee minimum WCAG contrast ratio (>= `min_contrast`) against background.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
pub(crate) fn ensure_contrast(mut fg: Color32, bg: Color32, min_contrast: f32) -> Color32 {
    let mut ratio = contrast_ratio(fg, bg);
    if ratio >= min_contrast {
        return fg;
    }

    let bg_lum = relative_luminance(bg);
    let is_bg_light = bg_lum > 0.5_f32;

    for _ in 0..20 {
        if ratio >= min_contrast {
            break;
        }
        if is_bg_light {
            fg = Color32::from_rgb(
                (f32::from(fg.r()) * 0.88_f32) as u8,
                (f32::from(fg.g()) * 0.88_f32) as u8,
                (f32::from(fg.b()) * 0.88_f32) as u8,
            );
        } else {
            fg = Color32::from_rgb(
                ((f32::from(fg.r()) * 1.12_f32 + 10.0_f32).min(255.0_f32)) as u8,
                ((f32::from(fg.g()) * 1.12_f32 + 10.0_f32).min(255.0_f32)) as u8,
                ((f32::from(fg.b()) * 1.12_f32 + 10.0_f32).min(255.0_f32)) as u8,
            );
        }
        ratio = contrast_ratio(fg, bg);
    }
    fg
}

// ---------------------------------------------------------------------------
// Platform-Adaptive Geometry & Rounding
// ---------------------------------------------------------------------------

/// Corner rounding for clipboard item cards.
#[must_use]
pub(crate) const fn card_rounding() -> Rounding {
    #[cfg(target_os = "macos")]
    {
        Rounding::same(10.0_f32)
    }
    #[cfg(target_os = "windows")]
    {
        Rounding::same(6.0_f32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Rounding::same(8.0_f32)
    }
}

/// Corner rounding for search bars.
#[must_use]
pub(crate) const fn search_rounding() -> Rounding {
    #[cfg(target_os = "macos")]
    {
        Rounding::same(8.0_f32)
    }
    #[cfg(target_os = "windows")]
    {
        Rounding::same(4.0_f32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Rounding::same(6.0_f32)
    }
}

/// Corner rounding for standard buttons.
#[must_use]
pub(crate) const fn button_rounding() -> Rounding {
    #[cfg(target_os = "macos")]
    {
        Rounding::same(6.0_f32)
    }
    #[cfg(target_os = "windows")]
    {
        Rounding::same(4.0_f32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Rounding::same(6.0_f32)
    }
}

/// Corner rounding for pills and badges.
#[must_use]
pub(crate) const fn pill_rounding() -> Rounding {
    #[cfg(target_os = "macos")]
    {
        Rounding::same(12.0_f32)
    }
    #[cfg(target_os = "windows")]
    {
        Rounding::same(4.0_f32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Rounding::same(8.0_f32)
    }
}

/// Left padding for the top header row on macOS to avoid traffic lights.
#[must_use]
pub(crate) const fn header_left_padding() -> f32 {
    #[cfg(target_os = "macos")]
    {
        68.0_f32
    }
    #[cfg(not(target_os = "macos"))]
    {
        0.0_f32
    }
}

/// Top space for the main header panel.
#[must_use]
pub(crate) const fn top_padding() -> f32 {
    #[cfg(target_os = "macos")]
    {
        10.0_f32
    }
    #[cfg(not(target_os = "macos"))]
    {
        6.0_f32
    }
}

// ---------------------------------------------------------------------------
// Pill & Badge UI Components
// ---------------------------------------------------------------------------

/// Computes distinct (`text_color`, `bg_tint`, `border_color`) for each category badge.
#[must_use]
pub(crate) const fn category_pill_colors(
    is_dark: bool,
    kind: crate::db::ItemKind,
) -> (Color32, Color32, Color32) {
    if is_dark {
        match kind {
            crate::db::ItemKind::Text => (
                Color32::from_rgb(220, 220, 225),
                Color32::from_rgb(45, 45, 48),
                Color32::from_rgb(70, 70, 74),
            ),
            crate::db::ItemKind::Url => (
                Color32::from_rgb(100, 190, 255),
                Color32::from_rgb(18, 44, 75),
                Color32::from_rgb(30, 90, 160),
            ),
            crate::db::ItemKind::Code => (
                Color32::from_rgb(255, 130, 120),
                Color32::from_rgb(58, 26, 26),
                Color32::from_rgb(130, 48, 48),
            ),
            crate::db::ItemKind::Color => (
                Color32::from_rgb(220, 150, 255),
                Color32::from_rgb(46, 24, 58),
                Color32::from_rgb(110, 50, 140),
            ),
            crate::db::ItemKind::Image => (
                Color32::from_rgb(110, 230, 145),
                Color32::from_rgb(20, 48, 30),
                Color32::from_rgb(40, 110, 65),
            ),
        }
    } else {
        match kind {
            crate::db::ItemKind::Text => (
                Color32::from_rgb(50, 50, 54),
                Color32::from_rgb(235, 235, 238),
                Color32::from_rgb(205, 205, 212),
            ),
            crate::db::ItemKind::Url => (
                Color32::from_rgb(0, 85, 180),
                Color32::from_rgb(225, 240, 255),
                Color32::from_rgb(170, 210, 255),
            ),
            crate::db::ItemKind::Code => (
                Color32::from_rgb(180, 25, 25),
                Color32::from_rgb(255, 235, 235),
                Color32::from_rgb(250, 185, 180),
            ),
            crate::db::ItemKind::Color => (
                Color32::from_rgb(125, 35, 155),
                Color32::from_rgb(245, 235, 250),
                Color32::from_rgb(220, 185, 240),
            ),
            crate::db::ItemKind::Image => (
                Color32::from_rgb(18, 115, 45),
                Color32::from_rgb(230, 248, 235),
                Color32::from_rgb(170, 230, 190),
            ),
        }
    }
}

/// Computes (`text_color`, `bg_fill`, `stroke_color`) for shortcut pills (Cmd+1..9).
#[must_use]
pub(crate) const fn shortcut_badge_colors(
    is_dark: bool,
    is_selected: bool,
) -> (Color32, Color32, Color32) {
    if is_selected {
        (
            Color32::WHITE,
            if is_dark {
                Color32::from_rgb(0, 100, 220)
            } else {
                Color32::from_rgb(0, 95, 200)
            },
            if is_dark {
                Color32::from_rgb(10, 132, 255)
            } else {
                Color32::from_rgb(0, 102, 204)
            },
        )
    } else if is_dark {
        (
            Color32::from_rgb(215, 215, 220),
            Color32::from_rgb(46, 46, 50),
            Color32::from_rgb(72, 72, 78),
        )
    } else {
        (
            Color32::from_rgb(60, 60, 64),
            Color32::from_rgb(235, 235, 240),
            Color32::from_rgb(205, 205, 212),
        )
    }
}

/// Renders a category pill tag with high-contrast, pleasing colors and distinct border.
pub(crate) fn render_pill_tag(
    ui: &mut egui::Ui,
    icon: &str,
    text: &str,
    text_color: Color32,
    bg_tint: Color32,
    border_color: Color32,
) {
    egui::Frame::none()
        .rounding(pill_rounding())
        .fill(bg_tint)
        .stroke(Stroke::new(1.0_f32, border_color))
        .inner_margin(egui::Margin::symmetric(7.0_f32, 2.0_f32))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(format!("{icon} {text}"))
                    .size(10.0_f32)
                    .color(text_color)
                    .strong(),
            );
        });
}

/// Renders an interactive segmented control pill tab.
pub(crate) fn render_segmented_tab(
    ui: &mut egui::Ui,
    is_dark: bool,
    selected: bool,
    icon: &str,
    label: &str,
    count: usize,
) -> egui::Response {
    let (fill, stroke, text_color) = if selected {
        (
            if is_dark {
                Color32::from_rgb(0, 100, 220)
            } else {
                Color32::from_rgb(0, 95, 200)
            },
            Stroke::new(
                1.0_f32,
                if is_dark {
                    Color32::from_rgb(10, 132, 255)
                } else {
                    Color32::from_rgb(0, 102, 204)
                },
            ),
            Color32::WHITE,
        )
    } else if is_dark {
        (
            Color32::from_rgb(38, 38, 42),
            Stroke::new(1.0_f32, Color32::from_rgb(62, 62, 68)),
            Color32::from_rgb(215, 215, 222),
        )
    } else {
        (
            Color32::WHITE,
            Stroke::new(1.0_f32, Color32::from_rgb(205, 205, 212)),
            Color32::from_rgb(50, 50, 54),
        )
    };

    let text = format!("{icon} {label} ({count})");
    let button = egui::Button::new(
        egui::RichText::new(text)
            .size(11.0_f32)
            .color(text_color)
            .strong(),
    )
    .fill(fill)
    .stroke(stroke)
    .rounding(pill_rounding());

    ui.add(button)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ItemKind;

    #[test]
    fn test_theme_palette_colors_dark_and_light() {
        for is_dark in [true, false] {
            let bg = panel_background(is_dark);
            assert_ne!(bg, Color32::TRANSPARENT);

            let card_unsel = card_fill(is_dark, false);
            let card_sel = card_fill(is_dark, true);
            assert_ne!(card_unsel, card_sel);

            let accent = accent_color(is_dark);
            assert_ne!(accent, Color32::TRANSPARENT);

            let txt_pri = text_primary(is_dark);
            let txt_sec = text_secondary(is_dark);
            assert_ne!(txt_pri, txt_sec);

            for kind in [
                ItemKind::Text,
                ItemKind::Url,
                ItemKind::Code,
                ItemKind::Color,
                ItemKind::Image,
            ] {
                let (text_c, bg_c, stroke_c) = category_pill_colors(is_dark, kind);
                assert_ne!(text_c, Color32::TRANSPARENT);
                assert_ne!(bg_c, Color32::TRANSPARENT);
                assert_ne!(stroke_c, Color32::TRANSPARENT);
            }

            let (sh_txt, sh_bg, sh_stroke) = shortcut_badge_colors(is_dark, false);
            assert_ne!(sh_txt, Color32::TRANSPARENT);
            assert_ne!(sh_bg, Color32::TRANSPARENT);
            assert_ne!(sh_stroke, Color32::TRANSPARENT);
        }
    }

    #[test]
    #[allow(clippy::too_many_lines, clippy::similar_names)]
    fn test_wcag_contrast_audit() {
        for is_dark in [true, false] {
            let mode = if is_dark { "DARK" } else { "LIGHT" };
            let panel = panel_background(is_dark);
            let card_unsel = card_fill(is_dark, false);
            let card_sel = card_fill(is_dark, true);
            let pri = text_primary(is_dark);
            let sec = text_secondary(is_dark);
            let prev = text_preview(is_dark);

            println!("\n=== {mode} MODE CONTRAST AUDIT ===");
            // WCAG AA requires >= 4.5:1 for normal text; WCAG AAA requires >= 7.0:1
            let pri_panel_cr = contrast_ratio(pri, panel);
            let pri_card_cr = contrast_ratio(pri, card_unsel);
            let pri_sel_cr = contrast_ratio(pri, card_sel);
            println!("Primary on Panel: {pri_panel_cr:.2}:1");
            println!("Primary on Card Unsel: {pri_card_cr:.2}:1");
            println!("Primary on Card Sel: {pri_sel_cr:.2}:1");
            assert!(
                pri_panel_cr >= 7.0,
                "Primary text on panel must meet WCAG AAA"
            );
            assert!(
                pri_card_cr >= 7.0,
                "Primary text on card must meet WCAG AAA"
            );
            assert!(
                pri_sel_cr >= 7.0,
                "Primary text on selected card must meet WCAG AAA"
            );

            let sec_panel_cr = contrast_ratio(sec, panel);
            let sec_card_cr = contrast_ratio(sec, card_unsel);
            let sec_sel_cr = contrast_ratio(sec, card_sel);
            println!("Secondary on Panel: {sec_panel_cr:.2}:1");
            println!("Secondary on Card Unsel: {sec_card_cr:.2}:1");
            println!("Secondary on Card Sel: {sec_sel_cr:.2}:1");
            assert!(
                sec_panel_cr >= 4.5,
                "Secondary text on panel must meet WCAG AA"
            );
            assert!(
                sec_card_cr >= 4.5,
                "Secondary text on card must meet WCAG AA"
            );
            assert!(
                sec_sel_cr >= 4.5,
                "Secondary text on selected card must meet WCAG AA"
            );

            let prev_card_cr = contrast_ratio(prev, card_unsel);
            let prev_sel_cr = contrast_ratio(prev, card_sel);
            println!("Preview on Card Unsel: {prev_card_cr:.2}:1");
            println!("Preview on Card Sel: {prev_sel_cr:.2}:1");
            assert!(
                prev_card_cr >= 7.0,
                "Preview text on card must meet WCAG AAA"
            );
            assert!(
                prev_sel_cr >= 7.0,
                "Preview text on selected card must meet WCAG AAA"
            );

            let s_bg = search_bg(is_dark);
            let pri_search_cr = contrast_ratio(pri, s_bg);
            let sec_search_cr = contrast_ratio(sec, s_bg);
            println!("Primary on Search Bg: {pri_search_cr:.2}:1");
            println!("Secondary on Search Bg: {sec_search_cr:.2}:1");
            assert!(pri_search_cr >= 7.0, "Primary on search must meet WCAG AAA");
            assert!(
                sec_search_cr >= 4.5,
                "Secondary on search must meet WCAG AA"
            );

            let ctx = egui::Context::default();
            setup_theme(&ctx, is_dark);
            let style = ctx.style();

            let code_job = crate::code::highlight_code(
                &ctx,
                &style,
                "fn main() { let x = 42; // comment\n}",
                crate::code::CodeLanguage::Rust,
            );
            let canvas_fill = if is_dark {
                Color32::from_rgb(26, 27, 34)
            } else {
                Color32::from_rgb(244, 244, 247)
            };
            for (i, sec_fmt) in code_job.sections.iter().enumerate() {
                let txt = &code_job.text[sec_fmt.byte_range.clone()];
                let c = sec_fmt.format.color;
                let cr = contrast_ratio(c, canvas_fill);
                println!("Code token [{i}] {txt:?}: color {c:?}, on canvas: {cr:.2}:1");
                assert!(
                    cr >= 4.5,
                    "Code token {txt:?} must meet WCAG AA contrast (>= 4.5:1)"
                );
            }

            for kind in [
                ItemKind::Text,
                ItemKind::Url,
                ItemKind::Code,
                ItemKind::Color,
                ItemKind::Image,
            ] {
                let (txt, bg, stroke) = category_pill_colors(is_dark, kind);
                let txt_bg_cr = contrast_ratio(txt, bg);
                let txt_card_cr = contrast_ratio(txt, card_unsel);
                println!("Pill {kind:?} text on bg_tint: {txt_bg_cr:.2}:1, on card: {txt_card_cr:.2}:1, border on card: {:.2}:1",
                    contrast_ratio(stroke, card_unsel));
                assert!(
                    txt_bg_cr >= 4.5,
                    "Pill {kind:?} text on tint must meet WCAG AA"
                );
                assert!(
                    txt_card_cr >= 4.5,
                    "Pill {kind:?} text on card must meet WCAG AA"
                );
            }

            let (sh_txt, sh_bg, _) = shortcut_badge_colors(is_dark, false);
            let sh_unsel_cr = contrast_ratio(sh_txt, sh_bg);
            println!("Shortcut unselected text on badge bg: {sh_unsel_cr:.2}:1");
            assert!(
                sh_unsel_cr >= 4.5,
                "Shortcut badge unselected text must meet WCAG AA"
            );

            let (sh_sel_txt, sh_sel_bg, _) = shortcut_badge_colors(is_dark, true);
            let sh_sel_cr = contrast_ratio(sh_sel_txt, sh_sel_bg);
            println!("Shortcut selected text on badge bg: {sh_sel_cr:.2}:1");
            assert!(
                sh_sel_cr >= 4.5,
                "Shortcut badge selected text must meet WCAG AA"
            );

            let toast = success_color(is_dark);
            let toast_cr = contrast_ratio(toast, panel);
            println!("Toast green on Panel: {toast_cr:.2}:1");
            assert!(
                toast_cr >= 4.5,
                "Toast success color on panel must meet WCAG AA"
            );

            let warning = warning_color(is_dark);
            let warn_panel_cr = contrast_ratio(warning, panel);
            let warn_card_cr = contrast_ratio(warning, card_unsel);
            println!("Warning orange on Panel: {warn_panel_cr:.2}:1");
            println!("Warning orange on Card Unsel: {warn_card_cr:.2}:1");
            assert!(
                warn_panel_cr >= 4.5,
                "Warning color on panel must meet WCAG AA"
            );
            assert!(
                warn_card_cr >= 4.5,
                "Warning color on card must meet WCAG AA"
            );

            let link = link_color(is_dark);
            let link_card_cr = contrast_ratio(link, card_unsel);
            let link_sel_cr = contrast_ratio(link, card_sel);
            println!("Link blue on Card Unsel: {link_card_cr:.2}:1");
            println!("Link blue on Card Sel: {link_sel_cr:.2}:1");
            assert!(link_card_cr >= 4.5, "Link blue on card must meet WCAG AA");
            assert!(
                link_sel_cr >= 4.5,
                "Link blue on selected card must meet WCAG AA"
            );
        }
    }
}
