mod appearance;
mod application_icon;
mod branding;
mod build_info;
mod coding;
mod connection;
mod desktop_notifications;
mod discovery;
mod event_batch;
mod feedback;
mod integrations;
mod local_daemon;
mod plugins;
mod provider;
mod provider_catalog;
mod provider_selection;
mod resources;
mod scheduled;
mod settings;
mod settings_components;
mod sleep_prevention;
#[cfg(target_os = "macos")]
mod system_tray;
mod tasks;
mod theme;
mod timeline;
mod ui_state;
mod workspace;

use appearance::{
    AppIconMode, AppearancePreferences, CodeFont, CodeTextSize, ColorScheme, ContentWidth,
    ContrastLevel, InterfaceFont, InterfaceTextSize,
};
use branding::{APP_ICON_DARK_ASSET, APP_ICON_LIGHT_ASSET, AppIcon, BrandAssets, brand_mark};
use coding::{CodingPreferences, SshConnectionTestState, detect_git_version};
use connection::{ConnectionPreferences, CredentialSource};
use discovery::{ActivityFilter, SearchScope};
use feedback::{FeedbackKind, push_app_notification};
use integrations::{IntegrationPreferences, IntegrationProbeState};
use provider::{
    PROVIDERS, ProviderBadgeSize, ProviderInfo, model_display_name, provider_badge, provider_label,
    provider_supports_turn_options, reasoning_effort_short_label,
};
use provider_selection::ComposerSelections;
use resources::{AgentMenuData, DeleteTarget, RetryMutation};
use settings_components::{
    SettingsCard, settings_action_button, settings_card, settings_danger_action_button,
    settings_input, settings_primary_action_button, settings_quiet_action_button,
    settings_select_button, settings_select_menu, settings_switch,
};
use tasks::{PendingNewTaskRecovery, TaskFilter};
use theme::{
    COLOR_BORDER, COLOR_BORDER_HEAVY, COLOR_BORDER_LIGHT, COLOR_EDITOR, COLOR_ERROR, COLOR_SUCCESS,
    COLOR_SURFACE, COLOR_SURFACE_SECONDARY, COLOR_SURFACE_UNDER, COLOR_TEXT, COLOR_TEXT_SECONDARY,
    COLOR_TEXT_TERTIARY, COLOR_WARNING, FONT_SIZE_BASE, FONT_SIZE_HEADING, FONT_SIZE_MONO,
    FONT_SIZE_SM, FONT_SIZE_XS, FONT_WEIGHT_BASE, PANE_TOOLBAR_HEIGHT, SIDEBAR_FONT_SIZE,
    SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, TITLEBAR_LEFT_PADDING, TOOLBAR_HEIGHT, blend_hex,
    configure_codex_theme, content_max_width, fixed_rgb, font_px, interface_font_family,
    is_dark_mode, mono_font_family, navigation_row_height, rgb, shell_background,
    sidebar_border_rgb, sidebar_rgb, sidebar_row_active_rgb, sidebar_row_hover_rgb,
    theme_color_hex,
};
use timeline::{
    ComposerAttachment, ConversationIndexInteraction, PendingPermission, QueuedPrompt,
    RetryControl, RetryPrompt, TimelineIndex, TimelineStatus, TimelineTurn,
};
use ui_state::{
    AgentConfigurationPreferences, CloseWindowBehavior, FollowUpBehavior, GeneralPreferences,
    PanelWidths, PromptSubmitBehavior, StartupDestination, UiPreferences, WindowPlacement,
};
use workspace::{
    FileOperationState, FileOperationTarget, GitOperationState, WorkspaceRefreshQueue,
};

