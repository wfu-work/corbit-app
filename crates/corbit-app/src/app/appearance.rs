use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

pub(super) const CODEX_DARK_BACKGROUND: u32 = 0x13_1313;
const PREVIOUS_CODEX_DARK_BACKGROUND: u32 = 0x18_1818;
const LEGACY_DARK_BACKGROUND: u32 = 0x20_2020;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

impl ColorScheme {
    pub(super) const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub(super) const fn card_label(self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::Light => "浅色",
            Self::Dark => "深色",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContrastLevel {
    Soft,
    #[default]
    Default,
    Strong,
}

impl ContrastLevel {
    pub(super) const ALL: [Self; 3] = [Self::Soft, Self::Default, Self::Strong];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Soft => "柔和",
            Self::Default => "默认",
            Self::Strong => "增强",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InterfaceFont {
    #[default]
    System,
    Sans,
}

impl InterfaceFont {
    pub(super) const ALL: [Self; 2] = [Self::System, Self::Sans];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::System => "系统默认",
            Self::Sans => "现代无衬线",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CodeFont {
    #[default]
    System,
    Classic,
}

impl CodeFont {
    pub(super) const ALL: [Self; 2] = [Self::System, Self::Classic];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::System => "系统等宽",
            Self::Classic => "经典等宽",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InterfaceTextSize {
    Small,
    #[default]
    Default,
    Large,
}

impl InterfaceTextSize {
    pub(super) const ALL: [Self; 3] = [Self::Small, Self::Default, Self::Large];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Small => "较小",
            Self::Default => "默认",
            Self::Large => "较大",
        }
    }

    pub(super) const fn scale_percent(self) -> u8 {
        match self {
            Self::Small => 93,
            Self::Default => 100,
            Self::Large => 107,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CodeTextSize {
    Small,
    #[default]
    Default,
    Large,
}

impl CodeTextSize {
    pub(super) const ALL: [Self; 3] = [Self::Small, Self::Default, Self::Large];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Small => "13 px",
            Self::Default => "14 px",
            Self::Large => "15 px",
        }
    }

    pub(super) const fn pixels(self) -> u8 {
        match self {
            Self::Small => 13,
            Self::Default => 14,
            Self::Large => 15,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContentWidth {
    Compact,
    #[default]
    Default,
    Wide,
}

impl ContentWidth {
    pub(super) const ALL: [Self; 3] = [Self::Compact, Self::Default, Self::Wide];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Compact => "紧凑",
            Self::Default => "默认",
            Self::Wide => "宽阔",
        }
    }

    pub(super) const fn pixels(self) -> u16 {
        match self {
            Self::Compact => 680,
            Self::Default => 768,
            Self::Wide => 920,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct AppearancePreferences {
    pub(super) color_scheme: ColorScheme,
    #[serde(with = "hex_color")]
    pub(super) accent_color: u32,
    #[serde(with = "hex_color")]
    pub(super) light_background: u32,
    #[serde(with = "hex_color")]
    pub(super) light_foreground: u32,
    #[serde(with = "hex_color")]
    pub(super) dark_background: u32,
    #[serde(with = "hex_color")]
    pub(super) dark_foreground: u32,
    pub(super) contrast: ContrastLevel,
    pub(super) translucent_sidebar: bool,
    pub(super) interface_font: InterfaceFont,
    pub(super) interface_text_size: InterfaceTextSize,
    pub(super) code_font: CodeFont,
    pub(super) code_text_size: CodeTextSize,
    pub(super) content_width: ContentWidth,
}

impl Default for AppearancePreferences {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::System,
            accent_color: 0x33_9cff,
            light_background: 0xff_ffff,
            light_foreground: 0x1a_1c1f,
            dark_background: CODEX_DARK_BACKGROUND,
            dark_foreground: 0xf5_f5f5,
            contrast: ContrastLevel::Default,
            translucent_sidebar: true,
            interface_font: InterfaceFont::System,
            interface_text_size: InterfaceTextSize::Default,
            code_font: CodeFont::System,
            code_text_size: CodeTextSize::Default,
            content_width: ContentWidth::Default,
        }
    }
}

impl AppearancePreferences {
    pub(super) fn load() -> Self {
        preferences_path()
            .and_then(|path| Self::load_from(&path).ok())
            .unwrap_or_default()
    }

    pub(super) fn save(self) -> anyhow::Result<()> {
        let path = preferences_path().ok_or_else(|| anyhow!("无法确定当前用户的配置目录"))?;
        self.save_to(&path)
    }

    pub(super) fn share_code(self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(&self).context("无法生成外观配置")
    }

    pub(super) fn from_share_code(value: &str) -> anyhow::Result<Self> {
        serde_json::from_str(value.trim()).context("外观配置格式无效")
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        let bytes =
            fs::read(path).with_context(|| format!("无法读取外观设置 {}", path.display()))?;
        let preferences: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("无法解析外观设置 {}", path.display()))?;
        Ok(preferences.migrate_legacy_defaults())
    }

    fn migrate_legacy_defaults(mut self) -> Self {
        // Normalize Corbit's previous built-in dark surfaces so existing
        // installations receive the current Codex palette without overwriting
        // any other appearance choices.
        if matches!(
            self.dark_background,
            LEGACY_DARK_BACKGROUND | PREVIOUS_CODEX_DARK_BACKGROUND
        ) {
            self.dark_background = CODEX_DARK_BACKGROUND;
        }
        self
    }

    fn save_to(self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("外观设置路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建配置目录 {}", parent.display()))?;
        let bytes = serde_json::to_vec_pretty(&self).context("无法序列化外观设置")?;
        fs::write(path, bytes).with_context(|| format!("无法写入外观设置 {}", path.display()))
    }
}

mod hex_color {
    use super::*;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ColorValue {
        Hex(String),
        Number(u32),
    }

    // Serde's `serialize_with` callback contract passes the field by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn serialize<S>(value: &u32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("#{:06x}", value & 0xff_ffff))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = ColorValue::deserialize(deserializer)?;
        let color = match value {
            ColorValue::Number(value) if value <= 0xff_ffff => value,
            ColorValue::Number(_) => return Err(D::Error::custom("颜色数值超出 #ffffff")),
            ColorValue::Hex(value) => {
                let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
                if value.len() != 6 {
                    return Err(D::Error::custom("颜色必须使用 #rrggbb 格式"));
                }
                u32::from_str_radix(value, 16).map_err(D::Error::custom)?
            }
        };
        Ok(color)
    }
}

#[cfg(target_os = "macos")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Corbit")
            .join("appearance.json")
    })
}

