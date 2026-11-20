use crate::db::Database;
use crate::password::PasswordConfig;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};

/// Preconfigured global shortcut choices available to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HotkeyPreset {
    /// Universal standard: Cmd+Shift+V (macOS) / Ctrl+Shift+V (Win/Linux)
    Universal,
    /// French AZERTY keyboard quote key: Cmd+ù (macOS) / Ctrl+ù (Win/Linux)
    AzertyQuote,
    /// Secondary copy shortcut: Cmd+Shift+C (macOS) / Ctrl+Shift+C (Win/Linux)
    CtrlShiftC,
    /// Spotlight style: Option+Space (macOS) / Alt+Space (Win/Linux)
    AltSpace,
    /// Compact: Cmd+Space (macOS) / Ctrl+Space (Win/Linux)
    CtrlSpace,
    /// No global shortcut
    None,
    /// User-defined custom shortcut (stored separately in settings)
    Custom,
}

impl HotkeyPreset {
    /// Returns `(display_macos, display_other, badge_macos, badge_other)`.
    const fn platform_labels(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Universal => (
                "Cmd + Shift + V (Universal recommended)",
                "Ctrl + Shift + V (Universal recommended)",
                "Cmd+Shift+V",
                "Ctrl+Shift+V",
            ),
            Self::AzertyQuote => (
                "Cmd + ù (French AZERTY)",
                "Ctrl + ù (French AZERTY)",
                "Cmd+ù",
                "Ctrl+ù",
            ),
            Self::CtrlShiftC => (
                "Cmd + Shift + C",
                "Ctrl + Shift + C",
                "Cmd+Shift+C",
                "Ctrl+Shift+C",
            ),
            Self::AltSpace => ("Option + Space", "Alt + Space", "Option+Space", "Alt+Space"),
            Self::CtrlSpace => ("Cmd + Space", "Ctrl + Space", "Cmd+Space", "Ctrl+Space"),
            Self::None => ("Disabled", "Disabled", "Disabled", "Disabled"),
            Self::Custom => ("Custom…", "Custom…", "Custom", "Custom"),
        }
    }

    /// Human-readable label for settings UI.
    #[must_use]
    pub(crate) const fn display_name(self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            self.platform_labels().0
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.platform_labels().1
        }
    }

    /// Short badge for header display.
    #[must_use]
    pub(crate) const fn badge(self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            self.platform_labels().2
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.platform_labels().3
        }
    }

    /// Converts this preset into the corresponding `global_hotkey::hotkey::HotKey` objects.
    /// Returns empty vec for `None` or `Custom` (custom hotkeys are resolved via settings).
    #[must_use]
    pub(crate) fn to_hotkeys(self) -> Vec<HotKey> {
        #[cfg(target_os = "macos")]
        let base_mod = Modifiers::META;
        #[cfg(not(target_os = "macos"))]
        let base_mod = Modifiers::CONTROL;

        match self {
            Self::Universal => vec![HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::KeyV)],
            Self::AzertyQuote => {
                #[cfg(target_os = "windows")]
                if crate::platform::quote_key_is_ugrave() {
                    vec![
                        HotKey::new(Some(base_mod), Code::Backquote),
                        HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Backquote),
                    ]
                } else {
                    vec![
                        HotKey::new(Some(base_mod), Code::Quote),
                        HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Quote),
                    ]
                }
                #[cfg(not(target_os = "windows"))]
                vec![
                    HotKey::new(Some(base_mod), Code::Quote),
                    HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Quote),
                ]
            }
            Self::CtrlShiftC => vec![HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::KeyC)],
            Self::AltSpace => vec![HotKey::new(Some(Modifiers::ALT), Code::Space)],
            Self::CtrlSpace => vec![HotKey::new(Some(base_mod), Code::Space)],
            Self::None | Self::Custom => vec![],
        }
    }

    /// All presets available in the selector (excludes None and Custom which are handled specially).
    #[must_use]
    pub(crate) const fn all_presets() -> &'static [Self] {
        &[
            Self::Universal,
            Self::AzertyQuote,
            Self::CtrlShiftC,
            Self::AltSpace,
            Self::CtrlSpace,
            Self::None,
            Self::Custom,
        ]
    }
}