use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, Application, ClipboardItem, Context,
    Corner, Div, Entity, FontWeight, IntoElement, KeyBinding, ListAlignment, ListState, ObjectFit,
    PathPromptOptions, Render, SharedString, Subscription, Task, Timer, Window,
    WindowBackgroundAppearance, WindowOptions, canvas, div, img, list, percentage, prelude::*, px,
    rems, rgb as gpui_rgb,
};
use gpui_component::{
    Disableable, Icon, PixelsExt, Root, Selectable, Sizable, Size, StyledExt, WindowExt,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    dialog::DialogButtonProps,
    input::{Enter, Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    plot::shape::{Arc, ArcData},
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    process::Command,
    time::Duration,
};

gpui::actions!(
    corbit,
    [
        OpenNewTask,
        OpenSearch,
        OpenTasks,
        OpenScheduled,
        OpenActivity,
        OpenSettings,
        CaptureAppSnapshot,
        SubmitPrompt
    ]
);

const CONVERSATION_TITLE_MAX_WIDTH: f32 = 420.;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MainSection {
    NewTask,
    Search,
    Tasks,
    Scheduled,
    Activity,
    Permissions,
    Conversation,
    Files,
    Changes,
    Resources,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResourceSection {
    General,
    Appearance,
    Notifications,
    Configuration,
    Providers,
    ComputerControl,
    AppSnapshot,
    Plugins,
    Browser,
    Shortcuts,
    Projects,
    SshConnections,
    Git,
    Hooks,
    #[serde(rename = "workspaces")]
    Workspaces,
    #[serde(rename = "agents")]
    Agents,
    Daemon,
    Devices,
    About,
    ThirdPartyLicenses,
}

#[allow(clippy::struct_excessive_bools)]
struct ConnectionView {
    state: corbit_client::ConnectionState,
    detail: String,
    connection_generation: u64,
    local_daemon_status: local_daemon::DaemonStatus,
    feedback: Option<feedback::AppFeedback>,
    feedback_generation: u64,
    daemon_endpoint: String,
    connection_preferences: ConnectionPreferences,
    connection_endpoint: Entity<InputState>,
    connection_token: Entity<InputState>,
    endpoint_environment_override: bool,
    credential_source: Option<CredentialSource>,
    system_credential_present: bool,
    connection_settings_error: Option<String>,
    server_info: Option<corbit_client::ServerInfo>,
    provider_catalog: Option<corbit_client::ProviderCatalog>,
    provider_catalog_error: Option<String>,
    provider_catalog_request_id: u64,
    plugins: Vec<corbit_client::PluginRecord>,
    plugin_marketplace: Vec<corbit_client::PluginMarketplaceEntry>,
    codex_official_plugin_catalog: Option<corbit_client::CodexOfficialPluginCatalog>,
    codex_official_plugin_error: Option<String>,
    codex_official_plugin_search: Entity<InputState>,
    codex_official_apps_needing_auth: Vec<corbit_client::CodexOfficialPluginApp>,
    pending_codex_official_plugin_install: Option<String>,
    pending_codex_official_plugin_uninstall: Option<String>,
    pending_plugin_uninstall: Option<String>,
    pending_plugin_update: Option<String>,
    pending_plugin_inspection: Option<corbit_client::PluginInspection>,
    main_section: MainSection,
    sidebar_collapsed: bool,
    settings_return_section: MainSection,
    resource_section: ResourceSection,
    general_preferences: GeneralPreferences,
    agent_configuration: AgentConfigurationPreferences,
    coding_preferences: CodingPreferences,
    integration_preferences: IntegrationPreferences,
    computer_access_status: IntegrationProbeState,
    browser_connection_status: IntegrationProbeState,
    snapshot_capture_status: IntegrationProbeState,
    computer_allowed_application: Entity<InputState>,
    browser_connector_endpoint: Entity<InputState>,
    browser_allowed_domain: Entity<InputState>,
    appearance: AppearancePreferences,
    appearance_error: Option<String>,
    appearance_theme_code: Entity<InputState>,
    appearance_accent_color: Entity<ColorPickerState>,
    appearance_light_background: Entity<ColorPickerState>,
    appearance_light_foreground: Entity<ColorPickerState>,
    appearance_dark_background: Entity<ColorPickerState>,
    appearance_dark_foreground: Entity<ColorPickerState>,
    snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    selected_project_id: Option<String>,
    selected_workspace_id: Option<String>,
    selected_agent_id: Option<String>,
    collapsed_sidebar_projects: BTreeSet<String>,
    project_name: Entity<InputState>,
    project_root_path: Entity<InputState>,
    project_new_name: Entity<InputState>,
    workspace_name: Entity<InputState>,
    workspace_directory: Entity<InputState>,
    workspace_new_name: Entity<InputState>,
    ssh_connection_name: Entity<InputState>,
    ssh_connection_host: Entity<InputState>,
    ssh_connection_user: Entity<InputState>,
    ssh_connection_port: Entity<InputState>,
    ssh_connection_identity_file: Entity<InputState>,
    ssh_connection_tests: BTreeMap<String, SshConnectionTestState>,
    pending_ssh_connection_delete: Option<String>,
    git_branch_prefix: Entity<InputState>,
    git_monitoring_instructions: Entity<InputState>,
    git_commit_instructions: Entity<InputState>,
    git_version: Option<String>,
    selected_provider: String,
    project_providers: BTreeMap<String, String>,
    search_input: Entity<InputState>,
    search_scope: SearchScope,
    activity_filter: ActivityFilter,
    task_filter: TaskFilter,
    scheduled_filter: scheduled::ScheduledFilter,
    scheduled_tasks: Vec<corbit_client::ScheduledTask>,
    scheduled_runs: Vec<corbit_client::ScheduledRun>,
    scheduled_search: Entity<InputState>,
    scheduled_title: Entity<InputState>,
    scheduled_prompt: Entity<InputState>,
    scheduled_interval: Entity<InputState>,
    scheduled_time: Entity<InputState>,
    scheduled_agent_id: Option<String>,
    scheduled_cadence: scheduled::ScheduledCadence,
    scheduled_weekday: u8,
    scheduled_permission_mode: corbit_client::AgentPermissionMode,
    scheduled_editor_open: bool,
    scheduled_editing_task_id: Option<String>,
    scheduled_delete_confirmation: Option<String>,
    scheduled_expanded_runs: BTreeSet<String>,
    scheduled_operation_in_flight: bool,
    scheduled_request_id: u64,
    agent_title: Entity<InputState>,
    agent_new_title: Entity<InputState>,
    new_task_prompt: Entity<InputState>,
    prompt_input: Entity<InputState>,
    prompt_drafts: BTreeMap<String, String>,
    prompt_input_agent_id: Option<String>,
    prompt_clear_agent_id: Option<String>,
    composer_selections: ComposerSelections,
    composer_permission_mode: corbit_client::AgentPermissionMode,
    prompt_attachments: Vec<ComposerAttachment>,
    clipboard_image_sequence: usize,
    queued_prompts: VecDeque<QueuedPrompt>,
    attachment_in_flight: bool,
    new_task_clear_requested: bool,
    new_task_recovery: Option<PendingNewTaskRecovery>,
    new_task_cleanup_armed: bool,
    timeline_list_state: ListState,
    timeline_list_agent_id: Option<String>,
    timeline_follow_pending: bool,
    conversation_index_interaction: ConversationIndexInteraction,
    panel_widths: PanelWidths,
    window_placement: Option<WindowPlacement>,
    ui_state_error: Option<String>,
    pairing_endpoint: Entity<InputState>,
    pairing_host_name: Entity<InputState>,
    devices: Vec<corbit_client::DeviceCredentialSummary>,
    pairing_offer: Option<corbit_client::PairingOffer>,
    pending_revoke_device_id: Option<String>,
    timeline: Vec<TimelineTurn>,
    timeline_index: TimelineIndex,
    timeline_dirty_turns: BTreeSet<(String, String)>,
    expanded_timeline_steps: BTreeSet<String>,
    expanded_timeline_activity: BTreeSet<String>,
    collapsed_streaming_timeline_activity: BTreeSet<String>,
    permissions: Vec<PendingPermission>,
    workspace_listing: Option<corbit_client::WorkspaceDirectoryListing>,
    workspace_file: Option<corbit_client::WorkspaceFileContent>,
    workspace_git_status: Option<corbit_client::WorkspaceGitStatus>,
    workspace_git_diff: Option<corbit_client::WorkspaceGitDiff>,
    workspace_refresh_queue: WorkspaceRefreshQueue,
    operation_in_flight: bool,
    new_task_in_flight: bool,
    device_operation_in_flight: bool,
    prompt_in_flight: bool,
    control_in_flight: bool,
    provider_switch_in_flight: bool,
    plugin_operation_in_flight: bool,
    codex_official_plugin_operation_in_flight: bool,
    file_operation_state: FileOperationState,
    file_operation_target: Option<FileOperationTarget>,
    git_operation_state: GitOperationState,
    retry_mutation: Option<RetryMutation>,
    retry_prompt: Option<RetryPrompt>,
    retry_control: Option<RetryControl>,
    delete_confirmation: Option<DeleteTarget>,
    runtime: Option<corbit_client::DaemonRuntime>,
    daemon_preflight_task: Option<Task<()>>,
    daemon_action_task: Option<Task<()>>,
    event_task: Option<Task<()>>,
    mutation_task: Option<Task<()>>,
    prompt_task: Option<Task<()>>,
    control_task: Option<Task<()>>,
    provider_catalog_task: Option<Task<()>>,
    provider_catalog_refresh_task: Option<Task<()>>,
    provider_switch_task: Option<Task<()>>,
    plugin_task: Option<Task<()>>,
    codex_official_plugin_task: Option<Task<()>>,
    computer_access_task: Option<Task<()>>,
    browser_connection_task: Option<Task<()>>,
    app_snapshot_task: Option<Task<()>>,
    file_task: Option<Task<()>>,
    git_task: Option<Task<()>>,
    ssh_connection_task: Option<Task<()>>,
    new_task_task: Option<Task<()>>,
    scheduled_task: Option<Task<()>>,
    scheduled_refresh_task: Option<Task<()>>,
    device_task: Option<Task<()>>,
    ui_save_task: Option<Task<()>>,
    _timeline_clock_task: Option<Task<()>>,
    sleep_preventer: Option<sleep_prevention::SleepPreventer>,
    _subscriptions: Vec<Subscription>,
}

impl ConnectionView {
    #[allow(clippy::too_many_lines)]
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        appearance: AppearancePreferences,
        ui_preferences: UiPreferences,
    ) -> Self {
        let UiPreferences {
            general,
            agent_configuration,
            coding,
            integrations,
            sidebar_collapsed,
            main_section: restored_main_section,
            settings_return_section,
            resource_section,
            selected_project_id,
            selected_workspace_id,
            selected_agent_id,
            selected_provider,
            project_providers,
            composer_selections,
            composer_permission_mode,
            task_filter,
            new_task_draft,
            prompt_drafts,
            new_task_recovery,
            panel_widths,
            window: window_placement,
        } = ui_preferences;
        let git_branch_prefix_value = coding.git.branch_prefix.clone();
        let git_monitoring_instructions_value = coding.git.monitoring_instructions.clone();
        let git_commit_instructions_value = coding.git.commit_instructions.clone();
        let browser_connector_endpoint_value = integrations.browser.connector_endpoint.clone();
        let main_section = general.startup_destination.resolve(restored_main_section);
        let (new_task_draft, prompt_drafts) = if general.save_prompt_drafts {
            (new_task_draft, prompt_drafts)
        } else {
            (String::new(), BTreeMap::new())
        };
        let connection_preferences = ConnectionPreferences::load();
        let resolved_endpoint = connection_preferences.resolved_endpoint();
        let daemon_endpoint = resolved_endpoint.value;
        let connection_endpoint = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(connection::DEFAULT_DAEMON_ENDPOINT)
                .default_value(connection_preferences.endpoint.clone())
        });
        let connection_token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("输入新的 Daemon Token")
                .masked(true)
        });
        let host_name = std::env::var("USER")
            .map_or_else(|_| "Corbit Desktop".into(), |user| format!("{user} 的 Mac"));
        let prompt_input_agent_id = selected_agent_id.clone();
        let prompt_draft = selected_agent_id
            .as_ref()
            .and_then(|agent_id| prompt_drafts.get(agent_id))
            .cloned()
            .unwrap_or_default();
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索任务、工作区或项目…"));
        let codex_official_plugin_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索 Codex 官方插件…"));
        let scheduled_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索已安排任务…"));
        let new_task_prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("描述你希望 Corbit 完成的任务")
                .default_value(new_task_draft)
                .auto_grow(3, 8)
        });
        let prompt_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("向 Corbit 提问或描述任务…")
                .default_value(prompt_draft)
                .auto_grow(2, 6)
                .intercept_paste(true)
        });
        let appearance_theme_code =
            cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 Corbit 外观配置 JSON"));
        let appearance_accent_color = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(gpui_rgb(appearance.accent_color))
        });
        let appearance_light_background = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(gpui_rgb(appearance.light_background))
        });
        let appearance_light_foreground = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(gpui_rgb(appearance.light_foreground))
        });
        let appearance_dark_background = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(gpui_rgb(appearance.dark_background))
        });
        let appearance_dark_foreground = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(gpui_rgb(appearance.dark_foreground))
        });
        let mut subscriptions =
            vec![
                cx.subscribe(&search_input, |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }),
            ];
        subscriptions.push(cx.subscribe(
            &scheduled_search,
            |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            &codex_official_plugin_search,
            |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(
            cx.subscribe(
                &new_task_prompt,
                |view, _, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { secondary: false }
                        if view.general_preferences.prompt_submit_behavior
                            == PromptSubmitBehavior::Enter =>
                    {
                        view.create_new_task(cx);
                    }
                    InputEvent::Change | InputEvent::PressEnter { .. } => {
                        view.schedule_ui_state_save(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        );
        subscriptions.push(cx.subscribe_in(
            &prompt_input,
            window,
            |view, input, event: &InputEvent, window, cx| match event {
                InputEvent::Paste(clipboard) => {
                    view.paste_prompt_clipboard(input, clipboard, window, cx);
                }
                InputEvent::Change | InputEvent::PressEnter { secondary: true } => {
                    if let Some(agent_id) = view.prompt_input_agent_id.clone() {
                        let value = input.read(cx).value().to_string();
                        if value.is_empty() {
                            view.prompt_drafts.remove(&agent_id);
                        } else {
                            view.prompt_drafts.insert(agent_id, value);
                        }
                    }
                    view.schedule_ui_state_save(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { secondary: false } => {
                    if view.general_preferences.prompt_submit_behavior
                        == PromptSubmitBehavior::Enter
                    {
                        let value_with_enter = input.read(cx).value().to_string();
                        let value_before_enter = value_with_enter
                            .strip_suffix('\n')
                            .unwrap_or(&value_with_enter)
                            .to_string();
                        input.update(cx, |input, cx| {
                            input.set_value(value_before_enter, window, cx);
                        });
                        view.send_prompt(cx);
                    } else {
                        if let Some(agent_id) = view.prompt_input_agent_id.clone() {
                            view.prompt_drafts
                                .insert(agent_id, input.read(cx).value().to_string());
                        }
                        view.schedule_ui_state_save(cx);
                        cx.notify();
                    }
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.observe_window_appearance(window, |view, window, cx| {
            if view.appearance.color_scheme == ColorScheme::System {
                configure_codex_theme(view.appearance, Some(window), cx);
                if let Err(error) =
                    application_icon::apply(view.appearance.app_icon_mode, is_dark_mode())
                {
                    view.appearance_error = Some(error.to_string());
                }
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe_in(
            &appearance_accent_color,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    view.appearance.accent_color = theme_color_hex(*color);
                    view.apply_appearance_preferences(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &appearance_light_background,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    view.appearance.light_background = theme_color_hex(*color);
                    view.apply_appearance_preferences(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &appearance_light_foreground,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    view.appearance.light_foreground = theme_color_hex(*color);
                    view.apply_appearance_preferences(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &appearance_dark_background,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    view.appearance.dark_background = theme_color_hex(*color);
                    view.apply_appearance_preferences(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &appearance_dark_foreground,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    view.appearance.dark_foreground = theme_color_hex(*color);
                    view.apply_appearance_preferences(window, cx);
                }
            },
        ));
        subscriptions.push(cx.observe_window_bounds(window, |view, window, cx| {
            view.window_placement = Some(WindowPlacement::capture(window.window_bounds()));
            view.schedule_ui_state_save(cx);
        }));
        let timeline_clock_task = cx.spawn(async move |weak_view, cx| {
            loop {
                Timer::after(Duration::from_secs(1)).await;
                let Some(view) = weak_view.upgrade() else {
                    break;
                };
                if view
                    .update(cx, |view, cx| {
                        if view
                            .timeline
                            .iter()
                            .any(|turn| turn.status == TimelineStatus::InProgress)
                        {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let view = Self {
            state: corbit_client::ConnectionState::Offline,
            detail: "等待连接本机 Corbit Daemon".into(),
            connection_generation: 0,
            local_daemon_status: local_daemon::DaemonStatus::checking(),
            feedback: None,
            feedback_generation: 0,
            daemon_endpoint: daemon_endpoint.clone(),
            connection_preferences,
            connection_endpoint,
            connection_token,
            endpoint_environment_override: resolved_endpoint.environment_override,
            credential_source: None,
            system_credential_present: false,
            connection_settings_error: None,
            server_info: None,
            provider_catalog: None,
            provider_catalog_error: None,
            provider_catalog_request_id: 0,
            plugins: Vec::new(),
            plugin_marketplace: Vec::new(),
            codex_official_plugin_catalog: None,
            codex_official_plugin_error: None,
            codex_official_plugin_search,
            codex_official_apps_needing_auth: Vec::new(),
            pending_codex_official_plugin_install: None,
            pending_codex_official_plugin_uninstall: None,
            pending_plugin_uninstall: None,
            pending_plugin_update: None,
            pending_plugin_inspection: None,
            main_section,
            sidebar_collapsed,
            settings_return_section,
            resource_section,
            general_preferences: general,
            agent_configuration,
            coding_preferences: coding,
            integration_preferences: integrations,
            computer_access_status: IntegrationProbeState::NotChecked,
            browser_connection_status: IntegrationProbeState::NotChecked,
            snapshot_capture_status: IntegrationProbeState::NotChecked,
            computer_allowed_application: input_state("例如：Xcode", window, cx),
            browser_connector_endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("http://127.0.0.1:9222")
                    .default_value(browser_connector_endpoint_value)
            }),
            browser_allowed_domain: input_state("例如：github.com 或 *.example.com", window, cx),
            appearance,
            appearance_error: None,
            appearance_theme_code,
            appearance_accent_color,
            appearance_light_background,
            appearance_light_foreground,
            appearance_dark_background,
            appearance_dark_foreground,
            snapshot: None,
            selected_project_id,
            selected_workspace_id,
            selected_agent_id,
            collapsed_sidebar_projects: BTreeSet::new(),
            project_name: input_state("项目名称", window, cx),
            project_root_path: input_state("项目绝对根目录", window, cx),
            project_new_name: input_state("项目新名称", window, cx),
            workspace_name: input_state("工作区名称", window, cx),
            workspace_directory: input_state("工作区绝对工作目录", window, cx),
            workspace_new_name: input_state("工作区新名称", window, cx),
            ssh_connection_name: input_state("例如：开发服务器", window, cx),
            ssh_connection_host: input_state("主机名、IP 或 SSH Host 别名", window, cx),
            ssh_connection_user: input_state("可选用户名", window, cx),
            ssh_connection_port: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("22")
                    .default_value("22")
            }),
            ssh_connection_identity_file: input_state("可选身份文件路径", window, cx),
            ssh_connection_tests: BTreeMap::new(),
            pending_ssh_connection_delete: None,
            git_branch_prefix: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("corbit/")
                    .default_value(git_branch_prefix_value)
            }),
            git_monitoring_instructions: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("例如：检查通过后再合并，并忽略不相关的变更…")
                    .default_value(git_monitoring_instructions_value)
                    .auto_grow(3, 8)
            }),
            git_commit_instructions: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("添加提交信息指引…")
                    .default_value(git_commit_instructions_value)
                    .auto_grow(3, 8)
            }),
            git_version: detect_git_version(),
            selected_provider,
            project_providers,
            search_input,
            search_scope: SearchScope::All,
            activity_filter: ActivityFilter::All,
            task_filter,
            scheduled_filter: scheduled::ScheduledFilter::All,
            scheduled_tasks: Vec::new(),
            scheduled_runs: Vec::new(),
            scheduled_search,
            scheduled_title: input_state("计划名称", window, cx),
            scheduled_prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("描述每次运行要完成的工作…")
                    .auto_grow(3, 8)
            }),
            scheduled_interval: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("分钟")
                    .default_value("60")
            }),
            scheduled_time: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("09:00")
                    .default_value("09:00")
            }),
            scheduled_agent_id: None,
            scheduled_cadence: scheduled::ScheduledCadence::Daily,
            scheduled_weekday: 1,
            scheduled_permission_mode: corbit_client::AgentPermissionMode::ReadOnly,
            scheduled_editor_open: false,
            scheduled_editing_task_id: None,
            scheduled_delete_confirmation: None,
            scheduled_expanded_runs: BTreeSet::new(),
            scheduled_operation_in_flight: false,
            scheduled_request_id: 0,
            agent_title: input_state("Agent 标题", window, cx),
            agent_new_title: input_state("Agent 新标题", window, cx),
            new_task_prompt,
            prompt_input,
            prompt_drafts,
            prompt_input_agent_id,
            prompt_clear_agent_id: None,
            composer_selections,
            composer_permission_mode,
            prompt_attachments: Vec::new(),
            clipboard_image_sequence: 0,
            queued_prompts: VecDeque::new(),
            attachment_in_flight: false,
            new_task_clear_requested: false,
            new_task_recovery,
            new_task_cleanup_armed: false,
            timeline_list_state: ListState::new(0, ListAlignment::Bottom, px(600.)),
            timeline_list_agent_id: None,
            timeline_follow_pending: false,
            conversation_index_interaction: ConversationIndexInteraction::default(),
            panel_widths,
            window_placement,
            ui_state_error: None,
            pairing_endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("手机可访问的 Daemon 地址")
                    .default_value(daemon_endpoint)
            }),
            pairing_host_name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("这台主机的名称")
                    .default_value(host_name)
            }),
            devices: Vec::new(),
            pairing_offer: None,
            pending_revoke_device_id: None,
            timeline: Vec::new(),
            timeline_index: TimelineIndex::default(),
            timeline_dirty_turns: BTreeSet::new(),
            expanded_timeline_steps: BTreeSet::new(),
            expanded_timeline_activity: BTreeSet::new(),
            collapsed_streaming_timeline_activity: BTreeSet::new(),
            permissions: Vec::new(),
            workspace_listing: None,
            workspace_file: None,
            workspace_git_status: None,
            workspace_git_diff: None,
            workspace_refresh_queue: WorkspaceRefreshQueue::default(),
            operation_in_flight: false,
            new_task_in_flight: false,
            device_operation_in_flight: false,
            prompt_in_flight: false,
            control_in_flight: false,
            provider_switch_in_flight: false,
            plugin_operation_in_flight: false,
            codex_official_plugin_operation_in_flight: false,
            file_operation_state: FileOperationState::Idle,
            file_operation_target: None,
            git_operation_state: GitOperationState::Idle,
            retry_mutation: None,
            retry_prompt: None,
            retry_control: None,
            delete_confirmation: None,
            runtime: None,
            daemon_preflight_task: None,
            daemon_action_task: None,
            event_task: None,
            mutation_task: None,
            prompt_task: None,
            control_task: None,
            provider_catalog_task: None,
            provider_catalog_refresh_task: None,
            provider_switch_task: None,
            plugin_task: None,
            codex_official_plugin_task: None,
            computer_access_task: None,
            browser_connection_task: None,
            app_snapshot_task: None,
            file_task: None,
            git_task: None,
            ssh_connection_task: None,
            new_task_task: None,
            scheduled_task: None,
            scheduled_refresh_task: None,
            device_task: None,
            ui_save_task: None,
            _timeline_clock_task: Some(timeline_clock_task),
            sleep_preventer: None,
            _subscriptions: subscriptions,
        };
        cx.defer_in(window, |view, _, cx| view.connect(cx));
        view
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if self.daemon_preflight_task.is_some() {
            return;
        }
        self.connection_generation = self.connection_generation.wrapping_add(1);
        let generation = self.connection_generation;
        self.daemon_preflight_task = None;
        self.event_task = None;
        self.clear_connection_bound_state();
        self.runtime = None;
        self.state = corbit_client::ConnectionState::Connecting;
        self.detail = "正在检查本机 Corbit Daemon".into();
        self.local_daemon_status = local_daemon::DaemonStatus::checking();
        self.clear_timeline();
        self.permissions.clear();
        self.retry_mutation = None;
        self.retry_prompt = None;
        self.retry_control = None;
        self.delete_confirmation = None;

        let endpoint = self.daemon_endpoint.clone();
        self.daemon_preflight_task = Some(cx.spawn(async move |view, cx| {
            let result = local_daemon::ensure_available(endpoint.clone()).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.connection_generation != generation {
                    return;
                }
                view.daemon_preflight_task = None;
                match result {
                    Ok(local_daemon::EnsureResult { outcome, status }) => {
                        view.detail = match outcome {
                            local_daemon::EnsureOutcome::NotManaged => {
                                "正在读取 Daemon 连接配置".into()
                            }
                            local_daemon::EnsureOutcome::AlreadyRunning => {
                                "本机 Daemon 已运行，正在读取连接配置".into()
                            }
                            local_daemon::EnsureOutcome::Started => {
                                let version = status.version.as_deref().unwrap_or("未知版本");
                                let node = status.node.as_ref().map_or_else(
                                    || "未知 Node".into(),
                                    |path| path.display().to_string(),
                                );
                                format!("已启动本机 Daemon {version}（{node}），正在读取连接配置")
                            }
                        };
                        view.local_daemon_status = status;
                        view.connect_after_daemon_preflight(endpoint, cx);
                    }
                    Err(error) => {
                        view.state = corbit_client::ConnectionState::Offline;
                        view.local_daemon_status = local_daemon::DaemonStatus::failed(&error);
                        view.show_error(format!("无法准备本机 Corbit Daemon：{error:#}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn connect_after_daemon_preflight(&mut self, endpoint: String, cx: &mut Context<Self>) {
        let credential = connection::resolve_credentials(&endpoint);
        self.credential_source = credential.source;
        self.system_credential_present = credential.system_credential_present;
        self.connection_settings_error.clone_from(&credential.error);
        let Some(token) = credential.token else {
            self.state = corbit_client::ConnectionState::Offline;
            let message = credential.error.map_or_else(
                || {
                    if connection::is_loopback_endpoint(&endpoint) {
                        "未找到本机 Daemon 凭据；请确认 Daemon 已启动，或在设置 > Daemon 中手动填写 Token".into()
                    } else {
                        "未配置 Daemon Token；请在设置 > Daemon 中保存凭证".into()
                    }
                },
                |error| format!("无法读取 Daemon 凭证：{error}"),
            );
            self.show_error(message, cx);
            return;
        };
        let config = match corbit_client::ClientConfig::desktop(endpoint, token) {
            Ok(config) => config,
            Err(error) => {
                self.state = corbit_client::ConnectionState::Offline;
                self.show_error(format!("连接配置无效：{error}"), cx);
                return;
            }
        };
        let runtime = match corbit_client::DaemonRuntime::spawn(config) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.state = corbit_client::ConnectionState::Offline;
                self.show_error(format!("无法启动网络运行时：{error}"), cx);
                return;
            }
        };
        let events = runtime.events();
        self.runtime = Some(runtime);
        self.detail = if self.credential_source == Some(CredentialSource::LocalDaemon) {
            "已自动发现本机 Daemon，正在检查服务状态".into()
        } else {
            "正在检查 Daemon 健康状态".into()
        };
        self.event_task = Some(cx.spawn(async move |view, cx| {
            let mut pending_event = None;
            loop {
                let event = if let Some(event) = pending_event.take() {
                    event
                } else {
                    let Ok(event) = events.recv().await else {
                        break;
                    };
                    event
                };
                if event_batch::is_streaming_timeline_delta(&event) {
                    Timer::after(event_batch::STREAMING_BATCH_INTERVAL).await;
                }
                let (batch, pending) =
                    event_batch::collect_runtime_event_batch(event, || events.try_recv().ok());
                pending_event = pending;
                let Some(view) = view.upgrade() else {
                    break;
                };
                if view
                    .update(cx, |view, cx| {
                        for event in batch {
                            view.apply_event(event, cx);
                        }
                        view.flush_timeline_list_updates();
                        if view.timeline_follow_pending {
                            view.timeline_follow_pending = false;
                            view.scroll_selected_timeline_to_latest();
                        }
                        if let Err(error) = view.sync_sleep_prevention() {
                            view.general_preferences.prevent_sleep_while_running = false;
                            view.show_error(error.to_string(), cx);
                            view.schedule_ui_state_save(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn clear_connection_bound_state(&mut self) {
        self.snapshot = None;
        self.server_info = None;
        self.provider_catalog = None;
        self.provider_catalog_error = None;
        self.clear_workspace_files();
        self.clear_workspace_git();
        self.operation_in_flight = false;
        self.prompt_in_flight = false;
        self.control_in_flight = false;
        self.provider_switch_in_flight = false;
        self.plugin_operation_in_flight = false;
        self.codex_official_plugin_operation_in_flight = false;
        self.new_task_in_flight = false;
        self.device_operation_in_flight = false;
        self.mutation_task = None;
        self.prompt_task = None;
        self.control_task = None;
        self.provider_catalog_task = None;
        self.provider_catalog_request_id = self.provider_catalog_request_id.wrapping_add(1);
        self.provider_catalog_refresh_task = None;
        self.provider_switch_task = None;
        self.plugin_task = None;
        self.codex_official_plugin_task = None;
        self.new_task_task = None;
        self.scheduled_task = None;
        self.scheduled_refresh_task = None;
        self.scheduled_operation_in_flight = false;
        self.scheduled_request_id = self.scheduled_request_id.wrapping_add(1);
        self.scheduled_tasks.clear();
        self.scheduled_runs.clear();
        self.device_task = None;
        self.devices.clear();
        self.plugins.clear();
        self.plugin_marketplace.clear();
        self.codex_official_plugin_catalog = None;
        self.codex_official_plugin_error = None;
        self.codex_official_apps_needing_auth.clear();
        self.pending_codex_official_plugin_install = None;
        self.pending_codex_official_plugin_uninstall = None;
        self.pending_plugin_uninstall = None;
        self.pending_plugin_update = None;
        self.pending_plugin_inspection = None;
        self.pairing_offer = None;
        self.pending_revoke_device_id = None;
        self.delete_confirmation = None;
    }

    fn apply_connection_state(
        &mut self,
        state: corbit_client::ConnectionState,
        cx: &mut Context<Self>,
    ) {
        let is_online = matches!(&state, corbit_client::ConnectionState::Online);
        if !matches!(state, corbit_client::ConnectionState::Online) {
            self.clear_connection_bound_state();
        }
        self.detail = match &state {
            corbit_client::ConnectionState::Offline => "连接已关闭".into(),
            corbit_client::ConnectionState::Connecting => "正在建立 WebSocket".into(),
            corbit_client::ConnectionState::Authenticating => "正在认证并协商协议".into(),
            corbit_client::ConnectionState::Online => "Daemon 已连接，正在同步权威状态".into(),
            corbit_client::ConnectionState::Reconnecting {
                attempt,
                delay_ms,
                reason,
            } => format!("连接中断：{reason}；第 {attempt} 次重连将在 {delay_ms}ms 后开始"),
            corbit_client::ConnectionState::AuthenticationFailed => {
                if self.credential_source == Some(CredentialSource::LocalDaemon) {
                    "已发现本机 Daemon，但自动读取的凭据被拒绝；如果启动时设置了 CORBIT_AUTH_TOKEN，请在设置 > Daemon 中手动填写同一 Token".into()
                } else {
                    "Bearer Token 无效或已被撤销".into()
                }
            }
            corbit_client::ConnectionState::Incompatible { expected, actual } => {
                format!("协议不兼容：桌面端需要 {expected}，Daemon 提供 {actual}")
            }
        };
        match &state {
            corbit_client::ConnectionState::Online => {
                self.show_success("Daemon 已连接，正在同步权威状态", cx);
            }
            corbit_client::ConnectionState::AuthenticationFailed
            | corbit_client::ConnectionState::Incompatible { .. } => {
                self.show_error(self.detail.clone(), cx);
            }
            _ => {}
        }
        self.state = state;
        if is_online {
            self.start_provider_catalog_if_ready(cx);
        }
    }

    fn apply_connection_error(&mut self, message: &str, cx: &mut Context<Self>) {
        self.clear_connection_bound_state();
        let authentication_failed = matches!(
            self.state,
            corbit_client::ConnectionState::AuthenticationFailed
        );
        let incompatible = matches!(
            self.state,
            corbit_client::ConnectionState::Incompatible { .. }
        );
        if !authentication_failed && !incompatible {
            self.state = corbit_client::ConnectionState::Offline;
        }
        if authentication_failed && self.credential_source == Some(CredentialSource::LocalDaemon) {
            self.show_error("已发现本机 Daemon，但自动读取的凭据被拒绝；如果启动时设置了 CORBIT_AUTH_TOKEN，请在设置 > Daemon 中手动填写同一 Token", cx);
        } else if authentication_failed || incompatible {
            self.show_error(self.detail.clone(), cx);
        } else {
            self.show_error(format!("连接失败：{message}"), cx);
        }
    }

    fn apply_event(&mut self, event: corbit_client::RuntimeEvent, cx: &mut Context<Self>) {
        match event {
            corbit_client::RuntimeEvent::HealthChecked => {
                if matches!(
                    self.state,
                    corbit_client::ConnectionState::Offline
                        | corbit_client::ConnectionState::Connecting
                ) {
                    self.detail = "Daemon 存活，正在建立 WebSocket".into();
                }
            }
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::StateChanged(state),
            ) => self.apply_connection_state(state, cx),
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::ServerInfo(info),
            ) => {
                self.detail = format!("Daemon {} · 协议 {}", info.version, info.protocol_version);
                self.server_info = Some(info);
                self.ensure_selected_provider();
                self.schedule_ui_state_save(cx);
            }
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::HistoryReset,
            ) => {
                self.clear_timeline();
                self.permissions.clear();
                self.detail = "Daemon 身份或事件游标已变化，正在重建权威时间线".into();
            }
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::AgentTimeline { payload, .. },
            ) => {
                let completed = matches!(
                    &payload.event,
                    corbit_client::AgentTimelineEvent::TurnCompleted { .. }
                );
                self.apply_timeline(payload, cx);
                if completed && self.main_section == MainSection::Scheduled {
                    self.schedule_scheduled_refresh(cx);
                }
            }
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::AgentPermission { payload, .. },
            ) => self.apply_permission(payload, cx),
            corbit_client::RuntimeEvent::Connection(
                corbit_client::ConnectionEvent::WorkspaceChanged(change),
            ) => self.apply_workspace_changed(&change, cx),
            corbit_client::RuntimeEvent::Snapshot(snapshot) => {
                self.detail = format!("权威状态已同步 · 修订 {}", snapshot.revision);
                self.snapshot = Some(snapshot);
                self.reconcile_selection();
                self.reconcile_new_task_recovery();
                self.start_provider_catalog_if_ready(cx);
                if self.main_section == MainSection::Scheduled {
                    self.load_scheduled_tasks(cx);
                }
                self.schedule_ui_state_save(cx);
                if self.main_section == MainSection::Resources
                    && self.resource_section == ResourceSection::Devices
                {
                    self.load_devices(cx);
                }
                if self.main_section == MainSection::Resources
                    && matches!(
                        self.resource_section,
                        ResourceSection::Plugins | ResourceSection::Hooks
                    )
                {
                    self.load_plugins(cx);
                    if self.resource_section == ResourceSection::Plugins {
                        self.load_codex_official_plugins(false, cx);
                    }
                }
            }
            corbit_client::RuntimeEvent::Error(message) => {
                self.apply_connection_error(&message, cx);
            }
        }
    }

    fn input_value(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().trim().to_owned()
    }

    fn ui_preferences(&self, cx: &App) -> UiPreferences {
        let (new_task_draft, prompt_drafts) = if self.general_preferences.save_prompt_drafts {
            (
                self.new_task_prompt.read(cx).value().to_string(),
                self.prompt_drafts.clone(),
            )
        } else {
            (String::new(), BTreeMap::new())
        };
        UiPreferences {
            general: self.general_preferences,
            agent_configuration: self.agent_configuration,
            coding: self.coding_preferences.clone(),
            integrations: self.integration_preferences.clone(),
            sidebar_collapsed: self.sidebar_collapsed,
            main_section: self.main_section,
            settings_return_section: self.settings_return_section,
            resource_section: self.resource_section,
            selected_project_id: self.selected_project_id.clone(),
            selected_workspace_id: self.selected_workspace_id.clone(),
            selected_agent_id: self.selected_agent_id.clone(),
            selected_provider: self.selected_provider.clone(),
            project_providers: self.project_providers.clone(),
            composer_selections: self.composer_selections.clone(),
            composer_permission_mode: self.composer_permission_mode,
            task_filter: self.task_filter,
            new_task_draft,
            prompt_drafts,
            new_task_recovery: self.new_task_recovery.clone(),
            panel_widths: self.panel_widths,
            window: self.window_placement,
        }
    }

    fn schedule_ui_state_save(&mut self, cx: &mut Context<Self>) {
        let preferences = self.ui_preferences(cx);
        self.ui_save_task = Some(cx.spawn(async move |view, cx| {
            Timer::after(Duration::from_millis(250)).await;
            let error = preferences.save().err().map(|error| error.to_string());
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.ui_state_error != error {
                    view.ui_state_error.clone_from(&error);
                    if let Some(error) = error {
                        view.show_error(format!("界面状态保存失败：{error}"), cx);
                    } else {
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn sync_prompt_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_agent_id = self.selected_agent_id.clone();
        let mut state_changed = false;

        if self.prompt_input_agent_id != selected_agent_id {
            if let Some(previous_agent_id) = self.prompt_input_agent_id.take() {
                let value = self.prompt_input.read(cx).value().to_string();
                if value.is_empty() {
                    self.prompt_drafts.remove(&previous_agent_id);
                } else {
                    self.prompt_drafts.insert(previous_agent_id, value);
                }
            }
            self.prompt_input_agent_id.clone_from(&selected_agent_id);
            let draft = selected_agent_id
                .as_ref()
                .and_then(|agent_id| self.prompt_drafts.get(agent_id))
                .cloned()
                .unwrap_or_default();
            self.prompt_input
                .update(cx, |input, cx| input.set_value(draft, window, cx));
            self.prompt_attachments.clear();
            state_changed = true;
        }

        if self.prompt_clear_agent_id.as_ref() == selected_agent_id.as_ref()
            && let Some(agent_id) = self.prompt_clear_agent_id.take()
        {
            self.prompt_drafts.remove(&agent_id);
            self.prompt_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            state_changed = true;
        }

        if self.new_task_clear_requested {
            self.new_task_clear_requested = false;
            self.new_task_prompt
                .update(cx, |input, cx| input.set_value("", window, cx));
            state_changed = true;
        }

        if state_changed {
            self.schedule_ui_state_save(cx);
        }
    }

    fn show_validation_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.show_error(message, cx);
    }

    fn set_main_section(&mut self, section: MainSection, cx: &mut Context<Self>) {
        if section == MainSection::Resources && self.main_section != MainSection::Resources {
            self.settings_return_section = self.main_section;
        }
        self.main_section = section;
        match section {
            MainSection::Files
                if self.snapshot.is_some()
                    && matches!(self.state, corbit_client::ConnectionState::Online)
                    && self.workspace_listing.is_none() =>
            {
                self.load_workspace_directory(String::new(), cx);
            }
            MainSection::Changes
                if self.snapshot.is_some()
                    && matches!(self.state, corbit_client::ConnectionState::Online)
                    && self.workspace_git_status.is_none() =>
            {
                self.load_workspace_git_status(cx);
            }
            MainSection::Scheduled => {
                self.load_scheduled_tasks(cx);
                self.schedule_ui_state_save(cx);
                cx.notify();
            }
            MainSection::NewTask
            | MainSection::Search
            | MainSection::Tasks
            | MainSection::Activity
            | MainSection::Permissions
            | MainSection::Conversation
            | MainSection::Files
            | MainSection::Changes
            | MainSection::Resources => {
                self.schedule_ui_state_save(cx);
                cx.notify();
            }
        }
    }

    fn set_resource_section(&mut self, section: ResourceSection, cx: &mut Context<Self>) {
        if self.main_section != MainSection::Resources {
            self.settings_return_section = self.main_section;
        }
        self.resource_section = section;
        self.main_section = MainSection::Resources;
        self.delete_confirmation = None;
        self.schedule_ui_state_save(cx);
        cx.notify();
        if section == ResourceSection::Devices {
            self.load_devices(cx);
        }
        if section == ResourceSection::Plugins {
            self.load_plugins(cx);
            self.load_codex_official_plugins(false, cx);
        }
        if section == ResourceSection::Hooks {
            self.load_plugins(cx);
        }
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        let return_section = self.settings_return_section;
        self.set_main_section(return_section, cx);
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn open_new_task(&mut self, _: &OpenNewTask, _: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::NewTask, cx);
    }

    fn open_search(&mut self, _: &OpenSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::Search, cx);
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    fn open_tasks(&mut self, _: &OpenTasks, _: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::Tasks, cx);
    }

    fn open_scheduled(&mut self, _: &OpenScheduled, _: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::Scheduled, cx);
    }

    fn open_activity(&mut self, _: &OpenActivity, _: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::Activity, cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.set_main_section(MainSection::Resources, cx);
    }

    fn submit_prompt_action(&mut self, _: &SubmitPrompt, _: &mut Window, cx: &mut Context<Self>) {
        match self.main_section {
            MainSection::NewTask => self.create_new_task(cx),
            MainSection::Conversation => self.send_prompt(cx),
            _ => {}
        }
    }

    fn render_section_button(
        &self,
        id: &'static str,
        section: MainSection,
        label: &'static str,
        icon: AppIcon,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .small()
            .h(navigation_row_height())
            .selected(self.main_section == section)
            .icon(Icon::new(icon))
            .label(label)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.set_main_section(section, cx);
            }))
    }

    fn render_connection_empty_state(
        &self,
        is_connecting: bool,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (title, description) = if is_online {
            ("正在同步工作区", "Daemon 已连接，正在读取项目和任务。")
        } else {
            (
                "连接 Corbit Daemon",
                "连接本机服务后，即可创建任务、与 Agent 对话并检查代码变更。",
            )
        };

        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .px_6()
            .pb(px(72.))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(px(500.))
                    .items_center()
                    .gap_3()
                    .child(brand_mark(44.))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_HEADING))
                            .font_semibold()
                            .child(title),
                    )
                    .child(
                        div()
                            .max_w(px(440.))
                            .text_center()
                            .line_height(px(21.))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(description),
                    )
                    .child(
                        div()
                            .max_w(px(460.))
                            .text_center()
                            .text_size(font_px(FONT_SIZE_SM))
                            .line_height(px(18.))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(self.detail.clone()),
                    )
                    .when(!is_online, |content| {
                        content.child(
                            Button::new("empty-connect")
                                .primary()
                                .label("连接 Daemon")
                                .loading(is_connecting)
                                .disabled(is_connecting)
                                .on_click(cx.listener(|view, _, _, cx| view.connect(cx))),
                        )
                    }),
            )
    }

    #[allow(clippy::too_many_lines)]
    fn render_main_header(&self, cx: &mut Context<Self>) -> Div {
        let selected_project = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_project_id.as_ref()?;
            snapshot
                .projects
                .iter()
                .find(|project| &project.id == selected)
        });
        let selected_workspace = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_workspace_id.as_ref()?;
            snapshot
                .workspaces
                .iter()
                .find(|workspace| &workspace.id == selected)
        });
        let selected_agent = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_agent_id.as_ref()?;
            snapshot.agents.iter().find(|agent| &agent.id == selected)
        });
        let conversation_menu_data = self.snapshot.as_ref().and_then(|snapshot| {
            let agent = selected_agent?;
            let workspace = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.id == agent.workspace_id)?;
            let project = snapshot
                .projects
                .iter()
                .find(|project| project.id == workspace.project_id)?;
            Some(AgentMenuData {
                agent_id: agent.id.clone(),
                agent_title: agent.title.clone(),
                working_directory: workspace.working_directory.clone(),
                project_id: project.id.clone(),
                workspace_id: workspace.id.clone(),
            })
        });
        let title = match self.main_section {
            MainSection::Resources => "设置".to_owned(),
            MainSection::NewTask => "新建任务".to_owned(),
            MainSection::Search => "搜索".to_owned(),
            MainSection::Tasks => "任务".to_owned(),
            MainSection::Scheduled => "已安排".to_owned(),
            MainSection::Activity => "活动".to_owned(),
            MainSection::Permissions => "审批".to_owned(),
            MainSection::Files => selected_workspace.map_or_else(
                || "工作区文件".to_owned(),
                |workspace| workspace.name.clone(),
            ),
            MainSection::Changes => "代码更改".to_owned(),
            MainSection::Conversation => selected_agent
                .map_or_else(|| "开始一个任务".to_owned(), |agent| agent.title.clone()),
        };
        let context = match self.main_section {
            MainSection::Resources => self.resource_section_label().to_owned(),
            MainSection::NewTask => selected_project.map_or_else(
                || "选择项目后直接描述任务".to_owned(),
                |project| project.root_path.clone(),
            ),
            MainSection::Search => "跨任务、工作区与项目查找".to_owned(),
            MainSection::Tasks => "所有本机 Agent 会话".to_owned(),
            MainSection::Scheduled => "在后台按计划运行 Agent 任务".to_owned(),
            MainSection::Activity => "本次连接的运行、完成与待处理事件".to_owned(),
            MainSection::Permissions => format!("{} 个待处理请求", self.permissions.len()),
            _ => match (selected_project, selected_workspace) {
                (Some(project), Some(workspace)) => {
                    format!("{}  /  {}", project.name, workspace.working_directory)
                }
                (Some(project), None) => project.root_path.clone(),
                _ => "本机工作区".to_owned(),
            },
        };
        let is_conversation = self.main_section == MainSection::Conversation;
        let conversation_title = title.clone();
        let standard_title = title;
        let header_view = cx.entity();
        let can_mutate_agent = matches!(self.state, corbit_client::ConnectionState::Online)
            && !self.operation_in_flight;
        div()
            .h_flex()
            .w_full()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(rgb(COLOR_BORDER_LIGHT))
            .bg(rgb(COLOR_SURFACE))
            .px_4()
            .when(self.sidebar_collapsed, |header| {
                header.pl(px(TITLEBAR_LEFT_PADDING))
            })
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .items_center()
                    .gap_2()
                    .when(self.sidebar_collapsed, |title_bar| {
                        title_bar.child(
                            Button::new("header-expand-sidebar")
                                .ghost()
                                .small()
                                .icon(Icon::new(AppIcon::PanelLeftOpen))
                                .tooltip("展开侧栏")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.toggle_sidebar(cx);
                                })),
                        )
                    })
                    .when(is_conversation, |title_bar| {
                        title_bar
                            .child(
                                Icon::new(AppIcon::Folder)
                                    .size(px(18.))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY)),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .max_w(px(CONVERSATION_TITLE_MAX_WIDTH))
                                    .truncate()
                                    .text_size(font_px(FONT_SIZE_BASE))
                                    .font_medium()
                                    .child(conversation_title),
                            )
                            .when_some(conversation_menu_data, |title_bar, data| {
                                title_bar.child(
                                    Button::new("header-conversation-more")
                                        .ghost()
                                        .small()
                                        .h(px(32.))
                                        .w(px(32.))
                                        .rounded(px(8.))
                                        .icon(Icon::new(AppIcon::More).size(px(16.)))
                                        .dropdown_menu(move |menu, _, _| {
                                            Self::agent_popup_menu(
                                                menu,
                                                header_view.clone(),
                                                AgentMenuData {
                                                    agent_id: data.agent_id.clone(),
                                                    agent_title: data.agent_title.clone(),
                                                    working_directory: data
                                                        .working_directory
                                                        .clone(),
                                                    project_id: data.project_id.clone(),
                                                    workspace_id: data.workspace_id.clone(),
                                                },
                                                can_mutate_agent,
                                            )
                                        }),
                                )
                            })
                    })
                    .when(!is_conversation, |title_bar| {
                        title_bar
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(font_px(FONT_SIZE_BASE))
                                    .font_medium()
                                    .truncate()
                                    .child(standard_title),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child(format!("·  {context}")),
                            )
                    }),
            )
            .child(div().h_flex().flex_none().items_center().gap_1().when(
                matches!(
                    self.main_section,
                    MainSection::Conversation | MainSection::Files | MainSection::Changes
                ),
                |controls| {
                    controls
                        .child(self.render_section_button(
                            "header-conversation",
                            MainSection::Conversation,
                            "对话",
                            AppIcon::Conversation,
                            cx,
                        ))
                        .child(self.render_section_button(
                            "header-files",
                            MainSection::Files,
                            "文件",
                            AppIcon::Folder,
                            cx,
                        ))
                        .child(self.render_section_button(
                            "header-changes",
                            MainSection::Changes,
                            "更改",
                            AppIcon::Changes,
                            cx,
                        ))
                },
            ))
    }

    fn render_shell_content(
        &self,
        show_sidebar: bool,
        sidebar: Div,
        main_pane: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !show_sidebar {
            return main_pane;
        }

        let view = cx.weak_entity();
        h_resizable("app-sidebar-split")
            .child(
                resizable_panel()
                    .size(px(self.panel_widths.sidebar()))
                    .size_range(px(SIDEBAR_MIN_WIDTH)..px(SIDEBAR_MAX_WIDTH))
                    .child(sidebar),
            )
            .child(resizable_panel().child(main_pane))
            .on_resize(move |state, _, cx| {
                let Some(width) = state.read(cx).sizes().first().copied() else {
                    return;
                };
                let _ = view.update(cx, |view, cx| {
                    view.panel_widths.set_sidebar(width.as_f32());
                    view.schedule_ui_state_save(cx);
                });
            })
            .into_any_element()
    }
}

