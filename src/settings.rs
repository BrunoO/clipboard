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
}

impl HotkeyPreset {
    /// Human-readable label for settings UI.
    #[must_use]
    pub(crate) const fn display_name(self) -> &'static str {
        #[cfg(target_os = "macos")]
        match self {
            Self::Universal => "Cmd + Shift + V (Universal recommended)",
            Self::AzertyQuote => "Cmd + ù (French AZERTY)",
            Self::CtrlShiftC => "Cmd + Shift + C",
            Self::AltSpace => "Option + Space",
            Self::CtrlSpace => "Cmd + Space",
        }

        #[cfg(not(target_os = "macos"))]
        match self {
            Self::Universal => "Ctrl + Shift + V (Universal recommended)",
            Self::AzertyQuote => "Ctrl + ù (French AZERTY)",
            Self::CtrlShiftC => "Ctrl + Shift + C",
            Self::AltSpace => "Alt + Space",
            Self::CtrlSpace => "Ctrl + Space",
        }
    }

    /// Short badge for header display.
    #[must_use]
    pub(crate) const fn badge(self) -> &'static str {
        #[cfg(target_os = "macos")]
        match self {
            Self::Universal => "Cmd+Shift+V",
            Self::AzertyQuote => "Cmd+ù",
            Self::CtrlShiftC => "Cmd+Shift+C",
            Self::AltSpace => "Option+Space",
            Self::CtrlSpace => "Cmd+Space",
        }

        #[cfg(not(target_os = "macos"))]
        match self {
            Self::Universal => "Ctrl+Shift+V",
            Self::AzertyQuote => "Ctrl+ù",
            Self::CtrlShiftC => "Ctrl+Shift+C",
            Self::AltSpace => "Alt+Space",
            Self::CtrlSpace => "Ctrl+Space",
        }
    }

    /// Converts this preset into the corresponding `global_hotkey::hotkey::HotKey` objects.
    #[must_use]
    pub(crate) fn to_hotkeys(self) -> Vec<HotKey> {
        #[cfg(target_os = "macos")]
        let base_mod = Modifiers::META;
        #[cfg(not(target_os = "macos"))]
        let base_mod = Modifiers::CONTROL;

        match self {
            Self::Universal => vec![HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::KeyV)],
            Self::AzertyQuote => vec![
                HotKey::new(Some(base_mod), Code::Quote),
                HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::Quote),
            ],
            Self::CtrlShiftC => vec![HotKey::new(Some(base_mod | Modifiers::SHIFT), Code::KeyC)],
            Self::AltSpace => vec![HotKey::new(Some(Modifiers::ALT), Code::Space)],
            Self::CtrlSpace => vec![HotKey::new(Some(base_mod), Code::Space)],
        }
    }

    /// All presets available in the selector.
    #[must_use]
    pub(crate) const fn all_presets() -> &'static [Self] {
        &[
            Self::Universal,
            Self::AzertyQuote,
            Self::CtrlShiftC,
            Self::AltSpace,
            Self::CtrlSpace,
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

/// Persistent user configuration for the clipboard manager.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    pub(crate) hotkey: HotkeyPreset,
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
    #[serde(default)]
    pub(crate) password_config: PasswordConfig,
    #[serde(default = "default_generator_skip_history")]
    pub(crate) generator_skip_history: bool,
    #[serde(default = "default_generator_reveal_value")]
    pub(crate) generator_reveal_value: bool,
}

const fn default_hide_on_blur() -> bool {
    true
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

const fn default_generator_skip_history() -> bool {
    true
}

const fn default_generator_reveal_value() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyPreset::Universal,
            theme: AppThemeMode::System,
            hide_on_copy: true,
            max_history: 100,
            hide_on_blur: true,
            restore_focus: true,
            auto_paste: true,
            ignore_concealed: true,
            auto_detect_secrets: true,
            auto_clear_secret_secs: 30,
            password_config: PasswordConfig::default(),
            generator_skip_history: true,
            generator_reveal_value: true,
        }
    }
}

impl AppSettings {
    const SETTINGS_KEY: &'static str = "app_config";

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
            assert!(
                !hotkeys.is_empty(),
                "Preset must generate at least 1 hotkey"
            );
            assert_ne!(preset.display_name(), "");
            assert_ne!(preset.badge(), "");
        }
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
