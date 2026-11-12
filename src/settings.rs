use crate::db::Database;
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
            Self::Universal => "⌘ + ⇧ + V (Universal recommended)",
            Self::AzertyQuote => "⌘ + ù (French AZERTY)",
            Self::CtrlShiftC => "⌘ + ⇧ + C",
            Self::AltSpace => "⌥ Option + Space",
            Self::CtrlSpace => "⌘ + Space",
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
            Self::Universal => "⌘⇧V",
            Self::AzertyQuote => "⌘ ù",
            Self::CtrlShiftC => "⌘⇧C",
            Self::AltSpace => "⌥Space",
            Self::CtrlSpace => "⌘Space",
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

/// Persistent user configuration for the clipboard manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    pub(crate) hotkey: HotkeyPreset,
    pub(crate) hide_on_copy: bool,
    pub(crate) max_history: usize,
    #[serde(default = "default_hide_on_blur")]
    pub(crate) hide_on_blur: bool,
}

const fn default_hide_on_blur() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyPreset::Universal,
            hide_on_copy: true,
            max_history: 100,
            hide_on_blur: true,
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
        modified.max_history = 250;
        modified.save(&db).expect("Save should succeed");

        // Reload
        let reloaded = AppSettings::load(&db);
        assert_eq!(reloaded.hotkey, HotkeyPreset::AltSpace);
        assert!(!reloaded.hide_on_copy);
        assert!(!reloaded.hide_on_blur);
        assert_eq!(reloaded.max_history, 250);
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
}
