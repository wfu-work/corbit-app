use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use gpui::{Bounds, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

use super::theme::{SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH};
use super::{MainSection, PendingNewTaskRecovery, ResourceSection, TaskFilter};

const MIN_WINDOW_WIDTH: f32 = 720.;
const MIN_WINDOW_HEIGHT: f32 = 520.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct PanelWidths {
    sidebar: f32,
    workspace_files_list: f32,
    workspace_changes_list: f32,
}

impl Default for PanelWidths {
    fn default() -> Self {
        Self {
            sidebar: SIDEBAR_DEFAULT_WIDTH,
            workspace_files_list: 300.,
            workspace_changes_list: 340.,
        }
    }
}

impl PanelWidths {
    pub(super) const MIN_WORKSPACE_LIST: f32 = 220.;
    pub(super) const MAX_WORKSPACE_LIST: f32 = 520.;

    fn normalized(mut self) -> Self {
        let defaults = Self::default();
        self.sidebar = Self::normalize_sidebar(self.sidebar, defaults.sidebar);
        self.workspace_files_list = Self::normalize_workspace_list(
            self.workspace_files_list,
            defaults.workspace_files_list,
        );
        self.workspace_changes_list = Self::normalize_workspace_list(
            self.workspace_changes_list,
            defaults.workspace_changes_list,
        );
        self
    }

    fn normalize_sidebar(width: f32, default: f32) -> f32 {
        if width.is_finite() {
            width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
        } else {
            default
        }
    }

    fn normalize_workspace_list(width: f32, default: f32) -> f32 {
        if width.is_finite() {
            width.clamp(Self::MIN_WORKSPACE_LIST, Self::MAX_WORKSPACE_LIST)
        } else {
            default
        }
    }

    pub(super) fn sidebar(self) -> f32 {
        self.sidebar
    }

    pub(super) fn workspace_files_list(self) -> f32 {
        self.workspace_files_list
    }

    pub(super) fn workspace_changes_list(self) -> f32 {
        self.workspace_changes_list
    }

    pub(super) fn set_sidebar(&mut self, width: f32) {
        self.sidebar = Self::normalize_sidebar(width, Self::default().sidebar);
    }

    pub(super) fn set_workspace_files_list(&mut self, width: f32) {
        self.workspace_files_list =
            Self::normalize_workspace_list(width, Self::default().workspace_files_list);
    }

    pub(super) fn set_workspace_changes_list(&mut self, width: f32) {
        self.workspace_changes_list =
            Self::normalize_workspace_list(width, Self::default().workspace_changes_list);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedWindowState {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct WindowPlacement {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: PersistedWindowState,
}

impl Default for WindowPlacement {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            width: 1120.,
            height: 760.,
            state: PersistedWindowState::Windowed,
        }
    }
}

impl WindowPlacement {
    pub(super) fn capture(window_bounds: WindowBounds) -> Self {
        let bounds = window_bounds.get_bounds();
        Self {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            state: match window_bounds {
                WindowBounds::Windowed(_) => PersistedWindowState::Windowed,
                WindowBounds::Maximized(_) => PersistedWindowState::Maximized,
                WindowBounds::Fullscreen(_) => PersistedWindowState::Fullscreen,
            },
        }
    }

    pub(super) fn window_bounds(self) -> Option<WindowBounds> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width < MIN_WINDOW_WIDTH
            || self.height < MIN_WINDOW_HEIGHT
        {
            return None;
        }
        let bounds = Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        };
        Some(match self.state {
            PersistedWindowState::Windowed => WindowBounds::Windowed(bounds),
            PersistedWindowState::Maximized => WindowBounds::Maximized(bounds),
            PersistedWindowState::Fullscreen => WindowBounds::Fullscreen(bounds),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct UiPreferences {
    pub(super) sidebar_collapsed: bool,
    pub(super) main_section: MainSection,
    pub(super) settings_return_section: MainSection,
    pub(super) resource_section: ResourceSection,
    pub(super) selected_project_id: Option<String>,
    pub(super) selected_workspace_id: Option<String>,
    pub(super) selected_agent_id: Option<String>,
    pub(super) selected_provider: String,
    pub(super) project_providers: BTreeMap<String, String>,
    pub(super) task_filter: TaskFilter,
    pub(super) new_task_draft: String,
    pub(super) prompt_drafts: BTreeMap<String, String>,
    pub(super) new_task_recovery: Option<PendingNewTaskRecovery>,
    pub(super) panel_widths: PanelWidths,
    pub(super) window: Option<WindowPlacement>,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            sidebar_collapsed: false,
            main_section: MainSection::NewTask,
            settings_return_section: MainSection::NewTask,
            resource_section: ResourceSection::General,
            selected_project_id: None,
            selected_workspace_id: None,
            selected_agent_id: None,
            selected_provider: "codex".into(),
            project_providers: BTreeMap::new(),
            task_filter: TaskFilter::All,
            new_task_draft: String::new(),
            prompt_drafts: BTreeMap::new(),
            new_task_recovery: None,
            panel_widths: PanelWidths::default(),
            window: None,
        }
    }
}

impl UiPreferences {
    pub(super) fn load() -> Self {
        preferences_path()
            .and_then(|path| Self::load_from(&path).ok())
            .unwrap_or_default()
    }

    pub(super) fn save(&self) -> anyhow::Result<()> {
        let path = preferences_path().ok_or_else(|| anyhow!("无法确定当前用户的配置目录"))?;
        self.save_to(&path)
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        let bytes =
            fs::read(path).with_context(|| format!("无法读取界面状态 {}", path.display()))?;
        let mut preferences: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("无法解析界面状态 {}", path.display()))?;
        preferences.panel_widths = preferences.panel_widths.normalized();
        Ok(preferences)
    }

    fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("界面状态路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建配置目录 {}", parent.display()))?;
        let bytes = serde_json::to_vec_pretty(self).context("无法序列化界面状态")?;
        fs::write(path, bytes).with_context(|| format!("无法写入界面状态 {}", path.display()))
    }
}

