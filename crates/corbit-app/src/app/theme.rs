use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

use gpui::{App, Hsla, Pixels, Rgba, Window, px as gpui_px, rgb as gpui_rgb};
use gpui_component::{Theme, ThemeMode};

use super::appearance::{
    AppearancePreferences, CODEX_DARK_BACKGROUND, CodeFont, ColorScheme, ContrastLevel,
    InterfaceFont,
};

// Base geometry and light-mode tokens used by the Codex-inspired desktop shell.
// The sidebar measurements are taken from the Codex desktop navigation rail at
// the default 100% interface scale.
pub(super) const SIDEBAR_DEFAULT_WIDTH: f32 = 272.;
pub(super) const SIDEBAR_MIN_WIDTH: f32 = 200.;
pub(super) const SIDEBAR_MAX_WIDTH: f32 = 520.;
pub(super) const TOOLBAR_HEIGHT: f32 = 46.;
pub(super) const PANE_TOOLBAR_HEIGHT: f32 = 40.;
pub(super) const NAV_ROW_HEIGHT: f32 = 30.;
pub(super) const SIDEBAR_FONT_SIZE: f32 = 14.;
pub(super) const FONT_SIZE_XS: f32 = 12.;
pub(super) const FONT_SIZE_SM: f32 = 13.;
pub(super) const FONT_SIZE_BASE: f32 = 15.;
pub(super) const FONT_SIZE_MONO: f32 = 14.;
pub(super) const FONT_SIZE_HEADING: f32 = 21.;
pub(super) const FONT_WEIGHT_BASE: f32 = 430.;
#[cfg(target_os = "macos")]
const SYSTEM_MONO_FONT_FAMILY: &str = "Menlo";
#[cfg(target_os = "windows")]
const SYSTEM_MONO_FONT_FAMILY: &str = "Consolas";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SYSTEM_MONO_FONT_FAMILY: &str = "DejaVu Sans Mono";
#[cfg(target_os = "macos")]
const CLASSIC_MONO_FONT_FAMILY: &str = "Monaco";
#[cfg(target_os = "windows")]
const CLASSIC_MONO_FONT_FAMILY: &str = "Courier New";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CLASSIC_MONO_FONT_FAMILY: &str = "Liberation Mono";
#[cfg(target_os = "macos")]
const SANS_FONT_FAMILY: &str = "Avenir Next";
#[cfg(target_os = "windows")]
const SANS_FONT_FAMILY: &str = "Segoe UI";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SANS_FONT_FAMILY: &str = "DejaVu Sans";
#[cfg(target_os = "macos")]
pub(super) const TITLEBAR_LEFT_PADDING: f32 = 80.;
#[cfg(not(target_os = "macos"))]
pub(super) const TITLEBAR_LEFT_PADDING: f32 = 12.;

pub(super) const COLOR_SURFACE: u32 = 0xff_ffff;
pub(super) const COLOR_SURFACE_UNDER: u32 = 0xf9_f9f9;
pub(super) const COLOR_SURFACE_SECONDARY: u32 = 0xf3_f3f3;
pub(super) const COLOR_EDITOR: u32 = 0xf8_f8f8;
pub(super) const COLOR_TEXT: u32 = 0x1a_1c1f;
pub(super) const COLOR_TEXT_SECONDARY: u32 = 0x5f_6062;
pub(super) const COLOR_TEXT_TERTIARY: u32 = 0x8c_8d8f;
pub(super) const COLOR_BORDER_LIGHT: u32 = 0xf2_f2f2;
pub(super) const COLOR_BORDER: u32 = 0xed_eded;
pub(super) const COLOR_BORDER_HEAVY: u32 = 0xe3_e3e3;
pub(super) const COLOR_ACCENT_BLUE: u32 = 0x33_9cff;
pub(super) const COLOR_SUCCESS: u32 = 0x00_a240;
pub(super) const COLOR_WARNING: u32 = 0xe2_5507;
pub(super) const COLOR_ERROR: u32 = 0xe0_2e2a;