/// User's preferred color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum AppThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl AppThemeMode {
    /// Human-readable label for settings UI.
    #[must_use]
    pub(crate) const fn display_name(self) -> &'static str {
        match self {
            Self::System => "System (Auto)",
            Self::Dark => "Dark Mode",
            Self::Light => "Light Mode",
        }
    }

    /// Resolves this preference to a concrete boolean (true = dark, false = light).
    #[must_use]
    pub(crate) fn resolve(self, ctx: &eframe::egui::Context) -> bool {
        match self {
            Self::Dark => true,
            Self::Light => false,
            Self::System => ctx
                .system_theme()
                .is_none_or(|t| t == eframe::egui::Theme::Dark),
        }
    }
}

/// Parsed representation of a custom hotkey string (e.g., "Ctrl+Shift+X").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedHotkey {
    modifiers: Modifiers,
    key: Code,
}

impl ParsedHotkey {
    /// Parses a string like "Ctrl+Shift+V" into modifiers and key code.
    #[allow(clippy::too_many_lines, clippy::match_same_arms)]
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }
        let key_str = parts.last().copied()?.trim();
        let mod_strs = &parts[..parts.len() - 1];

        let mut modifiers = Modifiers::empty();
        for m in mod_strs {
            let m = m.trim().to_lowercase();
            match m.as_str() {
                "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
                "alt" | "option" => modifiers |= Modifiers::ALT,
                "shift" => modifiers |= Modifiers::SHIFT,
                "cmd" | "meta" | "super" | "win" => modifiers |= Modifiers::META,
                _ => return None,
            }
        }

        #[cfg(target_os = "windows")]
        let azerty = crate::platform::quote_key_is_ugrave();
        #[cfg(not(target_os = "windows"))]
        let azerty = false;

        let key = match key_str.to_lowercase().as_str() {
            "space" => Code::Space,
            "enter" | "return" => Code::Enter,
            "tab" => Code::Tab,
            "esc" | "escape" => Code::Escape,
            "backspace" => Code::Backspace,
            "delete" => Code::Delete,
            "home" => Code::Home,
            "end" => Code::End,
            "pageup" => Code::PageUp,
            "pagedown" => Code::PageDown,
            "up" => Code::ArrowUp,
            "down" => Code::ArrowDown,
            "left" => Code::ArrowLeft,
            "right" => Code::ArrowRight,
            "insert" => Code::Insert,
            "f1" => Code::F1,
            "f2" => Code::F2,
            "f3" => Code::F3,
            "f4" => Code::F4,
            "f5" => Code::F5,
            "f6" => Code::F6,
            "f7" => Code::F7,
            "f8" => Code::F8,
            "f9" => Code::F9,
            "f10" => Code::F10,
            "f11" => Code::F11,
            "f12" => Code::F12,
            "f13" => Code::F13,
            "f14" => Code::F14,
            "f15" => Code::F15,
            "f16" => Code::F16,
            "f17" => Code::F17,
            "f18" => Code::F18,
            "f19" => Code::F19,
            "f20" => Code::F20,
            "f21" => Code::F21,
            "f22" => Code::F22,
            "f23" => Code::F23,
            "f24" => Code::F24,
            "ù" if azerty => Code::Backquote,
            "ù" => Code::Quote,
            "²" => Code::Quote,
            "quote" | "'" if azerty => Code::Backquote,
            "quote" | "'" => Code::Quote,
            "comma" | "," => Code::Comma,
            "minus" | "-" => Code::Minus,
            "equal" | "equals" | "=" | "plus" | "+" => Code::Equal,
            "period" | "." => Code::Period,
            "slash" | "/" | "questionmark" | "?" => Code::Slash,
            "semicolon" | ";" | "colon" | ":" => Code::Semicolon,
            "backslash" | "\\" | "pipe" | "|" => Code::Backslash,
            "bracketleft" | "openbracket" | "[" => Code::BracketLeft,
            "bracketright" | "closebracket" | "]" => Code::BracketRight,
            "grave" | "backtick" | "`" => Code::Backquote,
            "capslock" => Code::CapsLock,
            "scrolllock" => Code::ScrollLock,
            "numlock" => Code::NumLock,
            "printscreen" => Code::PrintScreen,
            "pause" => Code::Pause,
            s if s.len() == 1 => {
                // Single character keys
                let ch = s.chars().next()?;
                if ch.is_ascii_alphabetic() {
                    match ch.to_ascii_uppercase() {
                        'A' => Code::KeyA,
                        'B' => Code::KeyB,
                        'C' => Code::KeyC,
                        'D' => Code::KeyD,
                        'E' => Code::KeyE,
                        'F' => Code::KeyF,
                        'G' => Code::KeyG,
                        'H' => Code::KeyH,
                        'I' => Code::KeyI,
                        'J' => Code::KeyJ,
                        'K' => Code::KeyK,
                        'L' => Code::KeyL,
                        'M' => Code::KeyM,
                        'N' => Code::KeyN,
                        'O' => Code::KeyO,
                        'P' => Code::KeyP,
                        'Q' => Code::KeyQ,
                        'R' => Code::KeyR,
                        'S' => Code::KeyS,
                        'T' => Code::KeyT,
                        'U' => Code::KeyU,
                        'V' => Code::KeyV,
                        'W' => Code::KeyW,
                        'X' => Code::KeyX,
                        'Y' => Code::KeyY,
                        'Z' => Code::KeyZ,
                        _ => return None,
                    }
                } else if ch.is_ascii_digit() {
                    match ch {
                        '0' => Code::Digit0,
                        '1' => Code::Digit1,
                        '2' => Code::Digit2,
                        '3' => Code::Digit3,
                        '4' => Code::Digit4,
                        '5' => Code::Digit5,
                        '6' => Code::Digit6,
                        '7' => Code::Digit7,
                        '8' => Code::Digit8,
                        '9' => Code::Digit9,
                        _ => return None,
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        Some(Self { modifiers, key })
    }

    /// Converts to a `HotKey` with a new unique ID.
    fn to_hotkey(&self) -> HotKey {
        HotKey::new(Some(self.modifiers), self.key)
    }

    /// Formats back to a display string like "Ctrl+Shift+V".
    #[allow(clippy::too_many_lines)]
    pub(crate) fn format(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.contains(Modifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.modifiers.contains(Modifiers::ALT) {
            parts.push("Alt");
        }
        if self.modifiers.contains(Modifiers::SHIFT) {
            parts.push("Shift");
        }
        if self.modifiers.contains(Modifiers::META) {
            #[cfg(target_os = "macos")]
            parts.push("Cmd");
            #[cfg(not(target_os = "macos"))]
            parts.push("Win");
        }
        parts.push(match self.key {
            Code::Space => "Space",
            Code::Enter => "Enter",
            Code::Tab => "Tab",
            Code::Escape => "Esc",
            Code::Backspace => "Backspace",
            Code::Delete => "Delete",
            Code::Home => "Home",
            Code::End => "End",
            Code::PageUp => "PageUp",
            Code::PageDown => "PageDown",
            Code::ArrowUp => "Up",
            Code::ArrowDown => "Down",
            Code::ArrowLeft => "Left",
            Code::ArrowRight => "Right",
            Code::Insert => "Insert",
            Code::F1 => "F1",
            Code::F2 => "F2",
            Code::F3 => "F3",
            Code::F4 => "F4",
            Code::F5 => "F5",
            Code::F6 => "F6",
            Code::F7 => "F7",
            Code::F8 => "F8",
            Code::F9 => "F9",
            Code::F10 => "F10",
            Code::F11 => "F11",
            Code::F12 => "F12",
            Code::F13 => "F13",
            Code::F14 => "F14",
            Code::F15 => "F15",
            Code::F16 => "F16",
            Code::F17 => "F17",
            Code::F18 => "F18",
            Code::F19 => "F19",
            Code::F20 => "F20",
            Code::F21 => "F21",
            Code::F22 => "F22",
            Code::F23 => "F23",
            Code::F24 => "F24",
            Code::Quote => "Quote",
            Code::Comma => "Comma",
            Code::Minus => "Minus",
            Code::Equal => "Equal",
            Code::Period => "Period",
            Code::Slash => "Slash",
            Code::Semicolon => "Semicolon",
            Code::Backslash => "Backslash",
            Code::BracketLeft => "BracketLeft",
            Code::BracketRight => "BracketRight",
            Code::Backquote => {
                if crate::platform::quote_key_is_ugrave() {
                    "ù"
                } else {
                    "Grave"
                }
            }
            Code::CapsLock => "CapsLock",
            Code::ScrollLock => "ScrollLock",
            Code::NumLock => "NumLock",
            Code::PrintScreen => "PrintScreen",
            Code::Pause => "Pause",
            Code::KeyA => "A",
            Code::KeyB => "B",
            Code::KeyC => "C",
            Code::KeyD => "D",
            Code::KeyE => "E",
            Code::KeyF => "F",
            Code::KeyG => "G",
            Code::KeyH => "H",
            Code::KeyI => "I",
            Code::KeyJ => "J",
            Code::KeyK => "K",
            Code::KeyL => "L",
            Code::KeyM => "M",
            Code::KeyN => "N",
            Code::KeyO => "O",
            Code::KeyP => "P",
            Code::KeyQ => "Q",
            Code::KeyR => "R",
            Code::KeyS => "S",
            Code::KeyT => "T",
            Code::KeyU => "U",
            Code::KeyV => "V",
            Code::KeyW => "W",
            Code::KeyX => "X",
            Code::KeyY => "Y",
            Code::KeyZ => "Z",
            Code::Digit0 => "0",
            Code::Digit1 => "1",
            Code::Digit2 => "2",
            Code::Digit3 => "3",
            Code::Digit4 => "4",
            Code::Digit5 => "5",
            Code::Digit6 => "6",
            Code::Digit7 => "7",
            Code::Digit8 => "8",
            Code::Digit9 => "9",
            _ => "?",
        });
        parts.join("+")
    }
}

/// Persistent user configuration for the clipboard manager.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    pub(crate) hotkey: HotkeyPreset,
    #[serde(default)]
    pub(crate) custom_hotkey: String, // Stores custom hotkey string like "Ctrl+Shift+X"
    #[serde(default)]
    pub(crate) theme: AppThemeMode,
    pub(crate) hide_on_copy: bool,
    pub(crate) max_history: usize,
    #[serde(default = "default_hide_on_blur")]
    pub(crate) hide_on_blur: bool,
    #[serde(default = "default_restore_focus")]
    pub(crate) restore_focus: bool,
    #[serde(default = "default_auto_paste")]
    pub(crate) auto_paste: bool,
    #[serde(default = "default_ignore_concealed")]
    pub(crate) ignore_concealed: bool,
    #[serde(default = "default_auto_detect_secrets")]
    pub(crate) auto_detect_secrets: bool,
    #[serde(default = "default_auto_clear_secret_secs")]
    pub(crate) auto_clear_secret_secs: u64,
    #[serde(default = "default_auto_paste_delay_ms")]
    pub(crate) auto_paste_delay_ms: u64,
    #[serde(default)]
    pub(crate) password_config: PasswordConfig,
    #[serde(default = "default_generator_skip_history")]
    pub(crate) generator_skip_history: bool,
    #[serde(default = "default_generator_reveal_value")]
    pub(crate) generator_reveal_value: bool,
    #[serde(default = "default_keyring_explanation_shown")]
    pub(crate) keyring_explanation_shown: bool,
}

const fn default_hide_on_blur() -> bool {
    #[cfg(target_os = "windows")]
    {
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

const fn default_restore_focus() -> bool {
    true
}

const fn default_auto_paste() -> bool {
    true
}

const fn default_ignore_concealed() -> bool {
    true
}

const fn default_auto_detect_secrets() -> bool {
    true
}

const fn default_auto_clear_secret_secs() -> u64 {
    30
}

const fn default_auto_paste_delay_ms() -> u64 {
    120
}

const fn default_generator_skip_history() -> bool {
    true
}

const fn default_generator_reveal_value() -> bool {
    true
}

const fn default_keyring_explanation_shown() -> bool {
    false
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyPreset::Universal,
            custom_hotkey: String::new(),
            theme: AppThemeMode::System,
            hide_on_copy: true,
            max_history: 100,
            hide_on_blur: default_hide_on_blur(),
            restore_focus: true,
            auto_paste: true,
            auto_paste_delay_ms: 120,
            ignore_concealed: true,
            auto_detect_secrets: true,
            auto_clear_secret_secs: 30,
            password_config: PasswordConfig::default(),
            generator_skip_history: true,
            generator_reveal_value: true,
            keyring_explanation_shown: false,
        }
    }
}

impl AppSettings {
    const SETTINGS_KEY: &str = "app_config";

    /// Loads settings from the database, falling back to default if unset or corrupted.
    #[must_use]
    pub(crate) fn load(db: &Database) -> Self {
        if let Ok(Some(json)) = db.get_setting(Self::SETTINGS_KEY) {
            if let Ok(settings) = serde_json::from_str(&json) {
                return settings;
            }
        }
        Self::default()
    }

    /// Saves the settings to the database.
    pub(crate) fn save(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string(self)?;
        db.set_setting(Self::SETTINGS_KEY, &json)?;
        Ok(())
    }

    /// Returns the effective hotkeys for the current preset, including custom hotkey resolution.
    pub(crate) fn effective_hotkeys(&self) -> Vec<HotKey> {
        match self.hotkey {
            HotkeyPreset::Custom => ParsedHotkey::parse(&self.custom_hotkey)
                .map_or_else(Vec::new, |parsed| vec![parsed.to_hotkey()]),
            _ => self.hotkey.to_hotkeys(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn test_settings_default_and_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("settings_module_test.db");
        let db = Database::open(&db_path, TEST_KEY).unwrap();

        // Initially default
        let loaded = AppSettings::load(&db);
        assert_eq!(loaded, AppSettings::default());

        // Modify and save
        let mut modified = loaded;
        modified.hotkey = HotkeyPreset::AltSpace;
        modified.hide_on_copy = false;
        modified.hide_on_blur = false;
        modified.restore_focus = false;
        modified.auto_paste = false;
        modified.auto_clear_secret_secs = 45;
        modified.max_history = 250;
        modified.generator_skip_history = false;
        modified.generator_reveal_value = false;
        modified.password_config.length = 32;
        modified.password_config.exclude_ambiguous = true;
        modified.password_config.mode = crate::password::GeneratorMode::Passphrase;
        modified.password_config.word_count = 6;
        modified.password_config.capitalize = true;
        modified.save(&db).expect("Save should succeed");

        // Reload
        let reloaded = AppSettings::load(&db);
        assert_eq!(reloaded.hotkey, HotkeyPreset::AltSpace);
        assert!(!reloaded.hide_on_copy);
        assert!(!reloaded.hide_on_blur);
        assert!(!reloaded.restore_focus);
        assert!(!reloaded.auto_paste);
        assert_eq!(reloaded.auto_clear_secret_secs, 45);
        assert_eq!(reloaded.max_history, 250);
        assert!(!reloaded.generator_skip_history);
        assert!(!reloaded.generator_reveal_value);
        assert_eq!(reloaded.password_config.length, 32);
        assert!(reloaded.password_config.exclude_ambiguous);
        assert_eq!(
            reloaded.password_config.mode,
            crate::password::GeneratorMode::Passphrase
        );
        assert_eq!(reloaded.password_config.word_count, 6);
        assert!(reloaded.password_config.capitalize);
    }

    #[test]
    fn test_hotkey_preset_conversions() {
        for preset in HotkeyPreset::all_presets() {
            let hotkeys = preset.to_hotkeys();
            if *preset != HotkeyPreset::None && *preset != HotkeyPreset::Custom {
                assert!(
                    !hotkeys.is_empty(),
                    "Preset must generate at least 1 hotkey"
                );
            }
            assert_ne!(preset.display_name(), "");
            assert_ne!(preset.badge(), "");
        }
    }

    #[test]
    fn test_custom_hotkey_parsing() {
        // Test basic parsing
        let parsed = ParsedHotkey::parse("Ctrl+Shift+V").unwrap();
        assert!(parsed.modifiers.contains(Modifiers::CONTROL));
        assert!(parsed.modifiers.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::KeyV);
        assert_eq!(parsed.format(), "Ctrl+Shift+V");

        // Test Alt+Space
        let parsed = ParsedHotkey::parse("Alt+Space").unwrap();
        assert!(parsed.modifiers.contains(Modifiers::ALT));
        assert_eq!(parsed.key, Code::Space);

        // Test Cmd+Option+Shift+X (macOS)
        let parsed = ParsedHotkey::parse("Cmd+Option+Shift+X").unwrap();
        assert!(parsed.modifiers.contains(Modifiers::META));
        assert!(parsed.modifiers.contains(Modifiers::ALT));
        assert!(parsed.modifiers.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::KeyX);

        // Test single key
        let parsed = ParsedHotkey::parse("F1").unwrap();
        assert_eq!(parsed.key, Code::F1);
        assert_eq!(parsed.modifiers, Modifiers::empty());

        // Test Ctrl+ù (layout-aware on Windows: Backquote on AZERTY, Quote otherwise)
        let parsed = ParsedHotkey::parse("Ctrl+ù").unwrap();
        assert!(parsed.modifiers.contains(Modifiers::CONTROL));
        if crate::platform::quote_key_is_ugrave() {
            assert_eq!(parsed.key, Code::Backquote);
            assert_eq!(parsed.format(), "Ctrl+ù");
        } else {
            assert_eq!(parsed.key, Code::Quote);
            assert_eq!(parsed.format(), "Ctrl+Quote");
        }

        // Test invalid
        assert!(ParsedHotkey::parse("").is_none());
        assert!(ParsedHotkey::parse("InvalidKey").is_none());
        assert!(ParsedHotkey::parse("Ctrl+InvalidMod+V").is_none());
    }

    #[test]
    fn test_theme_mode_serialization_and_legacy_compatibility() {
        assert_eq!(AppThemeMode::default(), AppThemeMode::System);
        assert_eq!(AppThemeMode::System.display_name(), "System (Auto)");
        assert_eq!(AppThemeMode::Dark.display_name(), "Dark Mode");
        assert_eq!(AppThemeMode::Light.display_name(), "Light Mode");

        // Legacy JSON without "theme" field should deserialize with AppThemeMode::System
        let legacy_json = r#"{
            "hotkey": "Universal",
            "hide_on_copy": true,
            "max_history": 100,
            "hide_on_blur": true,
            "restore_focus": true,
            "auto_paste": true
        }"#;
        let parsed: AppSettings =
            serde_json::from_str(legacy_json).expect("Legacy JSON should parse");
        assert_eq!(parsed.theme, AppThemeMode::System);
        assert!(parsed.ignore_concealed);

        // Explicit theme mode JSON
        let light_json = r#"{
            "hotkey": "Universal",
            "theme": "Light",
            "hide_on_copy": true,
            "max_history": 100
        }"#;
        let parsed_light: AppSettings =
            serde_json::from_str(light_json).expect("Light theme JSON should parse");
        assert_eq!(parsed_light.theme, AppThemeMode::Light);
    }
}