#[cfg(target_os = "macos")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Corbit")
            .join("ui-state.json")
    })
}

#[cfg(target_os = "windows")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("APPDATA").map(|directory| {
        PathBuf::from(directory)
            .join("Corbit")
            .join("ui-state.json")
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|directory| directory.join("corbit").join("ui-state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_preferences_round_trip_and_accept_partial_files() {
        let directory =
            std::env::temp_dir().join(format!("corbit-ui-state-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("ui-state.json");
        let mut prompt_drafts = BTreeMap::new();
        prompt_drafts.insert("agent-1".into(), "继续实现测试".into());
        let preferences = UiPreferences {
            sidebar_collapsed: true,
            main_section: MainSection::Conversation,
            resource_section: ResourceSection::Appearance,
            selected_agent_id: Some("agent-1".into()),
            task_filter: TaskFilter::Active,
            prompt_drafts,
            panel_widths: PanelWidths {
                sidebar: 318.,
                workspace_files_list: 376.,
                workspace_changes_list: 412.,
            },
            window: Some(WindowPlacement {
                x: 80.,
                y: 120.,
                width: 1280.,
                height: 820.,
                state: PersistedWindowState::Maximized,
            }),
            ..UiPreferences::default()
        };

        preferences.save_to(&path).expect("UI state should save");
        assert_eq!(
            UiPreferences::load_from(&path).expect("UI state should load"),
            preferences
        );

        fs::write(&path, br#"{"sidebarCollapsed":true}"#).expect("partial UI state should save");
        assert_eq!(
            UiPreferences::load_from(&path).expect("partial UI state should load"),
            UiPreferences {
                sidebar_collapsed: true,
                ..UiPreferences::default()
            }
        );

        fs::remove_dir_all(directory).expect("temporary UI state should be removable");
    }

    #[test]
    fn panel_widths_are_normalized_when_loaded() {
        let directory =
            std::env::temp_dir().join(format!("corbit-panel-width-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("ui-state.json");
        fs::create_dir_all(&directory).expect("temporary directory should be creatable");
        fs::write(
            &path,
            br#"{"panelWidths":{"sidebar":40,"workspaceFilesList":40,"workspaceChangesList":900}}"#,
        )
        .expect("UI state should save");

        let preferences = UiPreferences::load_from(&path).expect("UI state should load");
        assert!((preferences.panel_widths.sidebar() - SIDEBAR_MIN_WIDTH).abs() < f32::EPSILON);
        assert!(
            (preferences.panel_widths.workspace_files_list() - PanelWidths::MIN_WORKSPACE_LIST)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (preferences.panel_widths.workspace_changes_list() - PanelWidths::MAX_WORKSPACE_LIST)
                .abs()
                < f32::EPSILON
        );

        fs::remove_dir_all(directory).expect("temporary UI state should be removable");
    }

    #[test]
    fn invalid_window_geometry_is_ignored() {
        let placement = WindowPlacement {
            width: 200.,
            ..WindowPlacement::default()
        };
        assert!(placement.window_bounds().is_none());
    }
}