#[cfg(target_os = "windows")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("APPDATA").map(|directory| {
        PathBuf::from(directory)
            .join("Corbit")
            .join("appearance.json")
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|directory| directory.join("corbit").join("appearance.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_keep_defaults_for_new_fields() {
        let directory =
            std::env::temp_dir().join(format!("corbit-appearance-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("appearance.json");
        let preferences = AppearancePreferences {
            color_scheme: ColorScheme::Dark,
            accent_color: 0x8b_5cf6,
            contrast: ContrastLevel::Strong,
            translucent_sidebar: false,
            interface_font: InterfaceFont::Sans,
            interface_text_size: InterfaceTextSize::Large,
            code_font: CodeFont::Classic,
            code_text_size: CodeTextSize::Small,
            content_width: ContentWidth::Wide,
            ..AppearancePreferences::default()
        };

        preferences.save_to(&path).expect("preferences should save");
        assert_eq!(
            AppearancePreferences::load_from(&path).expect("preferences should load"),
            preferences
        );

        fs::write(&path, br#"{"colorScheme":"light"}"#).expect("partial preferences should save");
        assert_eq!(
            AppearancePreferences::load_from(&path).expect("partial preferences should load"),
            AppearancePreferences {
                color_scheme: ColorScheme::Light,
                ..AppearancePreferences::default()
            }
        );

        fs::write(&path, br##"{"darkBackground":"#202020"}"##)
            .expect("legacy preferences should save");
        assert_eq!(
            AppearancePreferences::load_from(&path)
                .expect("legacy preferences should migrate")
                .dark_background,
            CODEX_DARK_BACKGROUND
        );

        fs::write(&path, br##"{"darkBackground":"#181818"}"##)
            .expect("previous Codex preferences should save");
        assert_eq!(
            AppearancePreferences::load_from(&path)
                .expect("previous Codex preferences should migrate")
                .dark_background,
            CODEX_DARK_BACKGROUND
        );

        fs::remove_dir_all(directory).expect("temporary preferences should be removable");
    }

    #[test]
    fn shared_theme_round_trips_human_readable_colors() {
        let preferences = AppearancePreferences {
            accent_color: 0xdb_2777,
            dark_background: CODEX_DARK_BACKGROUND,
            ..AppearancePreferences::default()
        };

        let shared = preferences.share_code().expect("theme should serialize");
        assert!(shared.contains("#db2777"));
        assert_eq!(
            AppearancePreferences::from_share_code(&shared).expect("theme should deserialize"),
            preferences
        );
    }
}
