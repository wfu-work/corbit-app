use std::borrow::Cow;

use gpui::{AnyElement, AssetSource, IntoElement, Result, SharedString, img, prelude::*, px, rgb};
use gpui_component::{Icon, IconNamed, StyledExt};

use super::theme::is_dark_mode;

pub(crate) const MARK_ASSET: &str = "brand/corbit-mark.svg";
pub(crate) const SYMBOL_LIGHT_ASSET: &str = "brand/corbit-symbol-light.svg";
pub(crate) const SYMBOL_DARK_ASSET: &str = "brand/corbit-symbol-dark.svg";
pub(crate) const CODEX_PROVIDER_ASSET: &str = "providers/codex.svg";
pub(crate) const CLAUDE_PROVIDER_ASSET: &str = "providers/claude.svg";

macro_rules! embedded_icon {
    ($name:literal) => {
        (
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/icons/",
                $name,
                ".svg"
            )) as &'static [u8],
        )
    };
}

const EMBEDDED_ASSETS: &[(&str, &[u8])] = &[
    (
        MARK_ASSET,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/brand/corbit-mark.svg"
        )),
    ),
    (
        SYMBOL_LIGHT_ASSET,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/brand/corbit-symbol-light.svg"
        )),
    ),
    (
        SYMBOL_DARK_ASSET,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/brand/corbit-symbol-dark.svg"
        )),
    ),
    (
        CODEX_PROVIDER_ASSET,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/providers/codex.svg"
        )),
    ),
    (
        CLAUDE_PROVIDER_ASSET,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/providers/claude.svg"
        )),
    ),
    embedded_icon!("activity"),
    embedded_icon!("arrow-left"),
    embedded_icon!("arrow-up"),
    embedded_icon!("blocks"),
    embedded_icon!("bot"),
    embedded_icon!("chevron-right"),
    embedded_icon!("circle-check"),
    embedded_icon!("copy"),
    embedded_icon!("external-link"),
    embedded_icon!("file"),
    embedded_icon!("folder-closed"),
    embedded_icon!("folder-open"),
    embedded_icon!("git-compare-arrows"),
    embedded_icon!("info"),
    embedded_icon!("keyboard"),
    embedded_icon!("list-todo"),
    embedded_icon!("message-square"),
    embedded_icon!("monitor-smartphone"),
    embedded_icon!("more-horizontal"),
    embedded_icon!("panel-left-close"),
    embedded_icon!("panel-left-open"),
    embedded_icon!("panels-top-left"),
    embedded_icon!("pencil"),
    embedded_icon!("play"),
    embedded_icon!("plus"),
    embedded_icon!("refresh-cw"),
    embedded_icon!("search"),
    embedded_icon!("settings"),
    embedded_icon!("shield-check"),
    embedded_icon!("square"),
    embedded_icon!("square-terminal"),
    embedded_icon!("sun"),
    embedded_icon!("trash-2"),
    embedded_icon!("user"),
    embedded_icon!("wrench"),
    embedded_icon!("x"),
];

/// Semantic icon vocabulary for Corbit's desktop interface.
///
/// Keeping product meaning here prevents individual screens from drifting to
/// unrelated glyphs and guarantees that every icon is bundled with the app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppIcon {
    Activity,
    Add,
    Agent,
    Appearance,
    Approval,
    Back,
    Changes,
    ChevronRight,
    Close,
    Conversation,
    Copy,
    Delete,
    Device,
    ExternalLink,
    File,
    Folder,
    FolderOpen,
    Info,
    More,
    PanelLeftClose,
    PanelLeftOpen,
    Play,
    Project,
    Provider,
    Rename,
    Refresh,
    Search,
    Send,
    Settings,
    Shortcuts,
    Stop,
    Success,
    Tasks,
    Terminal,
    Tool,
    User,
    Workspace,
}

impl AppIcon {
    #[cfg(test)]
    const ALL: &'static [Self] = &[
        Self::Activity,
        Self::Add,
        Self::Agent,
        Self::Appearance,
        Self::Approval,
        Self::Back,
        Self::Changes,
        Self::ChevronRight,
        Self::Close,
        Self::Conversation,
        Self::Copy,
        Self::Delete,
        Self::Device,
        Self::ExternalLink,
        Self::File,
        Self::Folder,
        Self::FolderOpen,
        Self::Info,
        Self::More,
        Self::PanelLeftClose,
        Self::PanelLeftOpen,
        Self::Play,
        Self::Project,
        Self::Provider,
        Self::Rename,
        Self::Refresh,
        Self::Search,
        Self::Send,
        Self::Settings,
        Self::Shortcuts,
        Self::Stop,
        Self::Success,
        Self::Tasks,
        Self::Terminal,
        Self::Tool,
        Self::User,
        Self::Workspace,
    ];

    const fn asset_path(self) -> &'static str {
        match self {
            Self::Activity => "icons/activity.svg",
            Self::Add => "icons/plus.svg",
            Self::Agent => "icons/bot.svg",
            Self::Appearance => "icons/sun.svg",
            Self::Approval => "icons/shield-check.svg",
            Self::Back => "icons/arrow-left.svg",
            Self::Changes => "icons/git-compare-arrows.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::Close => "icons/x.svg",
            Self::Conversation => "icons/message-square.svg",
            Self::Copy => "icons/copy.svg",
            Self::Delete => "icons/trash-2.svg",
            Self::Device => "icons/monitor-smartphone.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::File => "icons/file.svg",
            Self::Folder | Self::Project => "icons/folder-closed.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::Info => "icons/info.svg",
            Self::More => "icons/more-horizontal.svg",
            Self::PanelLeftClose => "icons/panel-left-close.svg",
            Self::PanelLeftOpen => "icons/panel-left-open.svg",
            Self::Play => "icons/play.svg",
            Self::Provider => "icons/blocks.svg",
            Self::Rename => "icons/pencil.svg",
            Self::Refresh => "icons/refresh-cw.svg",
            Self::Search => "icons/search.svg",
            Self::Send => "icons/arrow-up.svg",
            Self::Settings => "icons/settings.svg",
            Self::Shortcuts => "icons/keyboard.svg",
            Self::Stop => "icons/square.svg",
            Self::Success => "icons/circle-check.svg",
            Self::Tasks => "icons/list-todo.svg",
            Self::Terminal => "icons/square-terminal.svg",
            Self::Tool => "icons/wrench.svg",
            Self::User => "icons/user.svg",
            Self::Workspace => "icons/panels-top-left.svg",
        }
    }
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        self.asset_path().into()
    }
}