fn root_overlay_layers(
    view: &ConnectionView,
    window: &mut Window,
    cx: &mut Context<ConnectionView>,
) -> Vec<AnyElement> {
    let mut layers = Vec::with_capacity(4);
    if let Some(layer) = Root::render_sheet_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_dialog_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_notification_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(feedback) = view.render_feedback(cx) {
        layers.push(feedback);
    }
    layers
}

impl Render for ConnectionView {
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_prompt_editors(window, cx);
        let status = match self.state {
            corbit_client::ConnectionState::Offline => "未连接",
            corbit_client::ConnectionState::Connecting => "正在连接",
            corbit_client::ConnectionState::Authenticating => "正在认证",
            corbit_client::ConnectionState::Online => "已连接",
            corbit_client::ConnectionState::Reconnecting { .. } => "等待重连",
            corbit_client::ConnectionState::AuthenticationFailed => "认证失败",
            corbit_client::ConnectionState::Incompatible { .. } => "协议不兼容",
        };
        let is_connecting = matches!(
            self.state,
            corbit_client::ConnectionState::Connecting
                | corbit_client::ConnectionState::Authenticating
                | corbit_client::ConnectionState::Reconnecting { .. }
        );
        let is_online = matches!(self.state, corbit_client::ConnectionState::Online);
        let is_settings = self.main_section == MainSection::Resources;
        let sidebar = self.render_sidebar(status, is_connecting, is_online, cx);
        let header = self.render_main_header(cx);
        let content: AnyElement = if is_settings {
            self.render_resource_panel(is_online, cx).into_any_element()
        } else if self.snapshot.is_none() {
            self.render_connection_empty_state(is_connecting, is_online, cx)
                .into_any_element()
        } else {
            match self.main_section {
                MainSection::NewTask => {
                    self.render_new_task_panel(is_online, cx).into_any_element()
                }
                MainSection::Search => self.render_search_panel(cx).into_any_element(),
                MainSection::Tasks => self.render_tasks_panel(is_online, cx).into_any_element(),
                MainSection::Scheduled => self
                    .render_scheduled_panel(is_online, window, cx)
                    .into_any_element(),
                MainSection::Activity => self.render_activity_panel(cx).into_any_element(),
                MainSection::Permissions => self
                    .render_permissions_panel(is_online, cx)
                    .into_any_element(),
                MainSection::Conversation => self
                    .render_timeline_panel(is_online, window, cx)
                    .into_any_element(),
                MainSection::Files => self
                    .render_workspace_files_panel(is_online, cx)
                    .into_any_element(),
                MainSection::Changes => self
                    .render_workspace_git_panel(is_online, cx)
                    .into_any_element(),
                MainSection::Resources => {
                    unreachable!("settings are rendered before snapshot state")
                }
            }
        };

