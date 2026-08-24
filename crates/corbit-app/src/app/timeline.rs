pub(in crate::app) mod composer;
mod index;
mod lifecycle;
mod model;
mod provider_switch;

use super::*;
use chrono::{DateTime, Utc};
use composer::{
    MAX_PROMPT_ATTACHMENTS, attachment_size_label, context_window_percent, load_prompt_attachments,
    permission_mode_copy,
};
use gpui::ease_out_quint;
pub(super) use index::ConversationIndexInteraction;
use index::{
    CONVERSATION_INDEX_ANIMATION_DURATION, active_turn_indicator_dot_opacity,
    closest_conversation_index_entry, conversation_index_entries,
    conversation_index_marker_metrics, interpolate_rgba, scroll_timeline_to_latest,
};
use lifecycle::{apply_turn_completed_event, apply_turn_started_event, remember_turn_provider};
pub(super) use model::{
    ComposerAttachment, PendingPermission, QueuedPrompt, RetryControl, RetryPrompt, TimelineIndex,
    TimelineStatus, TimelineTurn,
};
use model::{TimelineStep, TimelineStepKind, TimelineUsage};
use provider_switch::{ProviderSwitchFailure, execute_provider_switch};

const ACTIVE_TURN_INDICATOR_ANIMATION_DURATION: Duration = Duration::from_millis(1_100);
const TIMELINE_JUMP_TO_LATEST_THRESHOLD: f32 = 48.;
const TIMELINE_FLOATING_CONTROL_SIZE: f32 = 40.;
const STREAMING_RESPONSE_PREVIEW_BYTES: usize = 12 * 1024;
const LONG_RESPONSE_VIRTUALIZATION_BYTES: usize = 24 * 1024;
const LONG_RESPONSE_VIEW_HEIGHT: f32 = 560.;
const CONVERSATION_BODY_FONT_SIZE: f32 = 15.;
const CONVERSATION_BODY_LINE_HEIGHT: f32 = 24.;
const CONVERSATION_PARAGRAPH_GAP_REMS: f32 = 0.75;
const DESKTOP_NOTIFICATION_MAX_EVENT_AGE_SECONDS: i64 = 120;

fn event_is_recent(occurred_at: &str) -> bool {
    let Ok(occurred_at) = DateTime::parse_from_rfc3339(occurred_at) else {
        return false;
    };
    let age = Utc::now().signed_duration_since(occurred_at.with_timezone(&Utc));
    age.num_seconds() >= -5 && age.num_seconds() <= DESKTOP_NOTIFICATION_MAX_EVENT_AGE_SECONDS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TurnCompletionActions {
    advance_queue: bool,
    notify: bool,
}

fn turn_completion_actions(already_terminal: bool, occurred_at: &str) -> TurnCompletionActions {
    let advance_queue = !already_terminal;
    TurnCompletionActions {
        advance_queue,
        notify: advance_queue && event_is_recent(occurred_at),
    }
}

fn conversation_markdown_style() -> TextViewStyle {
    let mut style = TextViewStyle::default()
        .paragraph_gap(rems(CONVERSATION_PARAGRAPH_GAP_REMS))
        .heading_font_size(|level, base| match level {
            1 => base * 1.4,
            2 => base * 1.2,
            3 => base * 1.1,
            _ => base,
        });
    style.heading_base_font_size = font_px(CONVERSATION_BODY_FONT_SIZE);
    style
}

fn streaming_response_preview(response: &str) -> (&str, bool) {
    if response.len() <= STREAMING_RESPONSE_PREVIEW_BYTES {
        return (response, false);
    }

    let mut start = response.len() - STREAMING_RESPONSE_PREVIEW_BYTES;
    while !response.is_char_boundary(start) {
        start += 1;
    }
    (&response[start..], true)
}

fn should_virtualize_response(response: &str) -> bool {
    response.len() >= LONG_RESPONSE_VIRTUALIZATION_BYTES
}

fn final_response_step(turn: &TimelineTurn) -> Option<(usize, &str)> {
    let (index, text) =
        turn.steps
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, step)| match &step.kind {
                TimelineStepKind::AssistantMessage { text } => Some((index, text.as_str())),
                _ => None,
            })?;

    (turn.steps[index + 1..].is_empty() && !text.trim().is_empty()).then_some((index, text))
}

fn final_response(turn: &TimelineTurn) -> Option<&str> {
    final_response_step(turn).map(|(_, response)| response)
}

fn shows_completed_response_footer(turn: &TimelineTurn) -> bool {
    turn.status == TimelineStatus::Completed && final_response(turn).is_some()
}

fn automatically_expands_timeline_activity(turn: &TimelineTurn) -> bool {
    turn.status == TimelineStatus::InProgress && final_response(turn).is_none()
}

fn timeline_activity_is_expanded(
    turn: &TimelineTurn,
    explicitly_expanded: bool,
    explicitly_collapsed_while_streaming: bool,
) -> bool {
    if automatically_expands_timeline_activity(turn) {
        !explicitly_collapsed_while_streaming
    } else {
        explicitly_expanded
    }
}

fn permission_belongs_to_agent(
    permission: &PendingPermission,
    selected_agent_id: Option<&str>,
) -> bool {
    selected_agent_id.is_some_and(|agent_id| permission.agent_id == agent_id)
}

fn permission_kind_label(permission_kind: &str) -> &'static str {
    match permission_kind {
        "command" => "命令执行权限",
        "file-change" => "文件修改权限",
        _ => "Agent 权限",
    }
}

fn permission_question(permission: &PendingPermission) -> String {
    permission
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .map_or_else(
            || match permission.permission_kind.as_str() {
                "command" => "是否允许执行此命令？".to_owned(),
                "file-change" => "是否允许修改这些文件？".to_owned(),
                _ => "是否允许 Agent 继续此操作？".to_owned(),
            },
            str::to_owned,
        )
}

fn should_show_timeline_jump_control(
    item_count: usize,
    logical_scroll_item: usize,
    last_item_bottom: Option<gpui::Pixels>,
    viewport_bottom: gpui::Pixels,
) -> bool {
    item_count > 0
        && logical_scroll_item < item_count
        && last_item_bottom
            .is_none_or(|bottom| bottom - viewport_bottom >= px(TIMELINE_JUMP_TO_LATEST_THRESHOLD))
}

fn timeline_is_away_from_latest(list_state: &ListState) -> bool {
    let item_count = list_state.item_count();
    let last_item_bottom = item_count
        .checked_sub(1)
        .and_then(|index| list_state.bounds_for_item(index))
        .map(|bounds| bounds.bottom());

    should_show_timeline_jump_control(
        item_count,
        list_state.logical_scroll_top().item_ix,
        last_item_bottom,
        list_state.viewport_bounds().bottom(),
    )
}