const DARK_SUCCESS: u32 = 0x3f_b9_50;
const DARK_WARNING: u32 = 0xf0_883e;
const DARK_ERROR: u32 = 0xf8_5149;
const CODEX_DARK_SURFACE_UNDER: u32 = 0x00_0000;
const CODEX_DARK_EDITOR: u32 = 0x21_2121;
const CODEX_DARK_SIDEBAR: u32 = 0x2c_2c2e;
const CODEX_DARK_SIDEBAR_OPACITY: f32 = 0.70;
const CODEX_DARK_SIDEBAR_BORDER: u32 = 0x24_2424;
const CODEX_DARK_SIDEBAR_ROW_HOVER: u32 = 0x3a_3b3d;
const CODEX_DARK_SIDEBAR_ROW_ACTIVE: u32 = 0x42_4244;
const CODEX_DARK_POPOVER: u32 = 0x2b_2b2c;
const CODEX_SELECTION_BLUE: u32 = 0x06_6dca;

static DARK_MODE: AtomicBool = AtomicBool::new(false);
static ACCENT_COLOR: AtomicU32 = AtomicU32::new(COLOR_ACCENT_BLUE);
static LIGHT_BACKGROUND: AtomicU32 = AtomicU32::new(COLOR_SURFACE);
static LIGHT_FOREGROUND: AtomicU32 = AtomicU32::new(COLOR_TEXT);
static DARK_BACKGROUND: AtomicU32 = AtomicU32::new(CODEX_DARK_BACKGROUND);
static DARK_FOREGROUND: AtomicU32 = AtomicU32::new(0xf5_f5f5);
static CONTRAST_LEVEL: AtomicU8 = AtomicU8::new(1);
static TRANSLUCENT_SIDEBAR: AtomicBool = AtomicBool::new(true);
static INTERFACE_FONT_STYLE: AtomicU8 = AtomicU8::new(0);
static CODE_FONT_STYLE: AtomicU8 = AtomicU8::new(0);
static INTERFACE_SCALE_PERCENT: AtomicU8 = AtomicU8::new(100);
static CODE_FONT_SIZE: AtomicU8 = AtomicU8::new(14);
static CONTENT_WIDTH_PIXELS: AtomicU16 = AtomicU16::new(768);

pub(super) fn is_dark_mode() -> bool {
    DARK_MODE.load(Ordering::Relaxed)
}

/// Resolves a semantic light token to the active palette.
///
/// Screens keep a single vocabulary (`COLOR_SURFACE`, `COLOR_TEXT`, and so on),
/// while this boundary guarantees that custom GPUI views and component widgets
/// switch palettes together.
pub(super) fn rgb(hex: u32) -> Rgba {
    let is_dark = DARK_MODE.load(Ordering::Relaxed);
    let background = if is_dark {
        DARK_BACKGROUND.load(Ordering::Relaxed)
    } else {
        LIGHT_BACKGROUND.load(Ordering::Relaxed)
    };
    let foreground = if is_dark {
        DARK_FOREGROUND.load(Ordering::Relaxed)
    } else {
        LIGHT_FOREGROUND.load(Ordering::Relaxed)
    };
    let resolved = resolve_color(hex, is_dark, background, foreground);
    gpui_rgb(resolved)
}

fn resolve_color(hex: u32, is_dark: bool, background: u32, foreground: u32) -> u32 {
    let contrast = CONTRAST_LEVEL.load(Ordering::Relaxed);
    let (border_light, border, border_heavy, secondary_text, tertiary_text) =
        contrast_factors(is_dark, contrast);
    match hex {
        COLOR_SURFACE => background,
        COLOR_SURFACE_UNDER => {
            if is_dark {
                if background == CODEX_DARK_BACKGROUND {
                    CODEX_DARK_SURFACE_UNDER
                } else {
                    blend_hex(background, 0x00_0000, 280)
                }
            } else {
                blend_hex(background, foreground, 25)
            }
        }
        COLOR_SURFACE_SECONDARY => blend_hex(background, foreground, 47),
        COLOR_EDITOR => {
            if is_dark {
                if background == CODEX_DARK_BACKGROUND {
                    CODEX_DARK_EDITOR
                } else {
                    blend_hex(background, foreground, 40)
                }
            } else {
                blend_hex(background, foreground, 30)
            }
        }
        COLOR_TEXT => foreground,
        COLOR_TEXT_SECONDARY => blend_hex(foreground, background, secondary_text),
        COLOR_TEXT_TERTIARY => blend_hex(foreground, background, tertiary_text),
        COLOR_BORDER_LIGHT => blend_hex(background, foreground, border_light),
        COLOR_BORDER => blend_hex(background, foreground, border),
        COLOR_BORDER_HEAVY => blend_hex(background, foreground, border_heavy),
        COLOR_ACCENT_BLUE => ACCENT_COLOR.load(Ordering::Relaxed),
        COLOR_SUCCESS if is_dark => DARK_SUCCESS,
        COLOR_WARNING if is_dark => DARK_WARNING,
        COLOR_ERROR if is_dark => DARK_ERROR,
        _ => hex,
    }
}