        let main_pane = if is_settings {
            div()
                .v_flex()
                .size_full()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .bg(rgb(COLOR_SURFACE))
                .child(content)
                .into_any_element()
        } else {
            div()
                .v_flex()
                .h_full()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .bg(rgb(COLOR_SURFACE))
                .child(header)
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .child(content),
                )
                .into_any_element()
        };
        let show_sidebar = !is_settings && !self.sidebar_collapsed;
        let shell_content = self.render_shell_content(show_sidebar, sidebar, main_pane, cx);
        let overlay_layers = root_overlay_layers(self, window, cx);

        div()
            .on_action(cx.listener(Self::open_new_task))
            .on_action(cx.listener(Self::open_search))
            .on_action(cx.listener(Self::open_tasks))
            .on_action(cx.listener(Self::open_scheduled))
            .on_action(cx.listener(Self::open_activity))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::capture_app_snapshot_action))
            .on_action(cx.listener(Self::submit_prompt_action))
            .h_flex()
            .size_full()
            .items_start()
            .min_w(px(0.))
            .overflow_hidden()
            .bg(shell_background())
            .font_family(interface_font_family())
            .text_size(font_px(FONT_SIZE_BASE))
            .font_weight(FontWeight(FONT_WEIGHT_BASE))
            .text_color(rgb(COLOR_TEXT))
            .child(shell_content)
            .children(overlay_layers)
    }
}

