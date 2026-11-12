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
            Stroke::new(1.0_f32, Color32::from_rgb(68, 68, 72))
        }
        #[cfg(target_os = "windows")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(60, 60, 60))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(68, 68, 72))
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(218, 218, 224))
        }
        #[cfg(target_os = "windows")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(225, 225, 225))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(218, 218, 224))
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
        Stroke::new(1.0_f32, Color32::from_rgb(68, 68, 72))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(205, 205, 212))
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

/// Primary high-contrast text color.
#[must_use]
pub(crate) const fn text_primary(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(245, 245, 248)
    } else {
        Color32::from_rgb(28, 28, 30)
    }
}

/// Secondary metadata and timestamp text color.
#[must_use]
pub(crate) const fn text_secondary(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(175, 175, 182)
    } else {
        Color32::from_rgb(105, 105, 112)
    }
}

/// Monospace content preview text color.
#[must_use]
pub(crate) const fn text_preview(is_dark: bool) -> Color32 {
    if is_dark {
        Color32::from_rgb(235, 235, 240)
    } else {
        Color32::from_rgb(32, 32, 34)
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
                Color32::from_rgb(60, 60, 64),
                Color32::from_rgb(235, 235, 238),
                Color32::from_rgb(210, 210, 215),
            ),
            crate::db::ItemKind::Url => (
                Color32::from_rgb(0, 102, 204),
                Color32::from_rgb(225, 240, 255),
                Color32::from_rgb(180, 215, 255),
            ),
            crate::db::ItemKind::Code => (
                Color32::from_rgb(190, 30, 30),
                Color32::from_rgb(255, 235, 235),
                Color32::from_rgb(255, 195, 190),
            ),
            crate::db::ItemKind::Color => (
                Color32::from_rgb(130, 40, 160),
                Color32::from_rgb(245, 235, 250),
                Color32::from_rgb(225, 195, 245),
            ),
            crate::db::ItemKind::Image => (
                Color32::from_rgb(25, 125, 50),
                Color32::from_rgb(230, 248, 235),
                Color32::from_rgb(180, 235, 195),
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
        (Color32::WHITE, accent_color(is_dark), accent_color(is_dark))
    } else if is_dark {
        (
            Color32::from_rgb(210, 210, 215),
            Color32::from_rgb(46, 46, 50),
            Color32::from_rgb(70, 70, 75),
        )
    } else {
        (
            Color32::from_rgb(72, 72, 76),
            Color32::from_rgb(235, 235, 240),
            Color32::from_rgb(210, 210, 216),
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
            accent_color(is_dark),
            Stroke::new(1.0_f32, accent_color(is_dark)),
            Color32::WHITE,
        )
    } else if is_dark {
        (
            Color32::from_rgb(38, 38, 42),
            Stroke::new(1.0_f32, Color32::from_rgb(58, 58, 62)),
            Color32::from_rgb(205, 205, 212),
        )
    } else {
        (
            Color32::WHITE,
            Stroke::new(1.0_f32, Color32::from_rgb(209, 209, 214)),
            Color32::from_rgb(60, 60, 64),
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
}