fn contrast_factors(is_dark: bool, contrast: u8) -> (u16, u16, u16, u16, u16) {
    match (is_dark, contrast) {
        (false, 0) => (35, 55, 85, 380, 620),
        (false, 2) => (80, 120, 180, 220, 430),
        (false, _) => (50, 80, 120, 300, 520),
        (true, 0) => (25, 55, 100, 380, 620),
        (true, 2) => (70, 130, 240, 220, 430),
        (true, _) => (40, 80, 160, 300, 520),
    }
}

/// Blends two RGB colors using a per-mille overlay amount (`0..=1000`).
pub(super) fn blend_hex(base: u32, overlay: u32, amount: u16) -> u32 {
    let amount = u32::from(amount.min(1_000));
    let blend_channel = |shift: u32| {
        let base = (base >> shift) & 0xff;
        let overlay = (overlay >> shift) & 0xff;
        ((base * (1_000 - amount) + overlay * amount + 500) / 1_000) << shift
    };
    blend_channel(16) | blend_channel(8) | blend_channel(0)
}

pub(super) fn fixed_rgb(hex: u32) -> Rgba {
    gpui_rgb(hex)
}

pub(super) fn theme_color_hex(color: Hsla) -> u32 {
    u32::from(Rgba::from(color)) >> 8
}

pub(super) fn interface_font_family() -> &'static str {
    if INTERFACE_FONT_STYLE.load(Ordering::Relaxed) == 1 {
        SANS_FONT_FAMILY
    } else {
        ".SystemUIFont"
    }
}

pub(super) fn mono_font_family() -> &'static str {
    if CODE_FONT_STYLE.load(Ordering::Relaxed) == 1 {
        CLASSIC_MONO_FONT_FAMILY
    } else {
        SYSTEM_MONO_FONT_FAMILY
    }
}

pub(super) fn sidebar_rgb() -> Rgba {
    let is_dark = DARK_MODE.load(Ordering::Relaxed);
    let translucent = TRANSLUCENT_SIDEBAR.load(Ordering::Relaxed);
    let background = DARK_BACKGROUND.load(Ordering::Relaxed);

    // Tint the native blur with Codex's cold neutral instead of covering it
    // with an opaque fill. The remaining backdrop contribution creates the
    // subtle position-dependent material variation visible in Codex.
    if is_dark && background == CODEX_DARK_BACKGROUND {
        let mut color = gpui_rgb(CODEX_DARK_SIDEBAR);
        if translucent {
            color.a = CODEX_DARK_SIDEBAR_OPACITY;
        }
        return color;
    }

    // Codex keeps the dark navigation rail visibly lighter than its content
    // surface. The translucent variant starts from a brighter source color so
    // the native material still resolves to the same visual hierarchy.
    let mut color = if is_dark {
        gpui_rgb(sidebar_surface_hex(
            background,
            DARK_FOREGROUND.load(Ordering::Relaxed),
            translucent,
        ))
    } else {
        rgb(COLOR_EDITOR)
    };
    if translucent {
        color.a = 0.70;
    }
    color
}

pub(super) fn sidebar_border_rgb() -> Rgba {
    if DARK_MODE.load(Ordering::Relaxed)
        && DARK_BACKGROUND.load(Ordering::Relaxed) == CODEX_DARK_BACKGROUND
    {
        gpui_rgb(CODEX_DARK_SIDEBAR_BORDER)
    } else {
        rgb(COLOR_BORDER)
    }
}