fn tool_display_title(raw: &str) -> String {
    if !raw.contains(['_', '-', '.']) {
        return raw.to_owned();
    }

    raw.split(['_', '-', '.'])
        .filter(|part| !part.is_empty())
        .enumerate()
        .map(|(index, part)| match part.to_ascii_lowercase().as_str() {
            "openai" => "OpenAI".to_owned(),
            "api" => "API".to_owned(),
            "json" => "JSON".to_owned(),
            "url" => "URL".to_owned(),
            "id" => "ID".to_owned(),
            _ if index == 0 => {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            }
            _ => part.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn composer_option_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(rgb(COLOR_TEXT_TERTIARY).into())
        .hover(sidebar_row_hover_rgb().into())
        .active(sidebar_row_active_rgb().into())
}

fn composer_action_variant(cx: &App) -> ButtonCustomVariant {
    let (background, foreground, hover, active) = if is_dark_mode() {
        (0xe2_e2e2, 0x2b_2b2b, 0xf0_f0f0, 0xd2_d2d2)
    } else {
        (0x2b_2b2b, 0xf7_f7f7, 0x1f_1f1f, 0x3b_3b3b)
    };

    ButtonCustomVariant::new(cx)
        .color(fixed_rgb(background).into())
        .foreground(fixed_rgb(foreground).into())
        .hover(fixed_rgb(hover).into())
        .active(fixed_rgb(active).into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContextWindowUsage {
    used_tokens: u64,
    context_window: u64,
}

impl ContextWindowUsage {
    fn percent(self) -> u8 {
        context_window_percent(self.used_tokens, self.context_window)
    }
}

impl ConnectionView {
    fn provider_catalog_entry(
        &self,
        provider: &str,
    ) -> Option<&corbit_client::ProviderCatalogEntry> {
        self.provider_catalog
            .as_ref()?
            .providers
            .iter()
            .find(|entry| entry.provider_id == provider && entry.available)
    }

    fn composer_model_info(&self, provider: &str) -> Option<&corbit_client::ProviderModelInfo> {
        let entry = self.provider_catalog_entry(provider)?;
        self.selected_agent_id
            .as_deref()
            .and_then(|agent_id| self.composer_selections.model(agent_id, entry))
            .or_else(|| entry.models.iter().find(|model| model.is_default))
            .or_else(|| entry.models.first())
    }

    fn composer_reasoning_effort(
        &self,
        provider: &str,
    ) -> Option<corbit_client::AgentReasoningEffort> {
        let model = self.composer_model_info(provider)?;
        let entry = self.provider_catalog_entry(provider)?;
        self.selected_agent_id
            .as_deref()
            .and_then(|agent_id| self.composer_selections.reasoning_effort(agent_id, entry))
            .or(model.default_reasoning_effort.filter(|effort| {
                model
                    .supported_reasoning_efforts
                    .iter()
                    .any(|candidate| candidate.reasoning_effort == *effort)
            }))
    }

    pub(super) fn reconcile_composer_catalog(&mut self) {
        let Some(catalog) = &self.provider_catalog else {
            return;
        };
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        self.composer_selections
            .reconcile_catalog(catalog, &snapshot.agents, &snapshot.projects);
    }

    fn choose_composer_model(&mut self, provider: &str, model: &str, cx: &mut Context<Self>) {
        if let (Some(agent_id), Some(entry)) = (
            self.selected_agent_id.as_deref(),
            self.provider_catalog_entry(provider).cloned(),
        ) && self
            .composer_selections
            .choose_model(agent_id, &entry, model)
        {
            self.reconcile_composer_catalog();
            self.schedule_ui_state_save(cx);
            cx.notify();
        }
    }

    fn composer_prompt_options(&self, provider: &str) -> corbit_client::AgentPromptOptions {
        let supports_turn_options = provider_supports_turn_options(provider);
        let supports_codex_configuration = provider == "codex";
        corbit_client::AgentPromptOptions {
            model: supports_turn_options
                .then(|| {
                    self.composer_model_info(provider)
                        .map(|model| model.id.clone())
                })
                .flatten(),
            permission_mode: supports_turn_options.then_some(self.composer_permission_mode),
            reasoning_effort: supports_turn_options
                .then(|| self.composer_reasoning_effort(provider))
                .flatten(),
            network_access: supports_codex_configuration
                .then_some(self.agent_configuration.network_access),
            reasoning_summary: supports_codex_configuration
                .then_some(self.agent_configuration.reasoning_summary),
            personality: supports_codex_configuration
                .then_some(self.agent_configuration.personality)
                .flatten(),
            attachments: self
                .prompt_attachments
                .iter()
                .map(|attachment| attachment.upload.clone())
                .collect(),
        }
    }

    fn apply_provider_switch_result(
        &mut self,
        result: Result<corbit_client::AuthoritativeSnapshot, ProviderSwitchFailure>,
        project_id: &str,
        provider: &str,
        provider_label: &str,
        cx: &mut Context<Self>,
    ) {
        self.provider_switch_in_flight = false;
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.project_providers
                    .insert(project_id.to_owned(), provider.to_owned());
                self.reconcile_selection();
                self.reconcile_composer_catalog();
                self.show_success(format!("当前项目已切换到 {provider_label}"), cx);
            }
            Err(failure) => {
                if let Some(snapshot) = failure.snapshot {
                    self.snapshot = Some(snapshot);
                    self.reconcile_selection();
                }
                if failure.provider_updated {
                    self.project_providers
                        .insert(project_id.to_owned(), provider.to_owned());
                    self.reconcile_composer_catalog();
                }
                self.show_error(
                    format!(
                        "Provider 切换未完成：{}。任务状态已同步，可在任务菜单中重新启动后继续。",
                        failure.message
                    ),
                    cx,
                );
            }
        }
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn switch_conversation_provider(&mut self, provider: &str, cx: &mut Context<Self>) {
        if self.provider_switch_in_flight || self.operation_in_flight || self.prompt_in_flight {
            self.show_warning("另一个任务操作正在执行，请稍候", cx);
            return;
        }
        if !self.provider_is_available(provider) {
            self.show_validation_error("所选提供商当前不可用", cx);
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.show_validation_error("正在同步任务状态，请稍后重试", cx);
            return;
        };
        let Some(agent) = self
            .selected_agent_id
            .as_ref()
            .and_then(|selected| snapshot.agents.iter().find(|agent| agent.id == *selected))
        else {
            self.show_validation_error("请先选择一个任务", cx);
            return;
        };
        let Some(project_id) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.id == agent.workspace_id)
            .map(|workspace| workspace.project_id.clone())
        else {
            self.show_validation_error("任务关联的项目已不存在", cx);
            return;
        };
        if agent.provider == provider {
            self.set_project_provider_preference(&project_id, provider, cx);
            return;
        }
        if agent.status != corbit_client::AgentStatus::Running {
            self.show_validation_error("只有运行中的任务可以在输入框切换 Provider", cx);
            return;
        }
        if self
            .timeline_index
            .agent_indices(&agent.id)
            .iter()
            .filter_map(|index| self.timeline.get(*index))
            .any(|turn| turn.status == TimelineStatus::InProgress)
        {
            self.show_validation_error("请等待当前 Turn 完成后再切换 Provider", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return;
        };
        if !matches!(self.state, corbit_client::ConnectionState::Online) {
            self.show_validation_error("请等待 Daemon 连接完成", cx);
            return;
        }

        let agent = agent.clone();
        let provider = provider.to_owned();
        let provider_label = Self::provider_label(&provider).to_owned();
        self.provider_switch_in_flight = true;
        self.detail = format!("正在切换到 {provider_label}…");
        self.provider_switch_task = Some(cx.spawn(async move |view, cx| {
            let result = execute_provider_switch(client, agent, provider.clone()).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.apply_provider_switch_result(
                    result,
                    &project_id,
                    &provider,
                    &provider_label,
                    cx,
                );
            });
        }));
        cx.notify();
    }

    pub(super) fn apply_timeline(
        &mut self,
        payload: corbit_client::AgentTimelinePayload,
        cx: &mut Context<Self>,
    ) {
        use corbit_client::AgentTimelineEvent;

        let corbit_client::AgentTimelinePayload {
            agent_id,
            provider,
            event,
            ..
        } = payload;
        let completion = match &event {
            AgentTimelineEvent::TurnCompleted {
                turn_id,
                status,
                error,
                occurred_at,
                ..
            } => {
                let already_terminal = self
                    .timeline_index
                    .location(&agent_id, turn_id)
                    .and_then(|location| self.timeline.get(location.timeline_index))
                    .is_some_and(|turn| turn.status != TimelineStatus::InProgress);
                let actions = turn_completion_actions(already_terminal, occurred_at);
                actions
                    .advance_queue
                    .then(|| (status.clone(), error.clone(), actions.notify))
            }
            _ => None,
        };
        if self.general_preferences.auto_follow_output
            && self.selected_agent_id.as_deref() == Some(agent_id.as_str())
        {
            self.timeline_follow_pending = true;
        }
        let provider = provider.or_else(|| {
            self.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.id == agent_id)
                    .map(|agent| agent.provider.clone())
            })
        });
        if let Some(turn_id) = Self::timeline_event_turn_id(&event) {
            self.timeline_dirty_turns
                .insert((agent_id.clone(), turn_id.to_owned()));
            remember_turn_provider(
                self.timeline_turn_mut(&agent_id, turn_id),
                provider.as_deref(),
            );
        }
        match event {
            event @ (AgentTimelineEvent::TurnStarted { .. }
            | AgentTimelineEvent::TurnCompleted { .. }) => {
                self.apply_turn_lifecycle_event(&agent_id, event);
            }
            event @ (AgentTimelineEvent::AssistantDelta { .. }
            | AgentTimelineEvent::ReasoningDelta { .. }
            | AgentTimelineEvent::PlanUpdated { .. }) => {
                self.apply_timeline_content_event(&agent_id, event);
            }
            event @ (AgentTimelineEvent::CommandUpdated { .. }
            | AgentTimelineEvent::CommandOutputDelta { .. }) => {
                self.apply_timeline_command_event(&agent_id, event);
            }
            event @ (AgentTimelineEvent::FileChangeUpdated { .. }
            | AgentTimelineEvent::ToolUpdated { .. }) => {
                self.apply_timeline_artifact_event(&agent_id, event);
            }
            event @ (AgentTimelineEvent::TurnDiffUpdated { .. }
            | AgentTimelineEvent::TurnUsageUpdated { .. }) => {
                self.apply_timeline_metadata_event(&agent_id, event);
            }
            AgentTimelineEvent::Unknown => {}
        }
        if let Some((status, error, should_notify)) = completion {
            if should_notify {
                self.notify_turn_completion(&agent_id, &status, error.as_deref());
            }
            self.start_next_queued_prompt(&agent_id, cx);
        }
    }

    fn notify_turn_completion(
        &self,
        agent_id: &str,
        status: &corbit_client::AgentTurnStatus,
        error: Option<&str>,
    ) {
        let task_name = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.agents.iter().find(|agent| agent.id == agent_id))
            .map_or("Corbit 任务", |agent| agent.title.as_str());
        let (enabled, title, body) = match status {
            corbit_client::AgentTurnStatus::Completed => (
                self.general_preferences.notify_task_completion,
                "任务已完成",
                task_name.to_owned(),
            ),
            corbit_client::AgentTurnStatus::Interrupted => (
                self.general_preferences.notify_task_failure,
                "任务已停止",
                task_name.to_owned(),
            ),
            corbit_client::AgentTurnStatus::Failed => (
                self.general_preferences.notify_task_failure,
                "任务执行失败",
                error.map_or_else(
                    || task_name.to_owned(),
                    |error| format!("{task_name}：{error}"),
                ),
            ),
        };
        if enabled {
            desktop_notifications::send(title, &body, self.general_preferences.notification_sound);
        }
    }

    fn timeline_event_turn_id(event: &corbit_client::AgentTimelineEvent) -> Option<&str> {
        use corbit_client::AgentTimelineEvent;

        match event {
            AgentTimelineEvent::TurnStarted { turn_id, .. }
            | AgentTimelineEvent::AssistantDelta { turn_id, .. }
            | AgentTimelineEvent::ReasoningDelta { turn_id, .. }
            | AgentTimelineEvent::PlanUpdated { turn_id, .. }
            | AgentTimelineEvent::CommandUpdated { turn_id, .. }
            | AgentTimelineEvent::CommandOutputDelta { turn_id, .. }
            | AgentTimelineEvent::FileChangeUpdated { turn_id, .. }
            | AgentTimelineEvent::ToolUpdated { turn_id, .. }
            | AgentTimelineEvent::TurnDiffUpdated { turn_id, .. }
            | AgentTimelineEvent::TurnUsageUpdated { turn_id, .. }
            | AgentTimelineEvent::TurnCompleted { turn_id, .. } => Some(turn_id),
            AgentTimelineEvent::Unknown => None,
        }
    }

    fn apply_turn_lifecycle_event(
        &mut self,
        agent_id: &str,
        event: corbit_client::AgentTimelineEvent,
    ) {
        match event {
            corbit_client::AgentTimelineEvent::TurnStarted {
                turn_id,
                prompt,
                occurred_at,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                apply_turn_started_event(turn, prompt, occurred_at);
            }
            corbit_client::AgentTimelineEvent::TurnCompleted {
                turn_id,
                status,
                error,
                occurred_at,
                duration_ms,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                apply_turn_completed_event(turn, &status, error, occurred_at, duration_ms);
            }
            _ => unreachable!("only lifecycle events are routed here"),
        }
    }

    fn apply_timeline_content_event(
        &mut self,
        agent_id: &str,
        event: corbit_client::AgentTimelineEvent,
    ) {
        match event {
            corbit_client::AgentTimelineEvent::AssistantDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                let step = Self::timeline_step_mut(
                    turn,
                    item_id,
                    TimelineStepKind::AssistantMessage {
                        text: String::new(),
                    },
                );
                step.status = corbit_client::AgentTimelineStepStatus::InProgress;
                if let TimelineStepKind::AssistantMessage { text } = &mut step.kind {
                    text.push_str(&delta);
                }
            }
            corbit_client::AgentTimelineEvent::ReasoningDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                let step = Self::timeline_step_mut(
                    turn,
                    item_id,
                    TimelineStepKind::Reasoning {
                        text: String::new(),
                    },
                );
                step.status = corbit_client::AgentTimelineStepStatus::InProgress;
                if let TimelineStepKind::Reasoning { text } = &mut step.kind {
                    text.push_str(&delta);
                }
            }
            corbit_client::AgentTimelineEvent::PlanUpdated {
                turn_id,
                explanation,
                steps,
                ..
            } => {
                let status = Self::timeline_plan_status(&steps);
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                let step = Self::timeline_step_mut(
                    turn,
                    "turn-plan".into(),
                    TimelineStepKind::Plan {
                        explanation: None,
                        steps: Vec::new(),
                    },
                );
                step.status = status;
                step.kind = TimelineStepKind::Plan { explanation, steps };
            }
            _ => unreachable!("only content events are routed here"),
        }
    }

    fn timeline_plan_status(
        steps: &[corbit_client::AgentTimelinePlanStep],
    ) -> corbit_client::AgentTimelineStepStatus {
        if !steps.is_empty()
            && steps
                .iter()
                .all(|step| step.status == corbit_client::AgentTimelineStepStatus::Completed)
        {
            corbit_client::AgentTimelineStepStatus::Completed
        } else if steps
            .iter()
            .any(|step| step.status == corbit_client::AgentTimelineStepStatus::InProgress)
        {
            corbit_client::AgentTimelineStepStatus::InProgress
        } else {
            corbit_client::AgentTimelineStepStatus::Pending
        }
    }

    fn apply_timeline_command_event(
        &mut self,
        agent_id: &str,
        event: corbit_client::AgentTimelineEvent,
    ) {
        let corbit_client::AgentTimelineEvent::CommandUpdated {
            turn_id,
            item_id,
            command,
            status,
            cwd,
            output,
            exit_code,
            duration_ms,
            ..
        } = event
        else {
            let corbit_client::AgentTimelineEvent::CommandOutputDelta {
                turn_id,
                item_id,
                delta,
                ..
            } = event
            else {
                unreachable!("only command events are routed here");
            };
            let turn = self.timeline_turn_mut(agent_id, &turn_id);
            let step = Self::timeline_step_mut(turn, item_id, Self::empty_command_step());
            step.status = corbit_client::AgentTimelineStepStatus::InProgress;
            if let TimelineStepKind::Command { output, .. } = &mut step.kind {
                output.push_str(&delta);
            }
            return;
        };

        let turn = self.timeline_turn_mut(agent_id, &turn_id);
        let step = Self::timeline_step_mut(turn, item_id, Self::empty_command_step());
        let previous_output = match &step.kind {
            TimelineStepKind::Command { output, .. } => output.clone(),
            _ => String::new(),
        };
        step.status = status;
        step.kind = TimelineStepKind::Command {
            command,
            cwd,
            output: output.unwrap_or(previous_output),
            exit_code,
            duration_ms,
        };
    }

    fn empty_command_step() -> TimelineStepKind {
        TimelineStepKind::Command {
            command: String::new(),
            cwd: None,
            output: String::new(),
            exit_code: None,
            duration_ms: None,
        }
    }

    fn apply_timeline_artifact_event(
        &mut self,
        agent_id: &str,
        event: corbit_client::AgentTimelineEvent,
    ) {
        match event {
            corbit_client::AgentTimelineEvent::FileChangeUpdated {
                turn_id,
                item_id,
                status,
                changes,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                let step = Self::timeline_step_mut(
                    turn,
                    item_id,
                    TimelineStepKind::FileChange {
                        changes: Vec::new(),
                    },
                );
                step.status = status;
                step.kind = TimelineStepKind::FileChange { changes };
            }
            corbit_client::AgentTimelineEvent::ToolUpdated {
                turn_id,
                item_id,
                tool_name,
                status,
                title,
                input,
                output,
                error,
                duration_ms,
                ..
            } => {
                let turn = self.timeline_turn_mut(agent_id, &turn_id);
                let step = Self::timeline_step_mut(
                    turn,
                    item_id,
                    TimelineStepKind::Tool {
                        tool_name: String::new(),
                        title: None,
                        input: None,
                        output: None,
                        error: None,
                        duration_ms: None,
                    },
                );
                step.status = status;
                step.kind = TimelineStepKind::Tool {
                    tool_name,
                    title,
                    input,
                    output,
                    error,
                    duration_ms,
                };
            }
            _ => unreachable!("only artifact events are routed here"),
        }
    }

    fn apply_timeline_metadata_event(
        &mut self,
        agent_id: &str,
        event: corbit_client::AgentTimelineEvent,
    ) {
        match event {
            corbit_client::AgentTimelineEvent::TurnDiffUpdated { turn_id, diff, .. } => {
                self.timeline_turn_mut(agent_id, &turn_id).diff = Some(diff);
            }
            corbit_client::AgentTimelineEvent::TurnUsageUpdated {
                turn_id,
                input_tokens,
                output_tokens,
                total_tokens,
                cached_input_tokens,
                reasoning_output_tokens,
                context_window,
                ..
            } => {
                self.timeline_turn_mut(agent_id, &turn_id).usage = Some(TimelineUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    cached_input_tokens,
                    reasoning_output_tokens,
                    context_window,
                });
            }
            _ => unreachable!("only metadata events are routed here"),
        }
    }

    pub(super) fn apply_permission(
        &mut self,
        payload: corbit_client::AgentPermissionPayload,
        cx: &mut Context<Self>,
    ) {
        let notification = match &payload.event {
            corbit_client::AgentPermissionEvent::PermissionRequested {
                approval_id,
                occurred_at,
                ..
            } => Some((
                approval_id.clone(),
                occurred_at.clone(),
                self.permissions
                    .iter()
                    .any(|permission| permission.approval_id == *approval_id),
            )),
            corbit_client::AgentPermissionEvent::PermissionResolved { .. } => None,
        };
        let agent_id = payload.agent_id;
        let notification_agent_id = agent_id.clone();
        let affected_turn = match payload.event {
            corbit_client::AgentPermissionEvent::PermissionRequested {
                approval_id,
                turn_id,
                permission_kind,
                reason,
                command,
                cwd,
                grant_root,
                available_decisions,
                ..
            } => {
                let affected_turn = (agent_id.clone(), turn_id.clone());
                let permission = PendingPermission {
                    agent_id,
                    approval_id: approval_id.clone(),
                    turn_id,
                    permission_kind,
                    reason,
                    command,
                    cwd,
                    grant_root,
                    available_decisions,
                };
                if let Some(index) = self
                    .permissions
                    .iter()
                    .position(|current| current.approval_id == approval_id)
                {
                    self.permissions[index] = permission;
                } else {
                    self.permissions.push(permission);
                }
                self.detail = "Agent 正在等待权限决定".into();
                Some(affected_turn)
            }
            corbit_client::AgentPermissionEvent::PermissionResolved { approval_id, .. } => {
                let affected_turn = self
                    .permissions
                    .iter()
                    .find(|permission| permission.approval_id == approval_id)
                    .map(|permission| (permission.agent_id.clone(), permission.turn_id.clone()));
                self.permissions
                    .retain(|permission| permission.approval_id != approval_id);
                affected_turn
            }
        };
        if let Some(turn) = affected_turn {
            self.timeline_dirty_turns.insert(turn);
        }
        if let Some((_, occurred_at, already_present)) = notification
            && !already_present
            && event_is_recent(&occurred_at)
            && self.general_preferences.notify_permission_requests
        {
            let task_name = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .find(|agent| agent.id == notification_agent_id)
                })
                .map_or("Corbit 任务", |agent| agent.title.as_str());
            desktop_notifications::send(
                "等待授权",
                task_name,
                self.general_preferences.notification_sound,
            );
        }
        cx.notify();
    }

    fn timeline_turn_mut(&mut self, agent_id: &str, turn_id: &str) -> &mut TimelineTurn {
        if let Some(location) = self.timeline_index.location(agent_id, turn_id) {
            return &mut self.timeline[location.timeline_index];
        }
        let timeline_index = self.timeline.len();
        self.timeline.push(TimelineTurn {
            agent_id: agent_id.to_owned(),
            turn_id: turn_id.to_owned(),
            provider: None,
            prompt: String::new(),
            steps: Vec::new(),
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        });
        self.timeline_index
            .insert(agent_id, turn_id, timeline_index);
        &mut self.timeline[timeline_index]
    }

    fn timeline_step_mut(
        turn: &mut TimelineTurn,
        item_id: String,
        kind: TimelineStepKind,
    ) -> &mut TimelineStep {
        if let Some(index) = turn.steps.iter().position(|step| step.item_id == item_id) {
            return &mut turn.steps[index];
        }
        turn.steps.push(TimelineStep {
            item_id,
            status: corbit_client::AgentTimelineStepStatus::Pending,
            kind,
        });
        turn.steps
            .last_mut()
            .expect("a placeholder timeline step was just inserted")
    }

    pub(super) fn clear_timeline(&mut self) {
        self.timeline.clear();
        self.timeline_index.clear();
        self.timeline_dirty_turns.clear();
        self.expanded_timeline_steps.clear();
        self.expanded_timeline_activity.clear();
        self.collapsed_streaming_timeline_activity.clear();
        self.timeline_list_state.reset(0);
        self.timeline_list_agent_id = None;
        self.conversation_index_interaction.reset();
        self.timeline_follow_pending = false;
        self.sleep_preventer = None;
    }

    pub(super) fn flush_timeline_list_updates(&mut self) {
        let dirty_turns = std::mem::take(&mut self.timeline_dirty_turns);
        let Some(agent_id) = self.timeline_list_agent_id.clone() else {
            return;
        };

        let item_count = self.timeline_index.agent_indices(&agent_id).len();
        let previous_count = self.timeline_list_state.item_count();
        if previous_count < item_count {
            self.timeline_list_state
                .splice(previous_count..previous_count, item_count - previous_count);
        } else if previous_count > item_count {
            self.timeline_list_state.reset(item_count);
            return;
        }

        for (dirty_agent_id, turn_id) in dirty_turns {
            if dirty_agent_id != agent_id {
                continue;
            }
            let Some(location) = self.timeline_index.location(&agent_id, &turn_id) else {
                continue;
            };
            if location.agent_index < previous_count {
                self.timeline_list_state
                    .splice(location.agent_index..location.agent_index + 1, 1);
            }
        }
    }

    pub(super) fn reset_timeline_list_to_selected(&mut self) {
        let agent_id = self.selected_agent_id.clone();
        let item_count = agent_id.as_deref().map_or(0, |agent_id| {
            self.timeline_index.agent_indices(agent_id).len()
        });
        self.timeline_list_state.reset(item_count);
        self.timeline_list_agent_id = agent_id;
        self.conversation_index_interaction.reset();
    }

    pub(super) fn scroll_selected_timeline_to_latest(&self) {
        scroll_timeline_to_latest(&self.timeline_list_state);
    }

    pub(super) fn sync_sleep_prevention(&mut self) -> anyhow::Result<()> {
        let should_prevent = self.general_preferences.prevent_sleep_while_running
            && self
                .timeline
                .iter()
                .any(|turn| turn.status == TimelineStatus::InProgress);
        match (should_prevent, self.sleep_preventer.is_some()) {
            (true, false) => {
                self.sleep_preventer = Some(sleep_prevention::SleepPreventer::start()?);
            }
            (false, true) => {
                self.sleep_preventer = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn sync_timeline_list_state(&mut self, agent_id: Option<String>) -> ListState {
        let item_count = agent_id.as_deref().map_or(0, |agent_id| {
            self.timeline_index.agent_indices(agent_id).len()
        });
        let previous_count = self.timeline_list_state.item_count();

        if self.timeline_list_agent_id != agent_id {
            self.timeline_list_state.reset(item_count);
            self.timeline_list_agent_id = agent_id;
            self.conversation_index_interaction.reset();
        } else if previous_count < item_count {
            self.timeline_list_state
                .splice(previous_count..previous_count, item_count - previous_count);
        } else if previous_count > item_count {
            self.timeline_list_state.reset(item_count);
        }

        self.timeline_list_state.clone()
    }

    fn choose_prompt_attachments(&mut self, cx: &mut Context<Self>) {
        if self.attachment_in_flight {
            self.show_warning("附件选择器正在打开", cx);
            return;
        }
        if self.prompt_attachments.len() >= MAX_PROMPT_ATTACHMENTS {
            self.show_validation_error(
                format!("每条消息最多可添加 {MAX_PROMPT_ATTACHMENTS} 个附件"),
                cx,
            );
            return;
        }
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择图片、文本或代码附件".into()),
        });
        let available_slots = MAX_PROMPT_ATTACHMENTS - self.prompt_attachments.len();
        let existing_bytes = self
            .prompt_attachments
            .iter()
            .map(|attachment| attachment.size_bytes)
            .sum();
        let selected_agent_id = self.selected_agent_id.clone();
        let background_executor = cx.background_executor().clone();
        self.attachment_in_flight = true;
        cx.notify();

        cx.spawn(async move |view, cx| {
            let selection = path_prompt.await;
            let loaded = match selection {
                Ok(Ok(Some(paths))) => Some(
                    background_executor
                        .spawn(async move {
                            load_prompt_attachments(paths, available_slots, existing_bytes)
                        })
                        .await,
                ),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => Some(Err(format!("无法打开文件选择器：{error}"))),
                Err(_) => Some(Err("文件选择器意外关闭，请重试".to_owned())),
            };
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.attachment_in_flight = false;
                if view.selected_agent_id != selected_agent_id {
                    cx.notify();
                    return;
                }
                match loaded {
                    Some(Ok(attachments)) => {
                        let count = attachments.len();
                        view.prompt_attachments.extend(attachments);
                        if count > 0 {
                            view.show_success(format!("已添加 {count} 个附件"), cx);
                        } else {
                            cx.notify();
                        }
                    }
                    Some(Err(message)) => view.show_error(message, cx),
                    None => cx.notify(),
                }
            });
        })
        .detach();
    }

    fn remove_prompt_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.prompt_attachments.len() {
            self.prompt_attachments.remove(index);
            cx.notify();
        }
    }

    fn active_turn_id(&self, agent_id: &str) -> Option<String> {
        self.timeline_index
            .agent_indices(agent_id)
            .iter()
            .rev()
            .filter_map(|index| self.timeline.get(*index))
            .find(|turn| turn.status == TimelineStatus::InProgress)
            .map(|turn| turn.turn_id.clone())
    }

    fn clear_submitted_composer(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        self.prompt_drafts.remove(agent_id);
        self.prompt_clear_agent_id = Some(agent_id.to_owned());
        self.prompt_attachments.clear();
        self.schedule_ui_state_save(cx);
    }

    fn queue_prompt(&mut self, prompt: QueuedPrompt, fallback: bool, cx: &mut Context<Self>) {
        let agent_id = prompt.agent_id.clone();
        self.queued_prompts.push_back(prompt);
        self.clear_submitted_composer(&agent_id, cx);
        let count = self
            .queued_prompts
            .iter()
            .filter(|prompt| prompt.agent_id == agent_id)
            .count();
        if fallback {
            self.show_info(
                format!("当前 Provider 或附件不支持立即调整，已安全排队 · 共 {count} 条"),
                cx,
            );
        } else {
            self.show_success(format!("消息已排队 · 共 {count} 条"), cx);
        }
    }

    fn submit_prompt_request(
        &mut self,
        prompt: QueuedPrompt,
        clear_composer: bool,
        requeue_on_error: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            if requeue_on_error {
                self.queued_prompts.push_front(prompt);
            }
            self.show_validation_error("Daemon 尚未连接", cx);
            return;
        };
        let signature = serde_json::to_string(&(&prompt.agent_id, &prompt.text, &prompt.options))
            .unwrap_or_else(|_| format!("{}:{}", prompt.agent_id, prompt.text));
        let client_mutation_id = self
            .retry_prompt
            .as_ref()
            .filter(|pending| pending.signature == signature)
            .map_or_else(
                || format!("prompt_{}", uuid::Uuid::new_v4()),
                |pending| pending.client_mutation_id.clone(),
            );
        self.retry_prompt = Some(RetryPrompt {
            signature,
            client_mutation_id: client_mutation_id.clone(),
        });
        self.prompt_in_flight = true;
        self.detail = if requeue_on_error {
            "正在发送已排队消息…".into()
        } else {
            "正在提交 Prompt…".into()
        };
        let submitted = prompt.clone();
        self.prompt_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .prompt_with_options(
                    prompt.agent_id,
                    prompt.text,
                    client_mutation_id,
                    prompt.options,
                )
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.prompt_in_flight = false;
                match result {
                    Ok(acknowledgement) => {
                        view.retry_prompt = None;
                        if clear_composer {
                            view.clear_submitted_composer(&submitted.agent_id, cx);
                        }
                        view.reset_timeline_list_to_selected();
                        view.show_success(
                            format!("Prompt 已接受 · Turn {}", acknowledgement.turn_id),
                            cx,
                        );
                    }
                    Err(error) => {
                        if requeue_on_error {
                            view.queued_prompts.push_front(submitted);
                        }
                        view.show_error(
                            format!("Prompt 提交失败：{error}；再次提交将复用 mutation ID"),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn steer_active_turn(
        &mut self,
        agent_id: String,
        turn_id: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return;
        };
        let signature = format!("steer:{agent_id}:{turn_id}:{text}");
        let client_mutation_id = self
            .retry_prompt
            .as_ref()
            .filter(|pending| pending.signature == signature)
            .map_or_else(
                || format!("steer_{}", uuid::Uuid::new_v4()),
                |pending| pending.client_mutation_id.clone(),
            );
        self.retry_prompt = Some(RetryPrompt {
            signature,
            client_mutation_id: client_mutation_id.clone(),
        });
        self.prompt_in_flight = true;
        self.detail = "正在调整当前任务…".into();
        let submitted_agent_id = agent_id.clone();
        self.prompt_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .steer(agent_id, turn_id, text, client_mutation_id)
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.prompt_in_flight = false;
                match result {
                    Ok(_) => {
                        view.retry_prompt = None;
                        view.clear_submitted_composer(&submitted_agent_id, cx);
                        view.show_success("已将消息加入当前任务", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("调整当前任务失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn start_next_queued_prompt(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        if self.prompt_in_flight || !matches!(self.state, corbit_client::ConnectionState::Online) {
            return;
        }
        let Some(index) = self
            .queued_prompts
            .iter()
            .position(|prompt| prompt.agent_id == agent_id)
        else {
            return;
        };
        let Some(prompt) = self.queued_prompts.remove(index) else {
            return;
        };
        self.submit_prompt_request(prompt, false, true, cx);
    }

    fn clear_queued_prompts(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        self.queued_prompts
            .retain(|prompt| prompt.agent_id != agent_id);
        self.show_info("已清空排队消息", cx);
    }

    pub(super) fn send_prompt(&mut self, cx: &mut Context<Self>) {
        if self.prompt_in_flight || self.attachment_in_flight || self.provider_switch_in_flight {
            self.show_warning("Prompt 正在提交或附件正在加载，请稍候", cx);
            return;
        }
        let Some(agent) = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_agent_id.as_ref()?;
            snapshot.agents.iter().find(|agent| &agent.id == selected)
        }) else {
            self.show_validation_error("请先选择一个 Agent", cx);
            return;
        };
        if agent.status != corbit_client::AgentStatus::Running {
            self.show_validation_error("只有运行中的 Agent 才能接收 Prompt", cx);
            return;
        }
        let agent_id = agent.id.clone();
        let provider = agent.provider.clone();
        let text = Self::input_value(&self.prompt_input, cx);
        if text.is_empty() && self.prompt_attachments.is_empty() {
            self.show_validation_error("请输入 Prompt 或添加附件", cx);
            return;
        }
        if !matches!(self.state, corbit_client::ConnectionState::Online) {
            self.show_validation_error("请等待 Daemon 连接完成", cx);
            return;
        }
        if let Some(message) = self.provider_prompt_blocker(&provider) {
            self.show_validation_error(message, cx);
            return;
        }

        let options = self.composer_prompt_options(&provider);
        let active_turn = self.active_turn_id(&agent_id);
        if let Some(turn_id) = active_turn {
            let can_steer = self.general_preferences.follow_up_behavior
                == FollowUpBehavior::SteerCurrent
                && provider == "codex"
                && options.attachments.is_empty()
                && !text.is_empty();
            if can_steer {
                self.steer_active_turn(agent_id, turn_id, text, cx);
            } else {
                let fallback =
                    self.general_preferences.follow_up_behavior == FollowUpBehavior::SteerCurrent;
                self.queue_prompt(
                    QueuedPrompt {
                        agent_id,
                        text,
                        options,
                    },
                    fallback,
                    cx,
                );
            }
            return;
        }
        self.submit_prompt_request(
            QueuedPrompt {
                agent_id,
                text,
                options,
            },
            true,
            false,
            cx,
        );
    }

    fn retry_turn_prompt(&mut self, prompt: String, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_input
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        self.send_prompt(cx);
    }

    fn copy_timeline_text(
        &mut self,
        text: String,
        description: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.show_success(format!("已复制{description}"), cx);
    }

    fn copy_timeline_response(&mut self, agent_id: &str, turn_id: &str, cx: &mut Context<Self>) {
        let Some(response) = self
            .timeline_index
            .location(agent_id, turn_id)
            .and_then(|location| self.timeline.get(location.timeline_index))
            .and_then(final_response)
            .map(str::to_owned)
        else {
            return;
        };

        self.copy_timeline_text(response, "回答", cx);
    }

    fn interrupt_turn(&mut self, agent_id: String, turn_id: String, cx: &mut Context<Self>) {
        if self.control_in_flight {
            self.show_warning("中断请求正在提交，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return;
        };
        let signature = format!("interrupt:{agent_id}:{turn_id}");
        let client_mutation_id = self.control_mutation_id(&signature);
        self.control_in_flight = true;
        self.detail = "正在中断 Turn…".into();
        self.control_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .interrupt(agent_id, turn_id, client_mutation_id)
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.control_in_flight = false;
                match result {
                    Ok(acknowledgement) => {
                        view.retry_control = None;
                        view.show_success(
                            format!("已请求中断 Turn {}", acknowledgement.turn_id),
                            cx,
                        );
                    }
                    Err(error) => {
                        view.show_error(
                            format!("Turn 中断失败：{error}；重试将复用 mutation ID"),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn resolve_permission(
        &mut self,
        permission: PendingPermission,
        decision: corbit_client::AgentApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        if self.control_in_flight {
            self.show_warning("权限决定正在提交，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return;
        };
        let signature = format!(
            "approval:{}:{}:{decision:?}",
            permission.agent_id, permission.approval_id
        );
        let client_mutation_id = self.control_mutation_id(&signature);
        self.control_in_flight = true;
        self.detail = "正在提交权限决定…".into();
        let affected_turn = (permission.agent_id.clone(), permission.turn_id.clone());
        self.timeline_dirty_turns.insert(affected_turn.clone());
        self.flush_timeline_list_updates();
        self.control_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .resolve_approval(
                    permission.agent_id,
                    permission.approval_id,
                    decision,
                    client_mutation_id,
                )
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.control_in_flight = false;
                match result {
                    Ok(acknowledgement) => {
                        view.retry_control = None;
                        view.permissions.retain(|permission| {
                            permission.approval_id != acknowledgement.approval_id
                        });
                        view.show_success("权限决定已提交", cx);
                    }
                    Err(error) => {
                        view.show_error(
                            format!("权限决定失败：{error}；重试将复用 mutation ID"),
                            cx,
                        );
                    }
                }
                view.timeline_dirty_turns.insert(affected_turn);
                view.flush_timeline_list_updates();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn control_mutation_id(&mut self, signature: &str) -> String {
        let client_mutation_id = self
            .retry_control
            .as_ref()
            .filter(|pending| pending.signature == signature)
            .map_or_else(
                || format!("control_{}", uuid::Uuid::new_v4()),
                |pending| pending.client_mutation_id.clone(),
            );
        self.retry_control = Some(RetryControl {
            signature: signature.to_owned(),
            client_mutation_id: client_mutation_id.clone(),
        });
        client_mutation_id
    }

    fn toggle_timeline_step(
        &mut self,
        agent_id: &str,
        turn_id: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.expanded_timeline_steps.remove(key) {
            self.expanded_timeline_steps.insert(key.to_owned());
        }
        self.timeline_dirty_turns
            .insert((agent_id.to_owned(), turn_id.to_owned()));
        self.flush_timeline_list_updates();
        cx.notify();
    }

    fn toggle_timeline_activity(
        &mut self,
        agent_id: &str,
        turn_id: &str,
        key: &str,
        automatically_expanded: bool,
        cx: &mut Context<Self>,
    ) {
        if automatically_expanded {
            if !self.collapsed_streaming_timeline_activity.remove(key) {
                self.collapsed_streaming_timeline_activity
                    .insert(key.to_owned());
            }
        } else if !self.expanded_timeline_activity.remove(key) {
            self.expanded_timeline_activity.insert(key.to_owned());
        }
        self.timeline_dirty_turns
            .insert((agent_id.to_owned(), turn_id.to_owned()));
        self.flush_timeline_list_updates();
        cx.notify();
    }

    fn timeline_activity_expansion_state(&self, turn: &TimelineTurn, key: &str) -> (bool, bool) {
        let automatically_expanded = automatically_expands_timeline_activity(turn);
        let expanded = timeline_activity_is_expanded(
            turn,
            self.expanded_timeline_activity.contains(key),
            self.collapsed_streaming_timeline_activity.contains(key),
        );
        (expanded, automatically_expanded)
    }

    fn render_step_code(label: &'static str, text: String) -> Div {
        div()
            .v_flex()
            .gap_1()
            .when(!label.is_empty(), |block| {
                block.child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(label),
                )
            })
            .child(
                div()
                    .max_h(px(260.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .bg(rgb(COLOR_EDITOR))
                    .px_3()
                    .py_2()
                    .font_family(mono_font_family())
                    .text_size(font_px(FONT_SIZE_MONO))
                    .line_height(px(19.))
                    .whitespace_nowrap()
                    .child(text)
                    .overflow_scrollbar(),
            )
    }

    #[allow(clippy::too_many_lines)]
    fn render_timeline_step(
        &self,
        turn_index: usize,
        step_index: usize,
        turn: &TimelineTurn,
        step: &TimelineStep,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("{}:{}:{}", turn.agent_id, turn.turn_id, step.item_id);
        let expanded = self.expanded_timeline_steps.contains(&key);
        if let TimelineStepKind::AssistantMessage { text } | TimelineStepKind::Reasoning { text } =
            &step.kind
        {
            return div()
                .w_full()
                .py_2()
                .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
                .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(if text.trim().is_empty() {
                    "正在分析…".to_owned()
                } else {
                    text.clone()
                });
        }
        let (icon, title, summary, copy_text) = match &step.kind {
            TimelineStepKind::AssistantMessage { text } => (
                AppIcon::Agent,
                "过程说明".to_owned(),
                text.lines().next().unwrap_or("正在处理…").to_owned(),
                text.clone(),
            ),
            TimelineStepKind::Reasoning { text } => (
                AppIcon::Agent,
                "思考过程".to_owned(),
                text.lines().next().unwrap_or("正在分析…").to_owned(),
                text.clone(),
            ),
            TimelineStepKind::Plan { steps, .. } => (
                AppIcon::Tasks,
                "执行计划".to_owned(),
                format!("{} 个步骤", steps.len()),
                steps
                    .iter()
                    .map(|step| step.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            TimelineStepKind::Command {
                command, output, ..
            } => (
                AppIcon::Terminal,
                "运行命令".to_owned(),
                command.lines().next().unwrap_or("命令执行").to_owned(),
                if output.is_empty() {
                    command.clone()
                } else {
                    format!("{command}\n\n{output}")
                },
            ),
            TimelineStepKind::FileChange { changes } => (
                AppIcon::Changes,
                "文件变更".to_owned(),
                format!("{} 个文件", changes.len()),
                changes
                    .iter()
                    .map(|change| {
                        format!(
                            "{}{}\n{}",
                            change.path,
                            change
                                .moved_to
                                .as_ref()
                                .map_or_else(String::new, |target| format!(" -> {target}")),
                            change.diff.as_deref().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
            TimelineStepKind::Diff { diff } => (
                AppIcon::Changes,
                "最终差异".to_owned(),
                format!("{} 行", diff.lines().count()),
                diff.clone(),
            ),
            TimelineStepKind::Tool {
                tool_name,
                title,
                input,
                output,
                error,
                ..
            } => (
                AppIcon::ToolCall,
                tool_display_title(title.as_deref().unwrap_or(tool_name)),
                String::new(),
                [input.as_deref(), output.as_deref(), error.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
        };
        let (status_label, status_color) = match step.status {
            corbit_client::AgentTimelineStepStatus::Pending => ("等待中", rgb(COLOR_TEXT_TERTIARY)),
            corbit_client::AgentTimelineStepStatus::InProgress => ("执行中", rgb(COLOR_SUCCESS)),
            corbit_client::AgentTimelineStepStatus::Completed => {
                ("已完成", rgb(COLOR_TEXT_TERTIARY))
            }
            corbit_client::AgentTimelineStepStatus::Failed => ("失败", rgb(COLOR_ERROR)),
            corbit_client::AgentTimelineStepStatus::Declined => ("已拒绝", rgb(COLOR_WARNING)),
        };
        let detail = match &step.kind {
            TimelineStepKind::AssistantMessage { text } => div()
                .line_height(px(20.))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(text.clone()),
            TimelineStepKind::Reasoning { text } => div()
                .line_height(px(20.))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(if text.is_empty() {
                    "正在分析…".to_owned()
                } else {
                    text.clone()
                }),
            TimelineStepKind::Plan { explanation, steps } => {
                let rows = steps
                    .iter()
                    .map(|step| {
                        let (marker, color) = match step.status {
                            corbit_client::AgentTimelineStepStatus::Completed => {
                                ("✓", rgb(COLOR_SUCCESS))
                            }
                            corbit_client::AgentTimelineStepStatus::InProgress => {
                                ("●", rgb(COLOR_SUCCESS))
                            }
                            _ => ("○", rgb(COLOR_TEXT_TERTIARY)),
                        };
                        div()
                            .h_flex()
                            .items_start()
                            .gap_2()
                            .child(div().w(px(14.)).flex_none().text_color(color).child(marker))
                            .child(div().flex_1().min_w(px(0.)).child(step.text.clone()))
                    })
                    .collect::<Vec<_>>();
                div()
                    .v_flex()
                    .gap_2()
                    .when_some(explanation.clone(), |body, explanation| {
                        body.child(
                            div()
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(explanation),
                        )
                    })
                    .children(rows)
            }
            TimelineStepKind::Command {
                command,
                cwd,
                output,
                exit_code,
                duration_ms,
            } => div()
                .v_flex()
                .gap_2()
                .when_some(cwd.clone(), |body, cwd| {
                    body.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(cwd),
                    )
                })
                .child(Self::render_step_code("命令", command.clone()))
                .when(!output.is_empty(), |body| {
                    body.child(Self::render_step_code("输出", output.clone()))
                })
                .when(exit_code.is_some() || duration_ms.is_some(), |body| {
                    let mut parts = Vec::new();
                    if let Some(exit_code) = exit_code {
                        parts.push(format!("退出码 {exit_code}"));
                    }
                    if let Some(duration_ms) = duration_ms {
                        parts.push(Self::format_duration(*duration_ms));
                    }
                    body.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(parts.join(" · ")),
                    )
                }),
            TimelineStepKind::FileChange { changes } => {
                let rows = changes
                    .iter()
                    .map(|change| {
                        let change_label = match change.change_kind {
                            corbit_client::AgentTimelineFileChangeKind::Added => "新增",
                            corbit_client::AgentTimelineFileChangeKind::Modified => "修改",
                            corbit_client::AgentTimelineFileChangeKind::Deleted => "删除",
                            corbit_client::AgentTimelineFileChangeKind::Moved => "移动",
                        };
                        div()
                            .v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Icon::new(AppIcon::File).size(px(14.)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .truncate()
                                            .font_family(mono_font_family())
                                            .text_size(font_px(FONT_SIZE_MONO))
                                            .child(change.path.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child(change_label),
                                    ),
                            )
                            .when_some(change.moved_to.clone(), |row, target| {
                                row.child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(format!("移动到 {target}")),
                                )
                            })
                            .when_some(
                                change.diff.clone().filter(|diff| !diff.is_empty()),
                                |row, diff| row.child(Self::render_step_code("差异", diff)),
                            )
                    })
                    .collect::<Vec<_>>();
                div().v_flex().gap_3().children(rows)
            }
            TimelineStepKind::Diff { diff } => Self::render_step_code("统一差异", diff.clone()),
            TimelineStepKind::Tool {
                input,
                output,
                error,
                duration_ms,
                ..
            } => div()
                .v_flex()
                .gap_2()
                .when_some(input.clone(), |body, input| {
                    body.child(Self::render_step_code("输入", input))
                })
                .when_some(output.clone(), |body, output| {
                    body.child(Self::render_step_code("结果", output))
                })
                .when_some(error.clone(), |body, error| {
                    body.child(div().text_color(rgb(COLOR_ERROR)).child(error))
                })
                .when_some(*duration_ms, |body, duration_ms| {
                    body.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(Self::format_duration(duration_ms)),
                    )
                }),
        };
        let control_id = turn_index.saturating_mul(10_000).saturating_add(step_index);
        let toggle_key = key.clone();
        let toggle_agent_id = turn.agent_id.clone();
        let toggle_turn_id = turn.turn_id.clone();
        let copy_disabled = copy_text.is_empty();

        let has_detail = !copy_text.trim().is_empty();
        let is_terminal_success = matches!(
            step.status,
            corbit_client::AgentTimelineStepStatus::Completed
        );

        div()
            .v_flex()
            .child(
                div()
                    .id(("toggle-timeline-step", control_id))
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .min_h(px(30.))
                    .rounded_md()
                    .px_1()
                    .text_size(font_px(FONT_SIZE_BASE))
                    .text_color(status_color)
                    .when(has_detail, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(rgb(COLOR_SURFACE_UNDER)))
                            .tooltip(move |window, cx| {
                                Tooltip::new(if expanded {
                                    "收起调用详情"
                                } else {
                                    "展开调用详情"
                                })
                                .build(window, cx)
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.toggle_timeline_step(
                                    &toggle_agent_id,
                                    &toggle_turn_id,
                                    &toggle_key,
                                    cx,
                                );
                            }))
                    })
                    .child(Icon::new(icon).size(px(16.)).text_color(status_color))
                    .child(
                        div()
                            .h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .truncate()
                            .child(title)
                            .when(!summary.is_empty(), |label| {
                                label.child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                                        .child(summary),
                                )
                            }),
                    )
                    .when(!is_terminal_success, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(status_color)
                                .child(status_label),
                        )
                    })
                    .when(has_detail, |row| {
                        row.child(
                            Icon::new(AppIcon::ChevronRight)
                                .size(px(13.))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .when(expanded, |icon| icon.rotate(percentage(90. / 360.))),
                        )
                    }),
            )
            .when(expanded && has_detail, |row| {
                row.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .ml(px(24.))
                        .mt_1()
                        .mb_2()
                        .max_h(px(320.))
                        .overflow_y_scrollbar()
                        .pr_3()
                        .text_size(font_px(FONT_SIZE_SM))
                        .child(detail)
                        .child(
                            div().h_flex().justify_end().child(
                                Button::new(("copy-timeline-step", control_id))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(AppIcon::Copy))
                                    .tooltip("复制调用详情")
                                    .disabled(copy_disabled)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.copy_timeline_text(copy_text.clone(), "调用详情", cx);
                                    })),
                            ),
                        ),
                )
            })
    }

    fn format_duration(duration_ms: u64) -> String {
        if duration_ms < 1_000 {
            format!("{duration_ms} ms")
        } else {
            Self::format_one_decimal(duration_ms, 100, " 秒")
        }
    }

    fn format_turn_duration(duration_ms: u64) -> String {
        let total_seconds = duration_ms / 1_000;
        let hours = total_seconds / 3_600;
        let minutes = total_seconds % 3_600 / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{hours}小时 {minutes}分钟 {seconds}秒")
        } else if minutes > 0 {
            format!("{minutes}分钟 {seconds}秒")
        } else {
            format!("{seconds}秒")
        }
    }

    fn elapsed_turn_duration_ms(started_at: Option<&str>, now: DateTime<Utc>) -> Option<u64> {
        let started_at = DateTime::parse_from_rfc3339(started_at?)
            .ok()?
            .with_timezone(&Utc);
        u64::try_from(now.signed_duration_since(started_at).num_milliseconds()).ok()
    }

    fn timeline_usage_summary(usage: &TimelineUsage) -> String {
        let mut parts = vec![
            format!("{} tokens", Self::format_token_count(usage.total_tokens)),
            format!(
                "输入 {} · 输出 {}",
                Self::format_token_count(usage.input_tokens),
                Self::format_token_count(usage.output_tokens)
            ),
        ];
        if let Some(cached) = usage.cached_input_tokens.filter(|tokens| *tokens > 0) {
            parts.push(format!("缓存 {}", Self::format_token_count(cached)));
        }
        if let Some(reasoning) = usage.reasoning_output_tokens.filter(|tokens| *tokens > 0) {
            parts.push(format!("推理 {}", Self::format_token_count(reasoning)));
        }
        if let Some(context_window) = usage.context_window.filter(|tokens| *tokens > 0) {
            let percent = context_window_percent(usage.total_tokens, context_window);
            parts.push(format!("上下文 {percent}%"));
        }
        parts.join(" · ")
    }

    fn render_timeline_activity(
        &self,
        index: usize,
        turn: &TimelineTurn,
        status: String,
        status_color: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("activity:{}:{}", turn.agent_id, turn.turn_id);
        let (expanded, automatically_expanded) = self.timeline_activity_expansion_state(turn, &key);
        let final_response_index = final_response_step(turn).map(|(index, _)| index);
        let mut activity_rows = turn
            .steps
            .iter()
            .enumerate()
            .filter(|(step_index, _)| Some(*step_index) != final_response_index)
            .map(|(step_index, step)| self.render_timeline_step(index, step_index, turn, step, cx))
            .collect::<Vec<_>>();
        if let Some(diff) = turn.diff.as_ref().filter(|diff| !diff.is_empty()) {
            let step = TimelineStep {
                item_id: "turn-diff".into(),
                status: match turn.status {
                    TimelineStatus::InProgress => {
                        corbit_client::AgentTimelineStepStatus::InProgress
                    }
                    TimelineStatus::Completed => corbit_client::AgentTimelineStepStatus::Completed,
                    TimelineStatus::Interrupted => corbit_client::AgentTimelineStepStatus::Declined,
                    TimelineStatus::Failed => corbit_client::AgentTimelineStepStatus::Failed,
                },
                kind: TimelineStepKind::Diff { diff: diff.clone() },
            };
            activity_rows.push(self.render_timeline_step(index, turn.steps.len(), turn, &step, cx));
        }
        if automatically_expanded {
            activity_rows.push(Self::render_thinking_shimmer(&turn.turn_id));
        }
        let has_activity = !activity_rows.is_empty();
        let mut activity_tooltip = if expanded {
            "收起思考与调用".to_owned()
        } else {
            "展开思考与调用".to_owned()
        };
        if let Some(usage) = &turn.usage {
            activity_tooltip.push('\n');
            activity_tooltip.push_str(&Self::timeline_usage_summary(usage));
        }
        let toggle_key = key.clone();
        let toggle_agent_id = turn.agent_id.clone();
        let toggle_turn_id = turn.turn_id.clone();

        div()
            .v_flex()
            .child(
                div()
                    .id(("toggle-timeline-activity", index))
                    .h_flex()
                    .w_full()
                    .min_h(px(32.))
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .px_1()
                    .text_size(font_px(FONT_SIZE_BASE))
                    .font_medium()
                    .text_color(status_color)
                    .when(has_activity, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(rgb(COLOR_SURFACE_UNDER)))
                            .tooltip(move |window, cx| {
                                Tooltip::new(activity_tooltip.clone()).build(window, cx)
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.toggle_timeline_activity(
                                    &toggle_agent_id,
                                    &toggle_turn_id,
                                    &toggle_key,
                                    automatically_expanded,
                                    cx,
                                );
                            }))
                    })
                    .child(status)
                    .when(has_activity, |row| {
                        row.child(
                            Icon::new(AppIcon::ChevronRight)
                                .size(px(13.))
                                .text_color(status_color)
                                .when(expanded, |icon| icon.rotate(percentage(90. / 360.))),
                        )
                    }),
            )
            .child(div().w_full().h(px(1.)).bg(rgb(COLOR_BORDER_LIGHT)))
            .when(expanded && has_activity, |activity| {
                activity.child(
                    div()
                        .id(("timeline-activity-details", index))
                        .v_flex()
                        .gap_1()
                        .max_h(px(440.))
                        .overflow_y_scrollbar()
                        .pt_3()
                        .pb_2()
                        .pr_3()
                        .children(activity_rows),
                )
            })
    }

    fn thinking_shimmer_opacity(delta: f32, glyph_index: u8) -> f32 {
        let phase = (delta - f32::from(glyph_index) * 0.16).rem_euclid(1.0);
        let direct_distance = (phase - 0.30).abs();
        let wrapped_distance = direct_distance.min(1.0 - direct_distance);
        let highlight = (1.0 - wrapped_distance / 0.18).clamp(0.0, 1.0);
        0.36 + highlight * highlight * 0.64
    }

    fn render_thinking_shimmer(turn_id: &str) -> Div {
        let glyphs = "正在思考"
            .chars()
            .enumerate()
            .map(|(glyph_index, glyph)| {
                let glyph_index =
                    u8::try_from(glyph_index).expect("thinking label has fewer than 256 glyphs");
                let animation_id: SharedString =
                    format!("thinking-shimmer-{turn_id}-{glyph_index}").into();
                div().child(glyph.to_string()).with_animation(
                    animation_id,
                    Animation::new(Duration::from_millis(1_600)).repeat(),
                    move |glyph, delta| {
                        glyph.opacity(Self::thinking_shimmer_opacity(delta, glyph_index))
                    },
                )
            })
            .collect::<Vec<_>>();

        div()
            .h_flex()
            .items_center()
            .pt_1()
            .font_medium()
            .text_size(font_px(FONT_SIZE_BASE))
            .text_color(rgb(COLOR_TEXT_SECONDARY))
            .children(glyphs)
    }

    fn format_token_count(tokens: u64) -> String {
        if tokens < 1_000 {
            tokens.to_string()
        } else if tokens < 1_000_000 {
            Self::format_one_decimal(tokens, 100, "k")
        } else {
            Self::format_one_decimal(tokens, 100_000, "m")
        }
    }

    fn format_context_token_count(tokens: u64) -> String {
        if tokens < 1_000 {
            tokens.to_string()
        } else if tokens < 1_000_000 {
            format!("{}k", tokens.saturating_add(500) / 1_000)
        } else {
            format!("{}m", tokens.saturating_add(500_000) / 1_000_000)
        }
    }

    fn format_one_decimal(value: u64, tenth: u64, suffix: &str) -> String {
        let rounded_tenths = value / tenth + u64::from(value % tenth >= tenth / 2);
        let whole = rounded_tenths / 10;
        let decimal = rounded_tenths % 10;
        if decimal == 0 {
            format!("{whole}{suffix}")
        } else {
            format!("{whole}.{decimal}{suffix}")
        }
    }

    fn latest_context_window_usage(&self, agent_id: &str) -> Option<ContextWindowUsage> {
        self.timeline_index
            .agent_indices(agent_id)
            .iter()
            .rev()
            .filter_map(|index| self.timeline.get(*index))
            .find_map(|turn| {
                let usage = turn.usage.as_ref()?;
                let context_window = usage.context_window.filter(|tokens| *tokens > 0)?;
                Some(ContextWindowUsage {
                    used_tokens: usage.total_tokens,
                    context_window,
                })
            })
    }

    fn render_context_window_usage(usage: Option<ContextWindowUsage>) -> impl IntoElement {
        let percent = usage.map_or(0, ContextWindowUsage::percent);
        let used_label = usage.map(|usage| Self::format_context_token_count(usage.used_tokens));
        let context_label =
            usage.map(|usage| Self::format_context_token_count(usage.context_window));
        let mut track_color = rgb(COLOR_TEXT_TERTIARY);
        track_color.a = 0.38;
        let progress_color = rgb(COLOR_TEXT_SECONDARY);

        div()
            .id("composer-context-window")
            .relative()
            .flex_none()
            .size(px(16.))
            .rounded_full()
            .tooltip(move |window, cx| {
                let used_label = used_label.clone();
                let context_label = context_label.clone();
                Tooltip::element(move |_, _| {
                    div()
                        .v_flex()
                        .min_w(px(224.))
                        .items_center()
                        .gap_1()
                        .py_1()
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(fixed_rgb(0x6f_6f6f))
                                .child("背景信息窗口："),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_BASE))
                                .font_medium()
                                .child(if usage.is_some() {
                                    format!("{percent}% 已用（剩余 {}%）", 100 - percent)
                                } else {
                                    "暂无使用数据".to_owned()
                                }),
                        )
                        .when_some(
                            used_label.clone().zip(context_label.clone()),
                            |tooltip, (used, context)| {
                                tooltip.child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .child(format!("已用 {used} 标记，共 {context}")),
                                )
                            },
                        )
                })
                .bg(fixed_rgb(0xf1_f1f1))
                .text_color(fixed_rgb(0x2b_2b2b))
                .border_color(fixed_rgb(0xe5_e5e5))
                .rounded(px(12.))
                .px_3()
                .py_2()
                .build(window, cx)
            })
            .child(
                canvas(
                    move |_, _, _| {},
                    move |bounds, (), window, _| {
                        let arc = Arc::new().inner_radius(5.).outer_radius(7.);
                        let track = ArcData {
                            data: &(),
                            index: 0,
                            value: 1.,
                            start_angle: 0.,
                            end_angle: std::f32::consts::TAU,
                            pad_angle: 0.,
                        };
                        arc.paint(&track, track_color, None, None, &bounds, window);

                        if percent > 0 {
                            let progress = ArcData {
                                data: &(),
                                index: 0,
                                value: f32::from(percent) / 100.,
                                start_angle: 0.,
                                end_angle: std::f32::consts::TAU * f32::from(percent) / 100.,
                                pad_angle: 0.,
                            };
                            arc.paint(&progress, progress_color, None, None, &bounds, window);
                        }
                    },
                )
                .size_full(),
            )
    }

    #[allow(clippy::too_many_lines)]
    fn render_timeline_turn(
        &self,
        index: usize,
        turn: &TimelineTurn,
        provider_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let provider_label = Self::provider_label(provider_id);
        let elapsed_duration_ms = turn.duration_ms.or_else(|| {
            (turn.status == TimelineStatus::InProgress)
                .then(|| Self::elapsed_turn_duration_ms(turn.started_at.as_deref(), Utc::now()))
                .flatten()
        });
        let elapsed_duration = elapsed_duration_ms.map(Self::format_turn_duration);
        let status = match turn.status {
            TimelineStatus::InProgress | TimelineStatus::Completed => elapsed_duration.map_or_else(
                || "已处理".to_owned(),
                |duration| format!("已处理 {duration}"),
            ),
            TimelineStatus::Interrupted => elapsed_duration.map_or_else(
                || "已中断".to_owned(),
                |duration| format!("已中断 · {duration}"),
            ),
            TimelineStatus::Failed => elapsed_duration.map_or_else(
                || "处理失败".to_owned(),
                |duration| format!("处理失败 · {duration}"),
            ),
        };
        let status_color = match turn.status {
            TimelineStatus::InProgress | TimelineStatus::Completed => rgb(COLOR_TEXT_TERTIARY),
            TimelineStatus::Interrupted => rgb(COLOR_WARNING),
            TimelineStatus::Failed => rgb(COLOR_ERROR),
        };
        let prompt = if turn.prompt.is_empty() {
            "Prompt 正在同步…".to_owned()
        } else {
            turn.prompt.clone()
        };
        let prompt_to_copy = turn.prompt.clone();
        let response_text = final_response(turn).unwrap_or_default();
        let show_response_footer = shows_completed_response_footer(turn);
        let response_usage_summary = turn.usage.as_ref().map(Self::timeline_usage_summary);
        let response_agent_id = turn.agent_id.clone();
        let response_turn_id = turn.turn_id.clone();
        let retry_prompt = turn.prompt.clone();
        let prompt_group: SharedString = format!("timeline-prompt-{index}").into();
        let is_in_progress = turn.status == TimelineStatus::InProgress;
        let activity = self.render_timeline_activity(index, turn, status, status_color, cx);
        let response = if response_text.is_empty() && is_in_progress {
            None
        } else if response_text.is_empty() {
            Some(
                div()
                    .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
                    .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(format!("{provider_label} 未返回文本内容"))
                    .into_any_element(),
            )
        } else if is_in_progress {
            let (preview, is_truncated) = streaming_response_preview(response_text);
            Some(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .when(is_truncated, |response| {
                        response.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child("回复较长，生成期间仅渲染最近内容；完成后显示全文"),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
                            .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT))
                            .child(preview.to_owned()),
                    )
                    .into_any_element(),
            )
        } else {
            let response = TextView::markdown(
                SharedString::from(format!("timeline-response-{}", turn.turn_id)),
                response_text.to_owned(),
                window,
                cx,
            )
            .style(conversation_markdown_style())
            .selectable(true)
            .w_full()
            .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
            .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT));
            let response = if should_virtualize_response(response_text) {
                response.scrollable(true).h(px(LONG_RESPONSE_VIEW_HEIGHT))
            } else {
                response
            };
            Some(response.into_any_element())
        };
        div()
            .v_flex()
            .gap_5()
            .py_6()
            .child(
                div().h_flex().w_full().justify_end().child(
                    div()
                        .relative()
                        .group(prompt_group.clone())
                        .max_w(px(620.))
                        .rounded(px(15.))
                        .bg(rgb(COLOR_EDITOR))
                        .px_4()
                        .py_3()
                        .text_size(font_px(FONT_SIZE_BASE))
                        .line_height(px(23.))
                        .child(div().pr_7().child(prompt))
                        .child(
                            div()
                                .absolute()
                                .top_1()
                                .right_1()
                                .invisible()
                                .group_hover(prompt_group, gpui::Styled::visible)
                                .child(
                                    Button::new(("copy-turn-prompt", index))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(AppIcon::Copy))
                                        .tooltip("复制问题")
                                        .disabled(prompt_to_copy.is_empty())
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.copy_timeline_text(
                                                prompt_to_copy.clone(),
                                                "问题",
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .relative()
                    .v_flex()
                    .gap_4()
                    .child(activity)
                    .when_some(response, gpui::ParentElement::child)
                    .when(show_response_footer, |conversation| {
                        conversation.child(
                            div()
                                .h_flex()
                                .w_full()
                                .items_center()
                                .gap_2()
                                .min_h(px(24.))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(
                                    Button::new(("copy-turn-response", index))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(AppIcon::Copy))
                                        .tooltip("复制回答")
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.copy_timeline_response(
                                                &response_agent_id,
                                                &response_turn_id,
                                                cx,
                                            );
                                        })),
                                )
                                .when_some(response_usage_summary, |footer, usage| {
                                    footer.child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .line_height(px(18.))
                                            .child(usage),
                                    )
                                }),
                        )
                    }),
            )
            .when_some(turn.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_ERROR))
                        .child(format!("错误：{error}")),
                )
            })
            .when(
                matches!(
                    turn.status,
                    TimelineStatus::Failed | TimelineStatus::Interrupted
                ) && !retry_prompt.is_empty(),
                |panel| {
                    panel.child(
                        Button::new(("retry-turn", index))
                            .outline()
                            .small()
                            .icon(Icon::new(AppIcon::Refresh))
                            .label("重新发送")
                            .disabled(self.prompt_in_flight)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.retry_turn_prompt(retry_prompt.clone(), window, cx);
                            })),
                    )
                },
            )
    }

    pub(super) fn render_permissions_panel(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let permissions = self
            .permissions
            .iter()
            .enumerate()
            .map(|(index, permission)| {
                let agent_title = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        snapshot
                            .agents
                            .iter()
                            .find(|agent| agent.id == permission.agent_id)
                    })
                    .map_or("未知任务", |agent| agent.title.as_str());
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .font_medium()
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(agent_title.to_owned()),
                    )
                    .child(Self::render_permission(
                        index,
                        permission,
                        self.control_in_flight || !is_online,
                        cx,
                    ))
            })
            .collect::<Vec<_>>();
        let has_permissions = !permissions.is_empty();

        div().size_full().child(
            div().size_full().overflow_y_scrollbar().child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(content_max_width())
                    .mx_auto()
                    .px_8()
                    .py_8()
                    .gap_5()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_HEADING))
                                    .font_semibold()
                                    .child("审批"),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("集中处理所有 Agent 的命令执行和文件修改请求。"),
                            ),
                    )
                    .when(!has_permissions, |panel| {
                        panel.child(
                            div()
                                .v_flex()
                                .items_center()
                                .gap_3()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_BORDER))
                                .bg(rgb(COLOR_SURFACE_UNDER))
                                .p_8()
                                .child(Icon::new(AppIcon::Success).text_color(rgb(COLOR_SUCCESS)))
                                .child(div().font_medium().child("没有待处理审批"))
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child("Agent 请求权限时会立即显示在这里。"),
                                ),
                        )
                    })
                    .children(permissions),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    fn render_permission(
        index: usize,
        permission: &PendingPermission,
        control_in_flight: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let permission_label = permission_kind_label(&permission.permission_kind);
        let permission_question = permission_question(permission);

        div()
            .v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(rgb(COLOR_BORDER_HEAVY))
            .bg(rgb(COLOR_SURFACE_UNDER))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(AppIcon::Approval)
                                    .size(px(16.))
                                    .text_color(rgb(COLOR_WARNING)),
                            )
                            .child(div().font_semibold().child(permission_label)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(COLOR_SURFACE_SECONDARY))
                            .px_2()
                            .py_1()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child("需要确认"),
                    ),
            )
            .child(
                div()
                    .line_height(px(21.))
                    .text_color(rgb(COLOR_TEXT))
                    .child(permission_question),
            )
            .when_some(permission.command.clone(), |panel, command| {
                panel.child(Self::render_permission_command(command))
            })
            .when_some(permission.cwd.clone(), |panel, cwd| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(format!("工作目录：{cwd}")),
                )
            })
            .when_some(permission.grant_root.clone(), |panel, grant_root| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(format!("授权目录：{grant_root}")),
                )
            })
            .child(Self::render_permission_actions(
                "panel",
                index,
                permission,
                control_in_flight,
                cx,
            ))
    }

    fn render_composer_permission(
        index: usize,
        permission: &PendingPermission,
        control_in_flight: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let permission_label = permission_kind_label(&permission.permission_kind);
        let permission_question = permission_question(permission);

        div()
            .v_flex()
            .w_full()
            .gap_3()
            .rounded(px(16.))
            .border_1()
            .border_color(rgb(COLOR_BORDER_HEAVY))
            .bg(rgb(COLOR_SURFACE_UNDER))
            .px_4()
            .py_3()
            .shadow_sm()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .v_flex()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .font_medium()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child(
                                        Icon::new(AppIcon::Approval)
                                            .size(px(14.))
                                            .text_color(rgb(COLOR_WARNING)),
                                    )
                                    .child(permission_label),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_BASE))
                                    .font_semibold()
                                    .line_height(px(22.))
                                    .child(permission_question),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(COLOR_SURFACE_SECONDARY))
                            .px_2()
                            .py_1()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child("需要确认"),
                    ),
            )
            .when_some(permission.command.clone(), |card, command| {
                card.child(Self::render_permission_command(command))
            })
            .when_some(permission.cwd.clone(), |card, cwd| {
                card.child(
                    div()
                        .truncate()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(format!("工作目录：{cwd}")),
                )
            })
            .when_some(permission.grant_root.clone(), |card, grant_root| {
                card.child(
                    div()
                        .truncate()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(format!("授权目录：{grant_root}")),
                )
            })
            .child(Self::render_permission_actions(
                "composer",
                index,
                permission,
                control_in_flight,
                cx,
            ))
    }

    fn render_permission_command(command: String) -> Div {
        div()
            .v_flex()
            .gap_1()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .bg(rgb(COLOR_EDITOR))
            .px_3()
            .py_2()
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                    .child("命令"),
            )
            .child(
                div()
                    .font_family(mono_font_family())
                    .text_size(font_px(FONT_SIZE_MONO))
                    .line_height(px(20.))
                    .text_color(rgb(COLOR_TEXT))
                    .child(command),
            )
    }

    fn render_permission_actions(
        surface: &'static str,
        index: usize,
        permission: &PendingPermission,
        control_in_flight: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let can_accept = permission
            .available_decisions
            .contains(&corbit_client::AgentApprovalDecision::Accept);
        let can_accept_for_session = permission
            .available_decisions
            .contains(&corbit_client::AgentApprovalDecision::AcceptForSession);
        let can_decline = permission
            .available_decisions
            .contains(&corbit_client::AgentApprovalDecision::Decline);
        let can_cancel = permission
            .available_decisions
            .contains(&corbit_client::AgentApprovalDecision::Cancel);
        let accept_permission = permission.clone();
        let session_permission = permission.clone();
        let decline_permission = permission.clone();
        let cancel_permission = permission.clone();

        div()
            .h_flex()
            .flex_wrap()
            .gap_2()
            .when(can_accept, |row| {
                row.child(
                    Button::new(SharedString::from(format!(
                        "{surface}-permission-accept-{index}"
                    )))
                    .primary()
                    .small()
                    .label("允许一次")
                    .loading(control_in_flight)
                    .disabled(control_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.resolve_permission(
                            accept_permission.clone(),
                            corbit_client::AgentApprovalDecision::Accept,
                            cx,
                        );
                    })),
                )
            })
            .when(can_accept_for_session, |row| {
                row.child(
                    Button::new(SharedString::from(format!(
                        "{surface}-permission-accept-session-{index}"
                    )))
                    .outline()
                    .small()
                    .label("本次任务允许")
                    .disabled(control_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.resolve_permission(
                            session_permission.clone(),
                            corbit_client::AgentApprovalDecision::AcceptForSession,
                            cx,
                        );
                    })),
                )
            })
            .when(can_decline, |row| {
                row.child(
                    Button::new(SharedString::from(format!(
                        "{surface}-permission-decline-{index}"
                    )))
                    .outline()
                    .small()
                    .label("拒绝")
                    .disabled(control_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.resolve_permission(
                            decline_permission.clone(),
                            corbit_client::AgentApprovalDecision::Decline,
                            cx,
                        );
                    })),
                )
            })
            .when(can_cancel, |row| {
                row.child(
                    Button::new(SharedString::from(format!(
                        "{surface}-permission-cancel-{index}"
                    )))
                    .outline()
                    .small()
                    .label("拒绝并停止")
                    .disabled(control_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.resolve_permission(
                            cancel_permission.clone(),
                            corbit_client::AgentApprovalDecision::Cancel,
                            cx,
                        );
                    })),
                )
            })
    }

    fn render_timeline_list_item(
        &self,
        agent_id: &str,
        display_index: usize,
        fallback_provider_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(&timeline_index) = self
            .timeline_index
            .agent_indices(agent_id)
            .get(display_index)
        else {
            return div().into_any_element();
        };
        let Some(turn) = self.timeline.get(timeline_index) else {
            return div().into_any_element();
        };
        let provider_id = turn
            .provider
            .as_deref()
            .filter(|provider| !provider.is_empty())
            .unwrap_or(fallback_provider_id);
        let rendered_turn = self.render_timeline_turn(display_index, turn, provider_id, window, cx);

        div()
            .w_full()
            .px_8()
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(content_max_width())
                    .mx_auto()
                    .child(rendered_turn),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_conversation_index(
        &self,
        turn_count: usize,
        timeline_list_state: &ListState,
        cx: &mut Context<Self>,
    ) -> Div {
        let entries = conversation_index_entries(turn_count);
        let active_turn = timeline_list_state
            .logical_scroll_top()
            .item_ix
            .min(turn_count.saturating_sub(1));
        let active_entry = closest_conversation_index_entry(&entries, active_turn);
        let interaction = self.conversation_index_interaction;

        div()
            .absolute()
            .left_1()
            .top_0()
            .bottom_0()
            .w(px(28.))
            .v_flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("conversation-index")
                    .v_flex()
                    .w(px(28.))
                    .items_center()
                    .gap_1()
                    .py_1()
                    .on_hover(cx.listener(|view, hovered, _, cx| {
                        if view.conversation_index_interaction.set_hovered(*hovered) {
                            cx.notify();
                        }
                    }))
                    .children(entries.into_iter().enumerate().map(
                        |(marker_slot, display_index)| {
                            let is_active = active_entry == Some(display_index);
                            let base_color = if is_active {
                                rgb(COLOR_TEXT_SECONDARY)
                            } else {
                                rgb(COLOR_BORDER_HEAVY)
                            };
                            let accent_color = rgb(COLOR_TEXT);
                            let from_metrics = conversation_index_marker_metrics(
                                is_active,
                                marker_slot,
                                interaction.from_slot,
                            );
                            let to_metrics = conversation_index_marker_metrics(
                                is_active,
                                marker_slot,
                                interaction.to_slot,
                            );
                            let list_state = timeline_list_state.clone();
                            let tooltip = format!("跳转到第 {} 轮对话", display_index + 1);
                            let animation_id: SharedString = format!(
                                "conversation-index-marker-{marker_slot}-{}",
                                interaction.animation_generation
                            )
                            .into();

                            div()
                                .id(("conversation-index-turn", display_index))
                                .h_flex()
                                .w(px(24.))
                                .h(px(10.))
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(COLOR_SURFACE_SECONDARY)))
                                .on_hover(cx.listener(move |view, hovered, _, cx| {
                                    if *hovered
                                        && view
                                            .conversation_index_interaction
                                            .focus_slot(marker_slot)
                                    {
                                        cx.notify();
                                    }
                                }))
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tooltip.clone()).build(window, cx)
                                })
                                .child(
                                    div().h(px(2.)).rounded_full().with_animation(
                                        animation_id,
                                        Animation::new(CONVERSATION_INDEX_ANIMATION_DURATION)
                                            .with_easing(ease_out_quint()),
                                        move |marker, delta| {
                                            let width = from_metrics.width
                                                + (to_metrics.width - from_metrics.width) * delta;
                                            let emphasis = from_metrics.emphasis
                                                + (to_metrics.emphasis - from_metrics.emphasis)
                                                    * delta;
                                            marker.w(px(width)).bg(interpolate_rgba(
                                                base_color,
                                                accent_color,
                                                emphasis,
                                            ))
                                        },
                                    ),
                                )
                                .on_click(cx.listener(move |_view, _, _, cx| {
                                    list_state.scroll_to(gpui::ListOffset {
                                        item_ix: display_index,
                                        offset_in_item: px(0.),
                                    });
                                    cx.notify();
                                }))
                        },
                    )),
            )
    }

    fn timeline_floating_control() -> Div {
        div()
            .h_flex()
            .size(px(TIMELINE_FLOATING_CONTROL_SIZE))
            .items_center()
            .justify_center()
            .occlude()
            .rounded_full()
            .border_1()
            .border_color(rgb(COLOR_BORDER_HEAVY))
            .bg(rgb(COLOR_SURFACE_SECONDARY))
            .shadow_md()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(COLOR_EDITOR)))
    }

    fn render_active_turn_indicator(
        agent_id: &str,
        turn_id: &str,
        timeline_list_state: &ListState,
        cx: &mut Context<Self>,
    ) -> Div {
        let indicator_id: SharedString =
            format!("active-turn-indicator-{agent_id}-{turn_id}").into();
        let dots = (0_u8..3).map(|dot_index| {
            let animation_id: SharedString =
                format!("active-turn-indicator-dot-{agent_id}-{turn_id}-{dot_index}").into();

            div()
                .size(px(5.))
                .rounded_full()
                .bg(rgb(COLOR_TEXT_SECONDARY))
                .with_animation(
                    animation_id,
                    Animation::new(ACTIVE_TURN_INDICATOR_ANIMATION_DURATION).repeat(),
                    move |dot, delta| {
                        dot.opacity(active_turn_indicator_dot_opacity(delta, dot_index))
                    },
                )
        });
        let list_state = timeline_list_state.clone();

        div()
            .absolute()
            .top(px(-32.))
            .left_0()
            .right_0()
            .h_flex()
            .justify_center()
            .child(
                Self::timeline_floating_control()
                    .id(indicator_id)
                    .active(|style| style.bg(rgb(COLOR_BORDER_LIGHT)))
                    .gap_1()
                    .tooltip(|window, cx| {
                        Tooltip::new("任务进行中 · 点击查看最新内容").build(window, cx)
                    })
                    .children(dots)
                    .on_click(cx.listener(move |_view, _, _, cx| {
                        scroll_timeline_to_latest(&list_state);
                        cx.notify();
                    })),
            )
    }

    fn render_jump_to_latest_indicator(
        timeline_list_state: &ListState,
        cx: &mut Context<Self>,
    ) -> Div {
        let list_state = timeline_list_state.clone();

        div()
            .absolute()
            .top(px(-32.))
            .left_0()
            .right_0()
            .h_flex()
            .justify_center()
            .child(
                Self::timeline_floating_control()
                    .id("timeline-jump-to-latest")
                    .active(|style| style.bg(rgb(COLOR_BORDER_LIGHT)))
                    .tooltip(|window, cx| Tooltip::new("滚动到最新内容").build(window, cx))
                    .child(
                        Icon::new(AppIcon::ScrollToLatest)
                            .size(px(20.))
                            .text_color(rgb(COLOR_TEXT_SECONDARY)),
                    )
                    .on_click(cx.listener(move |_view, _, _, cx| {
                        scroll_timeline_to_latest(&list_state);
                        cx.notify();
                    })),
            )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_timeline_panel(
        &mut self,
        is_online: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected_agent_id = self.selected_agent_id.clone();
        let selected_agent = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = selected_agent_id.as_ref()?;
            snapshot.agents.iter().find(|agent| &agent.id == selected)
        });
        let conversation_permissions = self
            .permissions
            .iter()
            .enumerate()
            .filter(|(_, permission)| {
                permission_belongs_to_agent(permission, selected_agent_id.as_deref())
            })
            .map(|(index, permission)| (index, permission.clone()))
            .collect::<Vec<_>>();
        let has_conversation_permissions = !conversation_permissions.is_empty();
        let selected_agent_exists = selected_agent.is_some();
        let selected_agent_running =
            selected_agent.is_some_and(|agent| agent.status == corbit_client::AgentStatus::Running);
        let provider_id = selected_agent
            .map(|agent| agent.provider.clone())
            .unwrap_or_default();
        let provider_prompt_blocker = (!provider_id.is_empty())
            .then(|| self.provider_prompt_blocker(&provider_id))
            .flatten();
        let active_turn = selected_agent_id.as_deref().and_then(|agent_id| {
            self.timeline_index
                .agent_indices(agent_id)
                .iter()
                .rev()
                .filter_map(|index| self.timeline.get(*index))
                .find(|turn| turn.status == TimelineStatus::InProgress)
                .map(|turn| (turn.agent_id.clone(), turn.turn_id.clone()))
        });
        let can_prompt = is_online
            && selected_agent_running
            && !self.prompt_in_flight
            && !self.attachment_in_flight
            && !self.provider_switch_in_flight
            && provider_prompt_blocker.is_none();
        let turn_count = selected_agent_id.as_deref().map_or(0, |agent_id| {
            self.timeline_index.agent_indices(agent_id).len()
        });
        let has_turns = turn_count > 0;
        let empty_title = if selected_agent_exists {
            "开始一个新任务"
        } else {
            "开始一个任务"
        };
        let empty_description = if selected_agent_exists {
            "在下方输入任务目标。Corbit 会持续展示 Agent 的响应、权限请求与执行状态。"
        } else {
            "选择项目并描述目标后，Corbit 会自动创建 Agent 并进入对话。"
        };
        let prompt_hint = if !selected_agent_exists {
            "请先使用“新建任务”开始任务".to_owned()
        } else if !is_online {
            "Daemon 当前未连接".to_owned()
        } else if !selected_agent_running {
            "请先在工作区管理中启动 Agent".to_owned()
        } else if active_turn.is_some() {
            match self.general_preferences.follow_up_behavior {
                FollowUpBehavior::SteerCurrent => "回车可立即调整当前任务".to_owned(),
                FollowUpBehavior::QueueNext => "回车会将消息排队到下一轮".to_owned(),
            }
        } else if let Some(message) = &provider_prompt_blocker {
            message.clone()
        } else {
            "发送内容将创建一个新的 Turn".to_owned()
        };
        let action_variant = composer_action_variant(cx);
        let composer_action = if let Some((agent_id, turn_id)) = active_turn.clone() {
            Button::new("interrupt-turn")
                .custom(action_variant)
                .with_size(px(30.))
                .rounded(px(15.))
                .icon(Icon::new(AppIcon::Stop))
                .tooltip("停止处理")
                .loading(self.control_in_flight)
                .disabled(!is_online || self.control_in_flight)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.interrupt_turn(agent_id.clone(), turn_id.clone(), cx);
                }))
        } else {
            Button::new("send-prompt")
                .custom(action_variant)
                .with_size(px(30.))
                .rounded(px(15.))
                .icon(Icon::new(AppIcon::Send))
                .tooltip("发送问题 · 回车")
                .loading(self.prompt_in_flight)
                .disabled(!can_prompt)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.send_prompt(cx);
                }))
        };
        let supports_turn_options = provider_supports_turn_options(&provider_id);
        let provider_choices = self.provider_options();
        let provider_view = cx.entity();
        let selected_provider = provider_id.clone();
        let option_variant = composer_option_variant(cx);
        let provider_button = Button::new("composer-provider")
            .custom(option_variant)
            .xsmall()
            .rounded(px(10.))
            .child(provider_badge(&provider_id, ProviderBadgeSize::Inline))
            .child(Self::provider_label(&provider_id).to_owned())
            .dropdown_caret(true)
            .tooltip("切换当前项目的提供商")
            .loading(self.provider_switch_in_flight)
            .disabled(
                !is_online
                    || !selected_agent_running
                    || self.provider_switch_in_flight
                    || active_turn.is_some()
                    || provider_choices.is_empty(),
            )
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(180.));
                for (provider, label, description) in provider_choices.clone() {
                    let item_view = provider_view.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .v_flex()
                                .min_w_0()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .font_medium()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(description),
                                )
                        })
                        .checked(selected_provider == provider)
                        .on_click(move |_, _, cx| {
                            item_view.update(cx, |view, cx| {
                                view.switch_conversation_provider(provider, cx);
                            });
                        }),
                    );
                }
                menu
            });
        let selected_model = self.composer_model_info(&provider_id).cloned();
        let model_label = selected_model.as_ref().map_or_else(
            || {
                if provider_prompt_blocker.is_some() {
                    "Provider 不可用".to_owned()
                } else {
                    "Provider 默认".to_owned()
                }
            },
            |model| model_display_name(&model.id, &model.display_name),
        );
        let model_choices = self
            .provider_catalog_entry(&provider_id)
            .map(|provider| provider.models.clone())
            .unwrap_or_default();
        let selected_model_for_menu = selected_model.clone();
        let model_view = cx.entity();
        let model_provider = provider_id.clone();
        let model_button = Button::new("composer-model")
            .custom(option_variant)
            .xsmall()
            .rounded(px(10.))
            .label(model_label)
            .tooltip(if supports_turn_options {
                "选择模型"
            } else {
                "当前 Provider 使用自己的默认模型"
            })
            .disabled(!can_prompt || !supports_turn_options || model_choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(260.)).max_w(px(420.));
                for model in model_choices.clone() {
                    let item_view = model_view.clone();
                    let item_provider = model_provider.clone();
                    let item_model = model.id.clone();
                    let checked = selected_model_for_menu
                        .as_ref()
                        .is_some_and(|selected| selected.id == model.id);
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div().v_flex().min_w_0().py_1().child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .child(model_display_name(&model.id, &model.display_name)),
                            )
                        })
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            item_view.update(cx, |view, cx| {
                                view.choose_composer_model(&item_provider, &item_model, cx);
                            });
                        }),
                    );
                }
                menu
            });

        let permission_mode = self.composer_permission_mode;
        let permission_view = cx.entity();
        let permission_copy = permission_mode_copy(permission_mode);
        let permission_button = Button::new("composer-permission")
            .custom(option_variant)
            .xsmall()
            .rounded(px(10.))
            .icon(Icon::new(AppIcon::Approval))
            .label(permission_copy.title)
            .dropdown_caret(true)
            .tooltip(if supports_turn_options {
                "更改权限"
            } else {
                "当前 Provider 管理自己的权限模式"
            })
            .disabled(!can_prompt || !supports_turn_options)
            .dropdown_menu(move |menu, _, _| {
                let choices = [
                    corbit_client::AgentPermissionMode::ReadOnly,
                    corbit_client::AgentPermissionMode::WorkspaceWrite,
                    corbit_client::AgentPermissionMode::FullAccess,
                ];
                let mut menu = menu.min_w(px(360.)).max_w(px(420.));
                for mode in choices {
                    let item_view = permission_view.clone();
                    let copy = permission_mode_copy(mode);
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w_0()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .font_medium()
                                        .child(copy.title),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(copy.description),
                                )
                        })
                        .checked(mode == permission_mode)
                        .on_click(move |_, _, cx| {
                            item_view.update(cx, |view, cx| {
                                view.composer_permission_mode = mode;
                                view.schedule_ui_state_save(cx);
                                cx.notify();
                            });
                        }),
                    );
                }
                menu
            });

        let reasoning_effort = self.composer_reasoning_effort(&provider_id);
        let reasoning_choices = selected_model
            .as_ref()
            .map(|model| model.supported_reasoning_efforts.clone())
            .unwrap_or_default();
        let reasoning_view = cx.entity();
        let reasoning_provider = provider_id.clone();
        let reasoning_button = Button::new("composer-reasoning")
            .custom(option_variant)
            .xsmall()
            .rounded(px(10.))
            .label(reasoning_effort.map_or("默认", reasoning_effort_short_label))
            .tooltip(if supports_turn_options {
                "选择推理等级"
            } else {
                "当前 Provider 管理自己的推理程度"
            })
            .disabled(!can_prompt || !supports_turn_options || reasoning_choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(240.)).max_w(px(360.));
                for choice in reasoning_choices.clone() {
                    let effort = choice.reasoning_effort;
                    let item_view = reasoning_view.clone();
                    let item_provider = reasoning_provider.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div().v_flex().min_w_0().py_1().child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .child(reasoning_effort_short_label(effort)),
                            )
                        })
                        .checked(reasoning_effort == Some(effort))
                        .on_click(move |_, _, cx| {
                            item_view.update(cx, |view, cx| {
                                if let (Some(agent_id), Some(entry)) = (
                                    view.selected_agent_id.as_deref(),
                                    view.provider_catalog_entry(&item_provider).cloned(),
                                ) && view
                                    .composer_selections
                                    .choose_reasoning_effort(agent_id, &entry, effort)
                                {
                                    view.schedule_ui_state_save(cx);
                                }
                                cx.notify();
                            });
                        }),
                    );
                }
                menu
            });
        let attachment_button = Button::new("composer-add-attachment")
            .ghost()
            .xsmall()
            .icon(Icon::new(AppIcon::Add))
            .tooltip("添加图片、文本或代码附件")
            .loading(self.attachment_in_flight)
            .disabled(
                !can_prompt
                    || self.attachment_in_flight
                    || self.prompt_attachments.len() >= MAX_PROMPT_ATTACHMENTS,
            )
            .on_click(cx.listener(|view, _, _, cx| {
                view.choose_prompt_attachments(cx);
            }));
        let attachment_chips = self
            .prompt_attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let name = attachment.upload.name.clone();
                div()
                    .h_flex()
                    .max_w(px(260.))
                    .items_center()
                    .gap_1()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(COLOR_BORDER))
                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                    .pl_2()
                    .pr_1()
                    .py_1()
                    .child(Icon::new(AppIcon::File).size(px(14.)))
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(font_px(FONT_SIZE_XS))
                            .child(format!(
                                "{} · {}",
                                name,
                                attachment_size_label(attachment.size_bytes)
                            )),
                    )
                    .child(
                        Button::new(("remove-prompt-attachment", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(AppIcon::Close).size(px(12.)))
                            .tooltip("移除附件")
                            .disabled(self.prompt_in_flight)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.remove_prompt_attachment(index, cx);
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let composer_permissions = conversation_permissions
            .iter()
            .map(|(index, permission)| {
                Self::render_composer_permission(
                    *index,
                    permission,
                    self.control_in_flight || !is_online,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let queued_prompt_count = selected_agent_id.as_deref().map_or(0, |agent_id| {
            self.queued_prompts
                .iter()
                .filter(|prompt| prompt.agent_id == agent_id)
                .count()
        });
        let queued_prompt_agent_id = selected_agent_id.clone().unwrap_or_default();
        let recovery_banner = self
            .new_task_recovery
            .as_ref()
            .filter(|recovery| recovery.agent_id() == selected_agent_id.as_deref())
            .and_then(|_| self.render_new_task_recovery_banner(is_online, cx));
        let has_recovery_banner = recovery_banner.is_some();
        let context_window_usage = selected_agent_id
            .as_deref()
            .and_then(|agent_id| self.latest_context_window_usage(agent_id));
        let context_window_indicator =
            selected_agent_exists.then(|| Self::render_context_window_usage(context_window_usage));
        let timeline_list_state = self.sync_timeline_list_state(selected_agent_id.clone());
        let timeline_floating_control = if has_conversation_permissions {
            None
        } else if let Some((agent_id, turn_id)) = active_turn.as_ref() {
            Some(Self::render_active_turn_indicator(
                agent_id,
                turn_id,
                &timeline_list_state,
                cx,
            ))
        } else if timeline_is_away_from_latest(&timeline_list_state) {
            Some(Self::render_jump_to_latest_indicator(
                &timeline_list_state,
                cx,
            ))
        } else {
            None
        };
        let list_agent_id = selected_agent_id.clone();
        let list_provider_id = provider_id.clone();
        let virtual_timeline = list(
            timeline_list_state.clone(),
            cx.processor(move |view, display_index, window, cx| {
                let Some(agent_id) = list_agent_id.as_deref() else {
                    return div().into_any_element();
                };
                view.render_timeline_list_item(
                    agent_id,
                    display_index,
                    &list_provider_id,
                    window,
                    cx,
                )
            }),
        )
        .size_full()
        .pb_6();
        let virtual_timeline = if has_recovery_banner {
            virtual_timeline.pt_2()
        } else {
            virtual_timeline.pt_8()
        };

        div()
            .v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(rgb(COLOR_SURFACE))
            .child(
                div()
                    .id("conversation-timeline-scroll")
                    .v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .when_some(recovery_banner, |conversation, banner| {
                        conversation.child(
                            div().w_full().px_8().pt_6().child(
                                div()
                                    .w_full()
                                    .max_w(content_max_width())
                                    .mx_auto()
                                    .child(banner),
                            ),
                        )
                    })
                    .when(!has_turns, |conversation| {
                        conversation.child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_h(px(0.))
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .px_3()
                                .text_center()
                                .child(
                                    div()
                                        .h_flex()
                                        .size(px(42.))
                                        .items_center()
                                        .justify_center()
                                        .rounded_lg()
                                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                                        .child(
                                            Icon::new(AppIcon::Terminal)
                                                .text_color(rgb(COLOR_TEXT_SECONDARY)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_HEADING))
                                        .font_semibold()
                                        .child(empty_title),
                                )
                                .child(
                                    div()
                                        .max_w(px(460.))
                                        .line_height(px(21.))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(empty_description),
                                ),
                        )
                    })
                    .when(has_turns, |conversation| {
                        conversation.child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h(px(0.))
                                .w_full()
                                .child(virtual_timeline)
                                .child(self.render_conversation_index(
                                    turn_count,
                                    &timeline_list_state,
                                    cx,
                                ))
                                .vertical_scrollbar(&timeline_list_state),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .v_flex()
                    .w_full()
                    .flex_none()
                    .bg(rgb(COLOR_SURFACE))
                    .px_8()
                    .pt_3()
                    .pb_4()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .max_w(content_max_width())
                            .mx_auto()
                            .gap_2()
                            .children(composer_permissions)
                            .when(queued_prompt_count > 0, |composer| {
                                composer.child(
                                    div()
                                        .h_flex()
                                        .w_full()
                                        .items_center()
                                        .justify_between()
                                        .rounded(px(10.))
                                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                                        .px_3()
                                        .py_2()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(format!(
                                            "已排队 {queued_prompt_count} 条消息，将在当前任务结束后发送"
                                        ))
                                        .child(
                                            Button::new("clear-queued-prompts")
                                                .ghost()
                                                .xsmall()
                                                .label("清空")
                                                .on_click(cx.listener(move |view, _, _, cx| {
                                                    view.clear_queued_prompts(
                                                        &queued_prompt_agent_id,
                                                        cx,
                                                    );
                                                })),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .v_flex()
                                    .w_full()
                                    .min_h(px(116.))
                                    .rounded(px(20.))
                                    .border_1()
                                    .border_color(rgb(COLOR_BORDER_HEAVY))
                                    .bg(rgb(COLOR_EDITOR))
                                    .px_2()
                                    .pt_2()
                                    .pb_2()
                                    .shadow_sm()
                                    .child(
                                        div().w_full().flex_1().min_h(px(60.)).child(
                                            Input::new(&self.prompt_input)
                                                .appearance(false)
                                                .disabled(!can_prompt),
                                        ),
                                    )
                                    .when(!attachment_chips.is_empty(), |composer| {
                                        composer.child(
                                            div()
                                                .h_flex()
                                                .w_full()
                                                .flex_wrap()
                                                .gap_2()
                                                .px_2()
                                                .pb_2()
                                                .children(attachment_chips),
                                        )
                                    })
                                    .child(
                                        div()
                                            .h_flex()
                                            .w_full()
                                            .items_center()
                                            .justify_between()
                                            .gap_2()
                                            .pl_1()
                                            .pr_2()
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .min_w(px(0.))
                                                    .flex_wrap()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(attachment_button)
                                                    .child(provider_button)
                                                    .child(permission_button),
                                            )
                                            .child(
                                                div()
                                                    .min_w(px(0.))
                                                    .flex_1()
                                                    .truncate()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                    .when(
                                                        !can_prompt || active_turn.is_some(),
                                                        move |status| {
                                                        status.child(prompt_hint)
                                                        },
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .flex_none()
                                                    .items_center()
                                                    .gap_1()
                                                    .when_some(
                                                        context_window_indicator,
                                                        gpui::ParentElement::child,
                                                    )
                                                    .child(model_button)
                                                    .child(reasoning_button)
                                                    .child(composer_action),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(timeline_floating_control, |composer, control| {
                        composer.child(control)
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_first_completion_advances_queue_without_notifying() {
        assert_eq!(
            turn_completion_actions(false, "2000-01-01T00:00:00Z"),
            TurnCompletionActions {
                advance_queue: true,
                notify: false,
            }
        );
        assert_eq!(
            turn_completion_actions(true, "2000-01-01T00:00:00Z"),
            TurnCompletionActions {
                advance_queue: false,
                notify: false,
            }
        );
    }

    #[test]
    fn streaming_response_preview_bounds_layout_work_on_utf8_boundaries() {
        let short = "简短回答";
        assert_eq!(streaming_response_preview(short), (short, false));

        let long = "界".repeat(STREAMING_RESPONSE_PREVIEW_BYTES);
        let (preview, truncated) = streaming_response_preview(&long);

        assert!(truncated);
        assert!(preview.len() <= STREAMING_RESPONSE_PREVIEW_BYTES);
        assert!(long.ends_with(preview));
    }

    #[test]
    fn internal_tool_names_use_codex_style_labels() {
        assert_eq!(
            tool_display_title("search_openai_docs"),
            "Search OpenAI docs"
        );
        assert_eq!(tool_display_title("fetch_openai_doc"), "Fetch OpenAI doc");
        assert_eq!(tool_display_title("Web search"), "Web search");
    }

    #[test]
    fn only_long_completed_responses_use_block_virtualization() {
        assert!(!should_virtualize_response(
            &"x".repeat(LONG_RESPONSE_VIRTUALIZATION_BYTES - 1)
        ));
        assert!(should_virtualize_response(
            &"x".repeat(LONG_RESPONSE_VIRTUALIZATION_BYTES)
        ));
    }

    #[test]
    fn only_the_last_assistant_item_after_tool_activity_is_the_final_answer() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "问题".into(),
            steps: vec![
                TimelineStep {
                    item_id: "message-commentary".into(),
                    status: corbit_client::AgentTimelineStepStatus::Completed,
                    kind: TimelineStepKind::AssistantMessage {
                        text: "我先检查当前实现。".into(),
                    },
                },
                TimelineStep {
                    item_id: "tool-1".into(),
                    status: corbit_client::AgentTimelineStepStatus::Completed,
                    kind: TimelineStepKind::Tool {
                        tool_name: "search".into(),
                        title: Some("Search docs".into()),
                        input: None,
                        output: None,
                        error: None,
                        duration_ms: None,
                    },
                },
                TimelineStep {
                    item_id: "message-final".into(),
                    status: corbit_client::AgentTimelineStepStatus::Completed,
                    kind: TimelineStepKind::AssistantMessage {
                        text: "这是最终回答。".into(),
                    },
                },
            ],
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::Completed,
            error: None,
        };

        assert_eq!(final_response_step(&turn), Some((2, "这是最终回答。")));

        turn.steps.pop();
        assert_eq!(final_response(&turn), None);
    }

    #[test]
    fn response_footer_is_only_shown_after_successful_completion() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "问题".into(),
            steps: vec![TimelineStep {
                item_id: "message-final".into(),
                status: corbit_client::AgentTimelineStepStatus::InProgress,
                kind: TimelineStepKind::AssistantMessage {
                    text: "正在生成的回答".into(),
                },
            }],
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        assert!(!shows_completed_response_footer(&turn));

        turn.status = TimelineStatus::Interrupted;
        assert!(!shows_completed_response_footer(&turn));

        turn.status = TimelineStatus::Failed;
        assert!(!shows_completed_response_footer(&turn));

        turn.status = TimelineStatus::Completed;
        assert!(shows_completed_response_footer(&turn));

        turn.steps.clear();
        assert!(!shows_completed_response_footer(&turn));
    }

    #[test]
    fn activity_auto_expands_until_the_final_response_starts() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "问题".into(),
            steps: vec![TimelineStep {
                item_id: "reasoning-1".into(),
                status: corbit_client::AgentTimelineStepStatus::InProgress,
                kind: TimelineStepKind::Reasoning {
                    text: "正在分析实现".into(),
                },
            }],
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        assert!(automatically_expands_timeline_activity(&turn));
        assert!(timeline_activity_is_expanded(&turn, false, false));
        assert!(!timeline_activity_is_expanded(&turn, false, true));

        turn.steps.push(TimelineStep {
            item_id: "message-final".into(),
            status: corbit_client::AgentTimelineStepStatus::InProgress,
            kind: TimelineStepKind::AssistantMessage {
                text: "这是最终回答。".into(),
            },
        });

        assert!(!automatically_expands_timeline_activity(&turn));
        assert!(!timeline_activity_is_expanded(&turn, false, false));
        assert!(timeline_activity_is_expanded(&turn, true, false));

        turn.status = TimelineStatus::Completed;
        assert!(!automatically_expands_timeline_activity(&turn));
        assert!(!timeline_activity_is_expanded(&turn, false, false));
    }

    #[test]
    fn context_window_percentage_rounds_and_clamps() {
        assert_eq!(context_window_percent(211_000, 258_000), 82);
        assert_eq!(context_window_percent(0, 258_000), 0);
        assert_eq!(context_window_percent(300_000, 258_000), 100);
        assert_eq!(context_window_percent(1, 0), 0);
    }

    #[test]
    fn context_window_token_copy_uses_codex_compact_counts() {
        assert_eq!(ConnectionView::format_context_token_count(48_000), "48k");
        assert_eq!(ConnectionView::format_context_token_count(258_400), "258k");
        assert_eq!(ConnectionView::format_context_token_count(1_200_000), "1m");
    }

    #[test]
    fn permission_modes_use_codex_copy() {
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::ReadOnly),
            composer::PermissionModeCopy {
                title: "请求批准",
                description: "编辑外部文件和使用互联网时始终询问",
            }
        );
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::WorkspaceWrite),
            composer::PermissionModeCopy {
                title: "帮我批准",
                description: "自动审查低风险权限请求，高风险操作仍会询问",
            }
        );
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::FullAccess),
            composer::PermissionModeCopy {
                title: "完全访问权限",
                description: "可不受限制地访问互联网和你电脑上的任何文件",
            }
        );
    }

    #[test]
    fn composer_permission_only_matches_the_selected_agent() {
        let permission = PendingPermission {
            agent_id: "agent-1".into(),
            approval_id: "approval-1".into(),
            turn_id: "turn-1".into(),
            permission_kind: "command".into(),
            reason: Some("是否允许运行检查？".into()),
            command: Some("make check".into()),
            cwd: Some("/workspace".into()),
            grant_root: None,
            available_decisions: vec![corbit_client::AgentApprovalDecision::Accept],
        };

        assert!(permission_belongs_to_agent(&permission, Some("agent-1")));
        assert!(!permission_belongs_to_agent(&permission, Some("agent-2")));
        assert!(!permission_belongs_to_agent(&permission, None));
        assert_eq!(permission_question(&permission), "是否允许运行检查？");

        let fallback_permission = PendingPermission {
            reason: Some("   ".into()),
            ..permission
        };
        assert_eq!(
            permission_question(&fallback_permission),
            "是否允许执行此命令？"
        );
    }

    #[test]
    fn replayed_provider_metadata_keeps_the_first_provider() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: None,
            prompt: String::new(),
            steps: Vec::new(),
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        remember_turn_provider(&mut turn, Some("codex"));
        remember_turn_provider(&mut turn, Some("claude"));
        assert_eq!(turn.provider.as_deref(), Some("codex"));
    }

    #[test]
    fn completed_turn_does_not_regress_when_duplicate_start_is_replayed() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "原始问题".into(),
            steps: vec![TimelineStep {
                item_id: "message-1".into(),
                status: corbit_client::AgentTimelineStepStatus::Completed,
                kind: TimelineStepKind::AssistantMessage {
                    text: "已完成回答".into(),
                },
            }],
            diff: None,
            usage: None,
            started_at: Some("2026-08-17T00:00:00Z".into()),
            completed_at: Some("2026-08-17T00:00:01Z".into()),
            duration_ms: Some(1_000),
            status: TimelineStatus::Completed,
            error: None,
        };

        apply_turn_started_event(&mut turn, String::new(), "2026-08-17T00:05:00Z".into());

        assert_eq!(turn.status, TimelineStatus::Completed);
        assert_eq!(turn.prompt, "原始问题");
        assert_eq!(turn.started_at.as_deref(), Some("2026-08-17T00:00:00Z"));
        assert_eq!(turn.completed_at.as_deref(), Some("2026-08-17T00:00:01Z"));
        assert_eq!(turn.duration_ms, Some(1_000));
    }

    #[test]
    fn lifecycle_replay_keeps_terminal_state_after_late_start_event() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("claude".into()),
            prompt: "问题".into(),
            steps: vec![TimelineStep {
                item_id: "message-1".into(),
                status: corbit_client::AgentTimelineStepStatus::InProgress,
                kind: TimelineStepKind::AssistantMessage {
                    text: "回答".into(),
                },
            }],
            diff: None,
            usage: None,
            started_at: Some("2026-08-17T00:00:00Z".into()),
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        apply_turn_completed_event(
            &mut turn,
            &corbit_client::AgentTurnStatus::Completed,
            None,
            "2026-08-17T00:00:08Z".into(),
            Some(8_000),
        );
        apply_turn_started_event(
            &mut turn,
            "重放的空 prompt".into(),
            "2026-08-17T00:00:09Z".into(),
        );

        assert_eq!(turn.status, TimelineStatus::Completed);
        assert_eq!(turn.prompt, "问题");
        assert_eq!(turn.completed_at.as_deref(), Some("2026-08-17T00:00:08Z"));
        assert_eq!(turn.duration_ms, Some(8_000));
    }

    #[test]
    fn lifecycle_completion_preserves_failure_semantics_and_closes_steps() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "问题".into(),
            steps: vec![TimelineStep {
                item_id: "step-1".into(),
                status: corbit_client::AgentTimelineStepStatus::InProgress,
                kind: TimelineStepKind::Reasoning {
                    text: "处理中".into(),
                },
            }],
            diff: None,
            usage: None,
            started_at: Some("2026-08-17T00:00:00Z".into()),
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        apply_turn_completed_event(
            &mut turn,
            &corbit_client::AgentTurnStatus::Failed,
            Some("连接中断".into()),
            "2026-08-17T00:00:02Z".into(),
            Some(2_000),
        );

        assert_eq!(turn.status, TimelineStatus::Failed);
        assert_eq!(turn.error.as_deref(), Some("连接中断"));
        assert_eq!(
            turn.steps[0].status,
            corbit_client::AgentTimelineStepStatus::Failed
        );
    }

    #[test]
    fn timeline_index_locates_large_interleaved_histories() {
        const AGENT_COUNT: usize = 8;
        const TURN_COUNT: usize = 10_000;
        let mut index = TimelineIndex::default();

        for timeline_index in 0..TURN_COUNT {
            let agent_number = timeline_index % AGENT_COUNT;
            let agent_id = format!("agent-{agent_number}");
            let turn_id = format!("turn-{timeline_index}");
            let location = index.insert(&agent_id, &turn_id, timeline_index);

            assert_eq!(location.timeline_index, timeline_index);
            assert_eq!(location.agent_index, timeline_index / AGENT_COUNT);
            assert_eq!(index.location(&agent_id, &turn_id), Some(location));
        }

        for agent_number in 0..AGENT_COUNT {
            let agent_id = format!("agent-{agent_number}");
            let agent_indices = index.agent_indices(&agent_id);
            assert_eq!(agent_indices.len(), TURN_COUNT / AGENT_COUNT);
            assert!(
                agent_indices
                    .windows(2)
                    .all(|pair| pair[1] - pair[0] == AGENT_COUNT)
            );
        }
    }

    #[test]
    fn timeline_index_does_not_duplicate_replayed_turns() {
        let mut index = TimelineIndex::default();
        let first = index.insert("agent-1", "turn-1", 12);
        let replayed = index.insert("agent-1", "turn-1", 99);

        assert_eq!(replayed, first);
        assert_eq!(index.agent_indices("agent-1"), &[12]);
        assert_eq!(index.location("agent-1", "turn-1"), Some(first));
    }

    #[test]
    fn timeline_index_clear_removes_turn_and_agent_entries() {
        let mut index = TimelineIndex::default();
        index.insert("agent-1", "turn-1", 0);
        index.insert("agent-2", "turn-2", 1);

        index.clear();

        assert!(index.location("agent-1", "turn-1").is_none());
        assert!(index.agent_indices("agent-1").is_empty());
        assert!(index.agent_indices("agent-2").is_empty());
    }

    #[test]
    fn conversation_index_keeps_all_entries_for_short_histories() {
        assert_eq!(conversation_index_entries(0), Vec::<usize>::new());
        assert_eq!(conversation_index_entries(4), vec![0, 1, 2, 3]);
    }

    #[test]
    fn conversation_index_samples_long_histories_and_keeps_both_ends() {
        let entries = conversation_index_entries(1_000);

        assert_eq!(entries.len(), index::CONVERSATION_INDEX_MAX_MARKERS);
        assert_eq!(entries.first(), Some(&0));
        assert_eq!(entries.last(), Some(&999));
        assert!(entries.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn conversation_index_selects_the_nearest_visible_marker() {
        let entries = vec![0, 10, 20, 30];

        assert_eq!(closest_conversation_index_entry(&entries, 17), Some(20));
        assert_eq!(closest_conversation_index_entry(&entries, 13), Some(10));
        assert_eq!(closest_conversation_index_entry(&[], 3), None);
    }

    #[test]
    fn conversation_index_hover_tracks_animated_focus_transitions() {
        let mut interaction = ConversationIndexInteraction::default();

        assert!(interaction.set_hovered(true));
        assert!(interaction.focus_slot(2));
        assert_eq!(interaction.from_slot, None);
        assert_eq!(interaction.to_slot, Some(2));

        assert!(interaction.focus_slot(3));
        assert_eq!(interaction.from_slot, Some(2));
        assert_eq!(interaction.to_slot, Some(3));
        assert!(!interaction.focus_slot(3));

        assert!(interaction.set_hovered(false));
        assert_eq!(interaction.from_slot, Some(3));
        assert_eq!(interaction.to_slot, None);
    }

    #[test]
    fn conversation_index_hover_emphasis_falls_off_across_neighbours() {
        let focused = conversation_index_marker_metrics(false, 4, Some(4));
        let neighbour = conversation_index_marker_metrics(false, 5, Some(4));
        let next_neighbour = conversation_index_marker_metrics(false, 6, Some(4));
        let distant = conversation_index_marker_metrics(false, 7, Some(4));
        let active = conversation_index_marker_metrics(true, 4, Some(4));

        assert!((focused.width - 20.).abs() < f32::EPSILON);
        assert!(focused.width > neighbour.width);
        assert!(neighbour.width > next_neighbour.width);
        assert!(next_neighbour.width > distant.width);
        assert!((distant.width - 8.).abs() < f32::EPSILON);
        assert!((active.width - 22.).abs() < f32::EPSILON);
    }

    #[test]
    fn active_turn_indicator_pulses_each_dot_without_hiding_it() {
        for dot_index in 0_u8..3 {
            let opacity = active_turn_indicator_dot_opacity(0.73, dot_index);
            assert!((0.35..=1.0).contains(&opacity));
        }

        assert!(
            active_turn_indicator_dot_opacity(0.32, 0) > active_turn_indicator_dot_opacity(0.32, 1)
        );
        assert!(
            active_turn_indicator_dot_opacity(0.50, 1) > active_turn_indicator_dot_opacity(0.50, 0)
        );
    }

    #[test]
    fn idle_jump_control_requires_meaningful_distance_from_latest() {
        assert!(!should_show_timeline_jump_control(0, 0, None, px(0.)));
        assert!(!should_show_timeline_jump_control(4, 4, None, px(0.)));
        assert!(!should_show_timeline_jump_control(
            4,
            2,
            Some(px(47.)),
            px(0.)
        ));
        assert!(should_show_timeline_jump_control(
            4,
            2,
            Some(px(48.)),
            px(0.)
        ));
        assert!(should_show_timeline_jump_control(4, 1, None, px(0.)));
    }

    #[test]
    fn active_turn_indicator_scrolls_timeline_to_latest_item() {
        let list_state = ListState::new(5, ListAlignment::Bottom, px(0.));
        list_state.scroll_to(gpui::ListOffset {
            item_ix: 1,
            offset_in_item: px(0.),
        });

        scroll_timeline_to_latest(&list_state);

        assert_eq!(list_state.logical_scroll_top().item_ix, 5);
        assert_eq!(list_state.logical_scroll_top().offset_in_item, px(0.));
    }

    #[test]
    fn turn_duration_matches_conversation_status_copy() {
        assert_eq!(ConnectionView::format_turn_duration(999), "0秒");
        assert_eq!(ConnectionView::format_turn_duration(14_000), "14秒");
        assert_eq!(ConnectionView::format_turn_duration(489_000), "8分钟 9秒");
        assert_eq!(
            ConnectionView::format_turn_duration(3_669_000),
            "1小时 1分钟 9秒"
        );
    }

    #[test]
    fn active_turn_duration_uses_its_rfc3339_start_time() {
        let now = DateTime::parse_from_rfc3339("2026-08-16T12:08:09Z")
            .expect("fixed timestamp should be valid")
            .with_timezone(&Utc);

        assert_eq!(
            ConnectionView::elapsed_turn_duration_ms(Some("2026-08-16T12:00:00Z"), now),
            Some(489_000)
        );
        assert_eq!(
            ConnectionView::elapsed_turn_duration_ms(Some("invalid"), now),
            None
        );
        assert_eq!(ConnectionView::elapsed_turn_duration_ms(None, now), None);
    }

    #[test]
    fn activity_usage_stays_available_without_cluttering_the_answer() {
        let usage = TimelineUsage {
            input_tokens: 1_200,
            output_tokens: 345,
            total_tokens: 1_545,
            cached_input_tokens: Some(800),
            reasoning_output_tokens: Some(120),
            context_window: Some(10_000),
        };

        assert_eq!(
            ConnectionView::timeline_usage_summary(&usage),
            "1.5k tokens · 输入 1.2k · 输出 345 · 缓存 800 · 推理 120 · 上下文 15%"
        );
    }

    #[test]
    fn thinking_shimmer_keeps_text_visible_while_highlight_moves() {
        for glyph_index in 0..4 {
            let opacity = ConnectionView::thinking_shimmer_opacity(0.73, glyph_index);
            assert!((0.36..=1.0).contains(&opacity));
        }

        assert!(
            ConnectionView::thinking_shimmer_opacity(0.30, 0)
                > ConnectionView::thinking_shimmer_opacity(0.30, 1)
        );
        assert!(
            ConnectionView::thinking_shimmer_opacity(0.46, 1)
                > ConnectionView::thinking_shimmer_opacity(0.46, 0)
        );
    }
}