fn input_state(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<ConnectionView>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(SharedString::from(placeholder)))
}

fn corbit_window_options(
    appearance: AppearancePreferences,
    window_placement: Option<WindowPlacement>,
) -> WindowOptions {
    let mut window_options = WindowOptions {
        window_bounds: window_placement.and_then(WindowPlacement::window_bounds),
        window_background: if appearance.translucent_sidebar {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        },
        ..WindowOptions::default()
    };
    if let Some(titlebar) = &mut window_options.titlebar {
        titlebar.title = None;
        titlebar.appears_transparent = true;
        titlebar.traffic_light_position = Some(gpui::point(px(12.), px(15.)));
    }
    window_options
}

fn show_main_window(cx: &mut App) -> anyhow::Result<()> {
    let front_window = cx
        .window_stack()
        .and_then(|windows| windows.first().copied())
        .or_else(|| cx.windows().first().copied());

    cx.activate(true);
    if let Some(window_handle) = front_window {
        window_handle.update(cx, |_, window, _| window.activate_window())?;
        return Ok(());
    }

    let appearance = AppearancePreferences::load();
    let ui_preferences = UiPreferences::load();
    configure_codex_theme(appearance, None, cx);
    if let Err(error) = application_icon::apply(appearance.app_icon_mode, is_dark_mode()) {
        eprintln!("Failed to apply Corbit application icon: {error:#}");
    }
    let window_options = corbit_window_options(appearance, ui_preferences.window);
    let window_handle = cx.open_window(window_options, |window, cx| {
        let view = cx.new(|cx| ConnectionView::new(window, cx, appearance, ui_preferences));
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    window_handle.update(cx, |_, window, _| window.activate_window())?;

    Ok(())
}

pub(crate) fn run() {
    let application = Application::new().with_assets(BrandAssets);
    application.on_reopen(|cx| {
        if let Err(error) = show_main_window(cx) {
            eprintln!("Failed to reopen the Corbit window: {error:#}");
        }
    });

    application.run(|cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("shift-enter", Enter { secondary: true }, Some("Input")),
            KeyBinding::new("cmd-enter", SubmitPrompt, Some("Input")),
            KeyBinding::new("cmd-n", OpenNewTask, None),
            KeyBinding::new("cmd-k", OpenSearch, None),
            KeyBinding::new("cmd-1", OpenTasks, None),
            KeyBinding::new("cmd-shift-a", OpenActivity, None),
            KeyBinding::new("cmd-shift-2", CaptureAppSnapshot, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("ctrl-n", OpenNewTask, None),
            KeyBinding::new("ctrl-k", OpenSearch, None),
            KeyBinding::new("ctrl-1", OpenTasks, None),
            KeyBinding::new("ctrl-shift-a", OpenActivity, None),
            KeyBinding::new("ctrl-shift-2", CaptureAppSnapshot, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("ctrl-enter", SubmitPrompt, Some("Input")),
        ]);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty()
                && UiPreferences::load().general.close_window_behavior == CloseWindowBehavior::Quit
            {
                cx.quit();
            }
        })
        .detach();

        #[cfg(target_os = "macos")]
        if let Err(error) = system_tray::install(cx) {
            eprintln!("Failed to install the Corbit system tray: {error:#}");
        }

        cx.spawn(async move |cx| {
            cx.update(show_main_window)??;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
