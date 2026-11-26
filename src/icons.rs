//! Application icon definitions backed by `egui_material_icons` (Material Symbols Rounded,
//! with both `filled` and `outline` variants registered).

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, TextFormat};
use egui_material_icons::{icons as mi, MaterialIcon, FONT_FAMILY, FONT_FAMILY_OUTLINED};

pub(crate) const ARROWS_CLOCKWISE: &str = mi::ICON_REFRESH.codepoint;
pub(crate) const ARROW_LEFT: &str = mi::ICON_ARROW_BACK.codepoint;
pub(crate) const ARROW_SQUARE_OUT: &str = mi::ICON_OPEN_IN_NEW.codepoint;
pub(crate) const BRACKETS_CURLY: &str = mi::ICON_DATA_OBJECT.codepoint;
pub(crate) const CHECK: &str = mi::ICON_CHECK.codepoint;
#[allow(dead_code)]
pub(crate) const CLIPBOARD: &str = mi::ICON_CONTENT_PASTE.codepoint;
pub(crate) const CLOCK_COUNTER_CLOCKWISE: &str = mi::ICON_HISTORY.codepoint;
pub(crate) const CODE: &str = mi::ICON_CODE.codepoint;
pub(crate) const COPY: &str = mi::ICON_CONTENT_COPY.codepoint;
pub(crate) const DATABASE: &str = mi::ICON_DATABASE.codepoint;
pub(crate) const EYE: &str = mi::ICON_VISIBILITY.codepoint;
pub(crate) const EYE_SLASH: &str = mi::ICON_VISIBILITY_OFF.codepoint;
pub(crate) const FILE_TEXT: &str = mi::ICON_DESCRIPTION.codepoint;
pub(crate) const GEAR: &str = mi::ICON_SETTINGS.codepoint;
pub(crate) const IMAGE: &str = mi::ICON_IMAGE.codepoint;
pub(crate) const INFO: &str = mi::ICON_INFO.codepoint;
pub(crate) const KEY: &str = mi::ICON_KEY.codepoint;
pub(crate) const KEYBOARD: &str = mi::ICON_KEYBOARD.codepoint;
pub(crate) const LINK: &str = mi::ICON_LINK.codepoint;
pub(crate) const LOCK: &str = mi::ICON_LOCK.codepoint;
pub(crate) const LOCK_KEY: &str = mi::ICON_ENHANCED_ENCRYPTION.codepoint;
pub(crate) const LOCK_OPEN: &str = mi::ICON_LOCK_OPEN.codepoint;
pub(crate) const MAGNIFYING_GLASS: &str = mi::ICON_SEARCH.codepoint;
pub(crate) const PALETTE: &str = mi::ICON_PALETTE.codepoint;
pub(crate) const PENCIL: &str = mi::ICON_EDIT.codepoint;
pub(crate) const PUSH_PIN: &str = mi::ICON_PUSH_PIN.codepoint;
#[allow(dead_code)]
pub(crate) const POWER: &str = mi::ICON_POWER_SETTINGS_NEW.codepoint;
pub(crate) const SHIELD_CHECK: &str = mi::ICON_VERIFIED_USER.codepoint;
pub(crate) const SLIDERS: &str = mi::ICON_TUNE.codepoint;
pub(crate) const STACK: &str = mi::ICON_STACKS.codepoint;
pub(crate) const TERMINAL: &str = mi::ICON_TERMINAL.codepoint;
pub(crate) const TRASH: &str = mi::ICON_DELETE.codepoint;
pub(crate) const WARNING: &str = mi::ICON_WARNING.codepoint;
pub(crate) const X: &str = mi::ICON_CLOSE.codepoint;

/// Returns a [`egui::RichText`] rendering `codepoint` with the outlined Material Symbols font.
pub(crate) fn outlined(codepoint: &'static str) -> egui::RichText {
    MaterialIcon::new(codepoint).outlined().rich_text()
}

/// Icon-only buttons render glyphs at this size (egui's small-button text is tiny).
pub(crate) const BUTTON_ICON_SIZE: f32 = 14.0;

/// Multiplier applied to icons placed before label text so they match text visual weight.
const LABEL_ICON_SCALE: f32 = 1.15;

/// Returns an outlined icon [`egui::RichText`] sized for icon-only buttons.
pub(crate) fn button_icon(codepoint: &'static str) -> egui::RichText {
    MaterialIcon::new(codepoint)
        .outlined()
        .rich_text()
        .size(BUTTON_ICON_SIZE)
}

/// Builds a [`LayoutJob`] combining a Material icon (explicitly `filled` or `outlined`)
/// followed by a space and proportional label text, vertically centered.
pub(crate) fn label_job(
    codepoint: &str,
    filled: bool,
    text: &str,
    size: f32,
    color: Color32,
) -> LayoutJob {
    let icon_family = if filled {
        FontFamily::Name(FONT_FAMILY.into())
    } else {
        FontFamily::Name(FONT_FAMILY_OUTLINED.into())
    };
    let mut job = LayoutJob::default();
    job.append(
        codepoint,
        0.0,
        TextFormat {
            font_id: FontId::new(size * LABEL_ICON_SCALE, icon_family),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        &format!(" {text}"),
        0.0,
        TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job
}