/// Embedded visual assets used by the GPUI application.
pub(crate) struct BrandAssets;

impl AssetSource for BrandAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(EMBEDDED_ASSETS
            .iter()
            .find_map(|(asset_path, bytes)| (*asset_path == path).then_some(Cow::Borrowed(*bytes))))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(EMBEDDED_ASSETS
            .iter()
            .filter(|(asset_path, _)| path.is_empty() || asset_path.starts_with(path))
            .map(|(asset_path, _)| SharedString::from(*asset_path))
            .collect())
    }
}

pub(crate) fn brand_mark(size: f32) -> impl IntoElement {
    let asset = if is_dark_mode() {
        SYMBOL_DARK_ASSET
    } else {
        SYMBOL_LIGHT_ASSET
    };
    img(asset).size(px(size))
}

#[derive(Clone, Copy)]
enum ProviderMark {
    Codex,
    Claude,
}

impl IconNamed for ProviderMark {
    fn path(self) -> SharedString {
        match self {
            Self::Codex => CODEX_PROVIDER_ASSET,
            Self::Claude => CLAUDE_PROVIDER_ASSET,
        }
        .into()
    }
}

pub(crate) fn provider_logo(provider_id: &str, size: f32) -> AnyElement {
    let dark = is_dark_mode();
    let (background, icon) = match provider_id {
        "codex" => (
            rgb(0x0010_1010),
            Icon::new(ProviderMark::Codex).text_color(rgb(0x00f7_f7f7)),
        ),
        "claude" => (
            rgb(if dark { 0x002a_211e } else { 0x00f4_eae5 }),
            Icon::new(ProviderMark::Claude).text_color(rgb(0x00d9_7757)),
        ),
        _ => (
            rgb(if dark { 0x0029_2929 } else { 0x00ed_eded }),
            Icon::new(AppIcon::Provider).text_color(rgb(if dark {
                0x00c7_c7c7
            } else {
                0x0055_5555
            })),
        ),
    };

    gpui::div()
        .flex_none()
        .h_flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size * 0.22))
        .bg(background)
        .child(icon.size(px(size * 0.58)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_mark_is_available_to_gpui() {
        let assets = BrandAssets;
        let mark = assets
            .load(MARK_ASSET)
            .expect("brand asset lookup should succeed")
            .expect("brand mark should be embedded");

        assert!(mark.starts_with(b"<?xml"));
        assert_eq!(
            assets.list("brand").expect("brand asset list should load"),
            vec![
                SharedString::from(MARK_ASSET),
                SharedString::from(SYMBOL_LIGHT_ASSET),
                SharedString::from(SYMBOL_DARK_ASSET),
            ]
        );
        assert!(
            assets
                .load("brand/missing.svg")
                .expect("missing asset lookup should succeed")
                .is_none()
        );
        for icon in AppIcon::ALL {
            let path = icon.asset_path();
            let bytes = assets
                .load(path)
                .expect("icon asset lookup should succeed")
                .unwrap_or_else(|| panic!("{path} should be embedded"));
            let svg = std::str::from_utf8(&bytes).expect("icon should be valid UTF-8 SVG");

            for expected in [
                "viewBox=\"0 0 24 24\"",
                "fill=\"none\"",
                "stroke=\"currentColor\"",
                "stroke-width=\"2\"",
                "stroke-linecap=\"round\"",
                "stroke-linejoin=\"round\"",
            ] {
                assert!(svg.contains(expected), "{path} should contain {expected}");
            }
        }
    }

    #[test]
    fn embedded_provider_marks_are_available_to_gpui() {
        let assets = BrandAssets;

        assert_eq!(
            assets
                .list("providers")
                .expect("provider asset list should load"),
            vec![
                SharedString::from(CODEX_PROVIDER_ASSET),
                SharedString::from(CLAUDE_PROVIDER_ASSET),
            ]
        );
        for path in [CODEX_PROVIDER_ASSET, CLAUDE_PROVIDER_ASSET] {
            let bytes = assets
                .load(path)
                .expect("provider asset lookup should succeed")
                .unwrap_or_else(|| panic!("{path} should be embedded"));
            let svg = std::str::from_utf8(&bytes).expect("provider mark should be UTF-8 SVG");
            assert!(svg.contains("viewBox=\"0 0 24 24\""));
            assert!(svg.contains("fill=\"currentColor\""));
        }
    }
}
