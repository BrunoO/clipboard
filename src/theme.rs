//! Platform-adaptive UI theme providing native macOS Apple HIG styling,
//! Windows 11 Fluent dark styling, and Linux Adwaita styling.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Rounding, Stroke};
use std::path::Path;

/// Applies the platform-adaptive theme visuals to the egui context.
pub(crate) fn setup_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // Window & Panel backgrounds
    visuals.panel_fill = panel_background();
    visuals.window_fill = panel_background();
    visuals.window_rounding = card_rounding();
    visuals.override_text_color = Some(Color32::from_rgb(238, 238, 240));

    // Base widget frames
    visuals.widgets.noninteractive.bg_fill = card_fill(false);
    visuals.widgets.noninteractive.bg_stroke = card_stroke(false);
    visuals.widgets.noninteractive.rounding = card_rounding();

    visuals.widgets.inactive.bg_fill = button_inactive_fill();
    visuals.widgets.inactive.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(255, 255, 255, 12));
    visuals.widgets.inactive.rounding = button_rounding();

    visuals.widgets.hovered.bg_fill = button_hover_fill();
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent_color());
    visuals.widgets.hovered.rounding = button_rounding();

    visuals.widgets.active.bg_fill = accent_color();
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent_color());
    visuals.widgets.active.rounding = button_rounding();

    // Selection visuals
    visuals.selection.bg_fill = selection_fill();
    visuals.selection.stroke = selection_stroke();

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
// Platform-Adaptive Color Palette
// ---------------------------------------------------------------------------

/// Primary window and panel background color.
#[must_use]
pub(crate) const fn panel_background() -> Color32 {
    #[cfg(target_os = "macos")]
    {
        // Apple HIG Dark Mode NSColor.windowBackgroundColor (#1E1E1E)
        Color32::from_rgb(30, 30, 30)
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 11 Fluent Dark Mica Alt / Solid (#202020)
        Color32::from_rgb(32, 32, 32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux Adwaita Dark (#242424)
        Color32::from_rgb(36, 36, 36)
    }
}

/// Background fill for item cards.
#[must_use]
pub(crate) const fn card_fill(is_selected: bool) -> Color32 {
    if is_selected {
        #[cfg(target_os = "macos")]
        {
            // Apple System Blue tinted dark selection (#183350)
            Color32::from_rgb(24, 51, 80)
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 11 Subtle Blue selection (#1D364A)
            Color32::from_rgb(29, 54, 74)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(31, 56, 88)
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            // Apple Secondary Grouped Background (#2C2C2E)
            Color32::from_rgb(44, 44, 46)
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 11 CardBackgroundFillDefault (#2B2B2B)
            Color32::from_rgb(43, 43, 43)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Color32::from_rgb(46, 46, 48)
        }
    }
}

/// Border stroke for item cards.
#[must_use]
pub(crate) fn card_stroke(is_selected: bool) -> Stroke {
    if is_selected {
        Stroke::new(1.5_f32, accent_color())
    } else {
        #[cfg(target_os = "macos")]
        {
            // Subtle Apple separator hairline (#3A3A3C)
            Stroke::new(1.0_f32, Color32::from_rgb(58, 58, 60))
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 11 CardStrokeColorDefault (#383838)
            Stroke::new(1.0_f32, Color32::from_rgb(56, 56, 56))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(58, 58, 60))
        }
    }
}

/// Selection background fill for egui visuals.
#[must_use]
pub(crate) const fn selection_fill() -> Color32 {
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
}

/// Selection stroke for egui visuals.
#[must_use]
pub(crate) fn selection_stroke() -> Stroke {
    Stroke::new(1.5_f32, accent_color())
}

/// Primary accent color.
#[must_use]
pub(crate) const fn accent_color() -> Color32 {
    #[cfg(target_os = "macos")]
    {
        // Apple System Blue (#0A84FF)
        Color32::from_rgb(10, 132, 255)
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 11 Accent Blue (#0078D4)
        Color32::from_rgb(0, 120, 212)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux Adwaita Blue (#3584E4)
        Color32::from_rgb(53, 132, 228)
    }
}

/// Search box background color (Spotlight-style on macOS, Fluent on Windows).
#[must_use]
pub(crate) const fn search_bg() -> Color32 {
    #[cfg(target_os = "macos")]
    {
        // Apple Spotlight / Raycast input fill (#262628)
        Color32::from_rgb(38, 38, 40)
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 11 TextBox fill (#2D2D2D)
        Color32::from_rgb(45, 45, 45)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Color32::from_rgb(42, 42, 44)
    }
}

/// Search box border stroke.
#[must_use]
pub(crate) fn search_stroke(is_focused: bool) -> Stroke {
    if is_focused {
        Stroke::new(1.5_f32, accent_color())
    } else {
        #[cfg(target_os = "macos")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(58, 58, 60))
        }
        #[cfg(target_os = "windows")]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(60, 60, 60))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Stroke::new(1.0_f32, Color32::from_rgb(58, 58, 60))
        }
    }
}

/// Button background when inactive.
#[must_use]
pub(crate) const fn button_inactive_fill() -> Color32 {
    #[cfg(target_os = "macos")]
    {
        Color32::from_rgba_premultiplied(255, 255, 255, 14)
    }
    #[cfg(target_os = "windows")]
    {
        Color32::from_rgb(45, 45, 45)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Color32::from_rgb(48, 48, 50)
    }
}

/// Button background when hovered.
#[must_use]
pub(crate) const fn button_hover_fill() -> Color32 {
    #[cfg(target_os = "macos")]
    {
        Color32::from_rgba_premultiplied(255, 255, 255, 26)
    }
    #[cfg(target_os = "windows")]
    {
        Color32::from_rgb(54, 54, 54)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Color32::from_rgb(56, 56, 58)
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
        // Apple HIG squircle card corners (10.0px)
        Rounding::same(10.0_f32)
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 11 card radius (6.0px)
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
        // Smooth Apple pill badge (12.0px)
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

/// Left padding for the top header row.
///
/// On macOS with a fullsize content view and transparent titlebar,
/// this reserves room for the native macOS window traffic lights (close, minimize, zoom).
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

/// Renders a category pill tag with subtle tinted background and distinct colored text.
pub(crate) fn render_pill_tag(
    ui: &mut egui::Ui,
    icon: &str,
    text: &str,
    text_color: Color32,
    bg_tint: Color32,
) {
    egui::Frame::none()
        .rounding(pill_rounding())
        .fill(bg_tint)
        .inner_margin(egui::Margin::symmetric(6.0_f32, 2.0_f32))
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
    selected: bool,
    icon: &str,
    label: &str,
    count: usize,
) -> egui::Response {
    let (fill, stroke, text_color) = if selected {
        #[cfg(target_os = "macos")]
        let sel_fill = Color32::from_rgba_premultiplied(255, 255, 255, 38);
        #[cfg(not(target_os = "macos"))]
        let sel_fill = Color32::from_rgba_premultiplied(255, 255, 255, 28);

        (
            sel_fill,
            Stroke::new(1.0_f32, accent_color()),
            Color32::WHITE,
        )
    } else {
        (
            Color32::TRANSPARENT,
            Stroke::NONE,
            Color32::from_rgb(160, 160, 165),
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