pub(super) fn sidebar_row_hover_rgb() -> Rgba {
    sidebar_interaction_rgb(CODEX_DARK_SIDEBAR_ROW_HOVER, 170)
}

pub(super) fn sidebar_row_active_rgb() -> Rgba {
    sidebar_interaction_rgb(CODEX_DARK_SIDEBAR_ROW_ACTIVE, 210)
}

fn sidebar_interaction_rgb(codex_default: u32, custom_dark_blend: u16) -> Rgba {
    let is_dark = DARK_MODE.load(Ordering::Relaxed);
    let background = DARK_BACKGROUND.load(Ordering::Relaxed);
    if is_dark && background == CODEX_DARK_BACKGROUND {
        return gpui_rgb(codex_default);
    }
    if is_dark {
        return gpui_rgb(blend_hex(
            background,
            DARK_FOREGROUND.load(Ordering::Relaxed),
            custom_dark_blend,
        ));
    }
    rgb(COLOR_BORDER)
}

fn sidebar_surface_hex(background: u32, foreground: u32, translucent: bool) -> u32 {
    blend_hex(background, foreground, if translucent { 230 } else { 130 })
}

fn sidebar_accent_hex(background: u32, foreground: u32) -> u32 {
    blend_hex(background, foreground, 230)
}

fn selection_background_hex(background: u32, is_dark: bool) -> u32 {
    blend_hex(
        background,
        CODEX_SELECTION_BLUE,
        if is_dark { 300 } else { 220 },
    )
}

pub(super) fn shell_background() -> Rgba {
    let mut color = rgb(COLOR_SURFACE_UNDER);
    if TRANSLUCENT_SIDEBAR.load(Ordering::Relaxed) {
        color.a = 0.;
    }
    color
}

pub(super) fn font_px(base_size: f32) -> Pixels {
    if (base_size - FONT_SIZE_MONO).abs() < f32::EPSILON {
        return gpui_px(f32::from(CODE_FONT_SIZE.load(Ordering::Relaxed)));
    }
    let scale = f32::from(INTERFACE_SCALE_PERCENT.load(Ordering::Relaxed)) / 100.;
    gpui_px(base_size * scale)
}

pub(super) fn navigation_row_height() -> Pixels {
    let scale = f32::from(INTERFACE_SCALE_PERCENT.load(Ordering::Relaxed)) / 100.;
    gpui_px(NAV_ROW_HEIGHT * scale.max(1.))
}

pub(super) fn content_max_width() -> Pixels {
    gpui_px(f32::from(CONTENT_WIDTH_PIXELS.load(Ordering::Relaxed)))
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn store_appearance_preferences(preferences: AppearancePreferences) {
    ACCENT_COLOR.store(preferences.accent_color, Ordering::Relaxed);
    LIGHT_BACKGROUND.store(preferences.light_background, Ordering::Relaxed);
    LIGHT_FOREGROUND.store(preferences.light_foreground, Ordering::Relaxed);
    DARK_BACKGROUND.store(preferences.dark_background, Ordering::Relaxed);
    DARK_FOREGROUND.store(preferences.dark_foreground, Ordering::Relaxed);
    CONTRAST_LEVEL.store(
        match preferences.contrast {
            ContrastLevel::Soft => 0,
            ContrastLevel::Default => 1,
            ContrastLevel::Strong => 2,
        },
        Ordering::Relaxed,
    );
    TRANSLUCENT_SIDEBAR.store(preferences.translucent_sidebar, Ordering::Relaxed);
    INTERFACE_FONT_STYLE.store(
        u8::from(preferences.interface_font == InterfaceFont::Sans),
        Ordering::Relaxed,
    );
    CODE_FONT_STYLE.store(
        u8::from(preferences.code_font == CodeFont::Classic),
        Ordering::Relaxed,
    );
    INTERFACE_SCALE_PERCENT.store(
        preferences.interface_text_size.scale_percent(),
        Ordering::Relaxed,
    );
    CODE_FONT_SIZE.store(preferences.code_text_size.pixels(), Ordering::Relaxed);
    CONTENT_WIDTH_PIXELS.store(preferences.content_width.pixels(), Ordering::Relaxed);
}

fn configure_component_theme(theme: &mut Theme, preferences: AppearancePreferences, is_dark: bool) {
    theme.font_family = interface_font_family().into();
    theme.font_size = font_px(FONT_SIZE_BASE);
    theme.mono_font_family = mono_font_family().into();
    theme.mono_font_size = font_px(FONT_SIZE_MONO);
    theme.radius = gpui_px(6.);
    theme.radius_lg = gpui_px(10.);
    theme.shadow = true;

    let colors = &mut theme.colors;
    colors.background = color(COLOR_SURFACE);
    colors.foreground = color(COLOR_TEXT);
    colors.border = color(COLOR_BORDER);
    colors.input = color(COLOR_BORDER_HEAVY);
    colors.caret = color(COLOR_TEXT);
    colors.ring = color(COLOR_ACCENT_BLUE);
    colors.selection = gpui_rgb(selection_background_hex(
        if is_dark {
            preferences.dark_background
        } else {
            preferences.light_background
        },
        is_dark,
    ))
    .into();

    colors.primary = color(COLOR_TEXT);
    colors.primary_foreground = color(COLOR_SURFACE);
    colors.primary_hover = color(if is_dark { 0xe5_e5e5 } else { 0x30_3030 });
    colors.primary_active = color(if is_dark { 0xd5_d5d5 } else { 0x41_4141 });
    colors.secondary = color(COLOR_SURFACE_SECONDARY);
    colors.secondary_foreground = color(COLOR_TEXT);
    colors.secondary_hover = color(COLOR_BORDER);
    colors.secondary_active = if is_dark {
        gpui_rgb(sidebar_accent_hex(
            preferences.dark_background,
            preferences.dark_foreground,
        ))
        .into()
    } else {
        color(COLOR_BORDER_HEAVY)
    };
    colors.accent = if is_dark {
        sidebar_row_hover_rgb().into()
    } else {
        color(COLOR_SURFACE_SECONDARY)
    };
    colors.accent_foreground = color(COLOR_TEXT);
    colors.muted = color(COLOR_SURFACE_SECONDARY);
    colors.muted_foreground = color(COLOR_TEXT_TERTIARY);

    colors.sidebar = sidebar_rgb().into();
    colors.sidebar_foreground = color(COLOR_TEXT);
    colors.sidebar_border = color(COLOR_BORDER);
    colors.sidebar_accent = if is_dark {
        gpui_rgb(sidebar_accent_hex(
            preferences.dark_background,
            preferences.dark_foreground,
        ))
        .into()
    } else {
        color(COLOR_BORDER)
    };
    colors.sidebar_accent_foreground = color(COLOR_TEXT);
    colors.sidebar_primary = color(COLOR_TEXT);
    colors.sidebar_primary_foreground = color(COLOR_SURFACE);

    colors.popover = if is_dark && preferences.dark_background == CODEX_DARK_BACKGROUND {
        gpui_rgb(CODEX_DARK_POPOVER).into()
    } else {
        color(COLOR_SURFACE)
    };
    colors.popover_foreground = color(COLOR_TEXT);
    colors.list = color(COLOR_SURFACE);
    colors.list_hover = if is_dark {
        sidebar_row_hover_rgb().into()
    } else {
        color(COLOR_SURFACE_SECONDARY)
    };
    colors.list_active = color(COLOR_BORDER);
    colors.list_active_border = color(COLOR_BORDER_HEAVY);
    colors.tab = color(COLOR_SURFACE);
    colors.tab_bar = color(COLOR_SURFACE);
    colors.tab_bar_segmented = color(COLOR_SURFACE_SECONDARY);
    colors.tab_active = color(COLOR_SURFACE);
    colors.tab_foreground = color(COLOR_TEXT_SECONDARY);
    colors.tab_active_foreground = color(COLOR_TEXT);

    colors.title_bar = color(COLOR_SURFACE_UNDER);
    colors.title_bar_border = color(COLOR_BORDER_LIGHT);
    colors.link = color(COLOR_ACCENT_BLUE);
    colors.link_hover = color(if is_dark { 0x75_b8ff } else { 0x0b_7fe5 });
    colors.link_active = color(if is_dark { 0x94_c8ff } else { 0x06_6fca });
    colors.success = color(COLOR_SUCCESS);
    colors.success_foreground = color(COLOR_SURFACE);
    colors.warning = color(COLOR_WARNING);
    colors.warning_foreground = color(COLOR_SURFACE);
    colors.danger = color(COLOR_ERROR);
    colors.danger_foreground = color(COLOR_SURFACE);
    colors.switch = color(COLOR_BORDER_HEAVY);
    colors.switch_thumb = color(COLOR_SURFACE);
}

pub(super) fn configure_codex_theme(
    preferences: AppearancePreferences,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    store_appearance_preferences(preferences);

    let system_appearance = window
        .as_deref()
        .map_or_else(|| cx.window_appearance(), Window::appearance);
    let mode = match preferences.color_scheme {
        ColorScheme::System => ThemeMode::from(system_appearance),
        ColorScheme::Light => ThemeMode::Light,
        ColorScheme::Dark => ThemeMode::Dark,
    };
    let is_dark = mode.is_dark();
    DARK_MODE.store(is_dark, Ordering::Relaxed);
    Theme::change(mode, None, cx);
    configure_component_theme(Theme::global_mut(cx), preferences, is_dark);

    if let Some(window) = window {
        window.set_background_appearance(if preferences.translucent_sidebar {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Transparent
        });
        window.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_blending_preserves_endpoints_and_channels() {
        assert_eq!(blend_hex(0x12_3456, 0xab_cdef, 0), 0x12_3456);
        assert_eq!(blend_hex(0x12_3456, 0xab_cdef, 1_000), 0xab_cdef);
        assert_eq!(blend_hex(0x00_0000, 0xff_ffff, 500), 0x80_8080);
    }

    #[test]
    fn default_dark_neutrals_remain_codex_like() {
        CONTRAST_LEVEL.store(1, Ordering::Relaxed);
        assert_eq!(
            resolve_color(COLOR_SURFACE, true, CODEX_DARK_BACKGROUND, 0xf5_f5f5),
            CODEX_DARK_BACKGROUND
        );
        assert_eq!(
            resolve_color(COLOR_SURFACE_UNDER, true, CODEX_DARK_BACKGROUND, 0xf5_f5f5),
            CODEX_DARK_SURFACE_UNDER
        );
        assert_eq!(
            resolve_color(COLOR_EDITOR, true, CODEX_DARK_BACKGROUND, 0xf5_f5f5),
            CODEX_DARK_EDITOR
        );
        assert_eq!(
            resolve_color(COLOR_BORDER, true, CODEX_DARK_BACKGROUND, 0xf5_f5f5),
            0x25_2525
        );
        assert_eq!(
            sidebar_surface_hex(CODEX_DARK_BACKGROUND, 0xf5_f5f5, true),
            0x47_4747
        );
        assert_eq!(
            sidebar_surface_hex(CODEX_DARK_BACKGROUND, 0xf5_f5f5, false),
            0x30_3030
        );
        assert_eq!(
            sidebar_accent_hex(CODEX_DARK_BACKGROUND, 0xf5_f5f5),
            0x47_4747
        );
        assert_eq!(CODEX_DARK_SIDEBAR, 0x2c_2c2e);
        assert!((CODEX_DARK_SIDEBAR_OPACITY - 0.70).abs() < f32::EPSILON);
        assert_eq!(CODEX_DARK_SIDEBAR_BORDER, 0x24_2424);
        assert_eq!(CODEX_DARK_SIDEBAR_ROW_HOVER, 0x3a_3b3d);
        assert_eq!(CODEX_DARK_SIDEBAR_ROW_ACTIVE, 0x42_4244);
        assert_eq!(CODEX_DARK_POPOVER, 0x2b_2b2c);
    }

    #[test]
    fn text_selection_matches_codex_without_using_the_accent_color() {
        assert_eq!(
            selection_background_hex(CODEX_DARK_BACKGROUND, true),
            0x0f_2e4a
        );
        assert_eq!(selection_background_hex(0xff_ffff, false), 0xc8_dff3);
    }
}
