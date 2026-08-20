use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use gpui::ease_out_quint;

const CONVERSATION_INDEX_MAX_MARKERS: usize = 32;
const CONVERSATION_INDEX_ANIMATION_DURATION: Duration = Duration::from_millis(180);
const ACTIVE_TURN_INDICATOR_ANIMATION_DURATION: Duration = Duration::from_millis(1_100);
const STREAMING_RESPONSE_PREVIEW_BYTES: usize = 12 * 1024;
const LONG_RESPONSE_VIRTUALIZATION_BYTES: usize = 24 * 1024;
const LONG_RESPONSE_VIEW_HEIGHT: f32 = 560.;
const CONVERSATION_BODY_FONT_SIZE: f32 = 15.;
const CONVERSATION_BODY_LINE_HEIGHT: f32 = 24.;
const CONVERSATION_PARAGRAPH_GAP_REMS: f32 = 0.75;
const MAX_PROMPT_ATTACHMENTS: usize = 3;
const MAX_PROMPT_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES: usize = 5 * 1024 * 1024;

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

fn is_permission_placeholder(turn: &TimelineTurn) -> bool {
    turn.prompt.is_empty()
        && turn.response.is_empty()
        && turn.steps.is_empty()
        && turn.diff.is_none()
        && turn.usage.is_none()
        && turn.started_at.is_none()
        && turn.completed_at.is_none()
        && turn.duration_ms.is_none()
        && turn.error.is_none()
}

fn remember_turn_provider(turn: &mut TimelineTurn, provider: Option<&str>) {
    if turn.provider.is_none() {
        turn.provider = provider
            .filter(|provider| !provider.is_empty())
            .map(str::to_owned);
    }
}

fn apply_turn_started_event(turn: &mut TimelineTurn, prompt: String, occurred_at: String) {
    if turn.status != TimelineStatus::InProgress {
        return;
    }

    turn.prompt = prompt;
    turn.error = None;
    turn.started_at = Some(occurred_at);
    turn.completed_at = None;
    turn.duration_ms = None;
}

fn conversation_index_entries(turn_count: usize) -> Vec<usize> {
    if turn_count <= CONVERSATION_INDEX_MAX_MARKERS {
        return (0..turn_count).collect();
    }

    let last_turn = turn_count - 1;
    (0..CONVERSATION_INDEX_MAX_MARKERS)
        .map(|slot| slot * last_turn / (CONVERSATION_INDEX_MAX_MARKERS - 1))
        .collect()
}

fn closest_conversation_index_entry(entries: &[usize], active_turn: usize) -> Option<usize> {
    entries
        .iter()
        .copied()
        .min_by_key(|entry| entry.abs_diff(active_turn))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ConversationIndexInteraction {
    hovered: bool,
    from_slot: Option<usize>,
    to_slot: Option<usize>,
    animation_generation: u64,
}

impl ConversationIndexInteraction {
    fn set_hovered(&mut self, hovered: bool) -> bool {
        if self.hovered == hovered {
            return false;
        }

        self.hovered = hovered;
        if hovered {
            self.from_slot = None;
            self.to_slot = None;
        } else {
            self.from_slot = self.to_slot;
            self.to_slot = None;
        }
        self.animation_generation = self.animation_generation.wrapping_add(1);
        true
    }

    fn focus_slot(&mut self, slot: usize) -> bool {
        if self.hovered && self.to_slot == Some(slot) {
            return false;
        }

        self.from_slot = self.hovered.then_some(self.to_slot).flatten();
        self.to_slot = Some(slot);
        self.hovered = true;
        self.animation_generation = self.animation_generation.wrapping_add(1);
        true
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ConversationIndexMarkerMetrics {
    width: f32,
    emphasis: f32,
}

fn conversation_index_marker_metrics(
    is_active: bool,
    marker_slot: usize,
    focus_slot: Option<usize>,
) -> ConversationIndexMarkerMetrics {
    let base_width = if is_active { 16. } else { 8. };
    let expanded_width = if is_active { 22. } else { 20. };
    let emphasis = focus_slot.map_or(0., |focus_slot| match marker_slot.abs_diff(focus_slot) {
        0 => 1.,
        1 => 0.42,
        2 => 0.16,
        _ => 0.,
    });

    ConversationIndexMarkerMetrics {
        width: base_width + (expanded_width - base_width) * emphasis,
        emphasis,
    }
}

fn interpolate_rgba(from: gpui::Rgba, to: gpui::Rgba, delta: f32) -> gpui::Rgba {
    gpui::Rgba {
        r: from.r + (to.r - from.r) * delta,
        g: from.g + (to.g - from.g) * delta,
        b: from.b + (to.b - from.b) * delta,
        a: from.a + (to.a - from.a) * delta,
    }
}

fn active_turn_indicator_dot_opacity(delta: f32, dot_index: u8) -> f32 {
    let phase = (delta - f32::from(dot_index) * 0.18).rem_euclid(1.0);
    let direct_distance = (phase - 0.32).abs();
    let wrapped_distance = direct_distance.min(1.0 - direct_distance);
    let pulse = (1.0 - wrapped_distance / 0.2).clamp(0.0, 1.0);
    0.35 + pulse * pulse * 0.65
}

fn scroll_timeline_to_latest(list_state: &ListState) {
    list_state.scroll_to(gpui::ListOffset {
        item_ix: list_state.item_count(),
        offset_in_item: px(0.),
    });
}

fn composer_supports_turn_options(provider: &str) -> bool {
    matches!(provider, "codex" | "claude")
}

fn composer_option_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(rgb(COLOR_TEXT).into())
        .hover(sidebar_row_hover_rgb().into())
        .active(sidebar_row_active_rgb().into())
}

fn composer_action_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(rgb(COLOR_TEXT).into())
        .foreground(rgb(COLOR_SURFACE).into())
        .hover(rgb(COLOR_TEXT_SECONDARY).into())
        .active(rgb(COLOR_TEXT_TERTIARY).into())
}

fn context_window_percent(used_tokens: u64, context_window: u64) -> u8 {
    if context_window == 0 {
        return 0;
    }

    let used_tokens = u128::from(used_tokens);
    let context_window = u128::from(context_window);
    let rounded = (used_tokens * 100 + context_window / 2) / context_window;
    rounded.min(100) as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PermissionModeCopy {
    title: &'static str,
    description: &'static str,
}

fn permission_mode_copy(mode: corbit_client::AgentPermissionMode) -> PermissionModeCopy {
    match mode {
        corbit_client::AgentPermissionMode::ReadOnly => PermissionModeCopy {
            title: "请求批准",
            description: "编辑外部文件和使用互联网时始终询问",
        },
        corbit_client::AgentPermissionMode::WorkspaceWrite => PermissionModeCopy {
            title: "帮我批准",
            description: "仅对检测到的风险操作请求批准",
        },
        corbit_client::AgentPermissionMode::FullAccess => PermissionModeCopy {
            title: "完全访问权限",
            description: "可不受限制地访问互联网和你电脑上的任何文件",
        },
    }
}

fn reasoning_effort_label(effort: corbit_client::AgentReasoningEffort) -> &'static str {
    match effort {
        corbit_client::AgentReasoningEffort::Low => "低推理",
        corbit_client::AgentReasoningEffort::Medium => "中推理",
        corbit_client::AgentReasoningEffort::High => "高推理",
        corbit_client::AgentReasoningEffort::Xhigh => "极高推理",
        corbit_client::AgentReasoningEffort::Max => "最高推理",
        corbit_client::AgentReasoningEffort::Ultra => "超强推理",
    }
}

fn reasoning_effort_short_label(effort: corbit_client::AgentReasoningEffort) -> &'static str {
    match effort {
        corbit_client::AgentReasoningEffort::Low => "低",
        corbit_client::AgentReasoningEffort::Medium => "中",
        corbit_client::AgentReasoningEffort::High => "高",
        corbit_client::AgentReasoningEffort::Xhigh => "极高",
        corbit_client::AgentReasoningEffort::Max => "最高",
        corbit_client::AgentReasoningEffort::Ultra => "超强",
    }
}

fn attachment_size_label(size: usize) -> String {
    format!("{} KB", size.div_ceil(1024))
}

fn load_prompt_attachments(
    paths: Vec<PathBuf>,
    available_slots: usize,
    existing_bytes: usize,
) -> Result<Vec<ComposerAttachment>, String> {
    if paths.len() > available_slots {
        return Err(format!(
            "每条消息最多可添加 {MAX_PROMPT_ATTACHMENTS} 个附件"
        ));
    }
    let mut total_bytes = existing_bytes;
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| format!("附件名称无效：{}", path.display()))?
                .to_owned();
            let bytes =
                std::fs::read(&path).map_err(|error| format!("无法读取附件 {name}：{error}"))?;
            if bytes.len() > MAX_PROMPT_ATTACHMENT_BYTES {
                return Err(format!("附件 {name} 超过 2 MB 上限"));
            }
            total_bytes += bytes.len();
            if total_bytes > MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES {
                return Err("附件总大小不能超过 5 MB".to_owned());
            }
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mime_type = match extension.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => {
                    std::str::from_utf8(&bytes)
                        .map_err(|_| format!("附件 {name} 不是支持的图片或 UTF-8 文本文件"))?;
                    "text/plain"
                }
            };
            Ok(ComposerAttachment {
                upload: corbit_client::AgentPromptAttachment {
                    name,
                    mime_type: mime_type.to_owned(),
                    data_base64: STANDARD.encode(&bytes),
                },
                size_bytes: bytes.len(),
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(super) struct RetryPrompt {
    signature: String,
    client_mutation_id: String,
}

#[derive(Clone, Debug)]
pub(super) struct ComposerAttachment {
    upload: corbit_client::AgentPromptAttachment,
    size_bytes: usize,
}

#[derive(Clone, Debug)]
pub(super) struct RetryControl {
    signature: String,
    client_mutation_id: String,
}

struct ProviderSwitchFailure {
    message: String,
    snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    provider_updated: bool,
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

async fn execute_provider_switch(
    client: corbit_client::DaemonRuntimeClient,
    agent: corbit_client::AgentResource,
    provider: String,
) -> Result<corbit_client::AuthoritativeSnapshot, ProviderSwitchFailure> {
    let mut snapshot = None;
    if agent.status == corbit_client::AgentStatus::Running {
        match client
            .mutate_and_snapshot(
                "agent.stop",
                json!({
                    "agentId": agent.id.clone(),
                    "clientMutationId": format!("provider_switch_stop_{}", uuid::Uuid::new_v4()),
                }),
            )
            .await
        {
            Ok((_, stopped_snapshot)) => snapshot = Some(stopped_snapshot),
            Err(error) => {
                return Err(ProviderSwitchFailure {
                    message: format!("停止原 Provider 会话失败：{error}"),
                    snapshot: None,
                    provider_updated: false,
                });
            }
        }
    }

    if agent.provider != provider {
        match client
            .mutate_and_snapshot(
                "agent.update",
                json!({
                    "agentId": agent.id.clone(),
                    "provider": provider.clone(),
                    "clientMutationId": format!("provider_switch_update_{}", uuid::Uuid::new_v4()),
                }),
            )
            .await
        {
            Ok((_, updated_snapshot)) => snapshot = Some(updated_snapshot),
            Err(error) => {
                return Err(ProviderSwitchFailure {
                    message: format!("更新 Agent Provider 失败：{error}"),
                    snapshot,
                    provider_updated: false,
                });
            }
        }
    }

    match client
        .mutate_and_snapshot(
            "agent.start",
            json!({
                "agentId": agent.id.clone(),
                "clientMutationId": format!("provider_switch_start_{}", uuid::Uuid::new_v4()),
            }),
        )
        .await
    {
        Ok((_, running_snapshot)) => Ok(running_snapshot),
        Err(error) => Err(ProviderSwitchFailure {
            message: format!("新 Provider 会话启动失败：{error}"),
            snapshot,
            provider_updated: agent.provider != provider,
        }),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TimelineStatus {
    InProgress,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug)]
pub(super) enum TimelineStepKind {
    Reasoning {
        text: String,
    },
    Plan {
        explanation: Option<String>,
        steps: Vec<corbit_client::AgentTimelinePlanStep>,
    },
    Command {
        command: String,
        cwd: Option<String>,
        output: String,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
    },
    FileChange {
        changes: Vec<corbit_client::AgentTimelineFileChange>,
    },
    Diff {
        diff: String,
    },
    Tool {
        tool_name: String,
        title: Option<String>,
        input: Option<String>,
        output: Option<String>,
        error: Option<String>,
        duration_ms: Option<u64>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct TimelineStep {
    pub(super) item_id: String,
    pub(super) status: corbit_client::AgentTimelineStepStatus,
    pub(super) kind: TimelineStepKind,
}

#[derive(Clone, Debug)]
pub(super) struct TimelineUsage {
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) total_tokens: u64,
    pub(super) cached_input_tokens: Option<u64>,
    pub(super) reasoning_output_tokens: Option<u64>,
    pub(super) context_window: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct TimelineTurn {
    pub(super) agent_id: String,
    pub(super) turn_id: String,
    pub(super) provider: Option<String>,
    pub(super) prompt: String,
    pub(super) response: String,
    pub(super) steps: Vec<TimelineStep>,
    pub(super) diff: Option<String>,
    pub(super) usage: Option<TimelineUsage>,
    pub(super) started_at: Option<String>,
    pub(super) completed_at: Option<String>,
    pub(super) duration_ms: Option<u64>,
    pub(super) status: TimelineStatus,
    pub(super) error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TimelineLocation {
    timeline_index: usize,
    agent_index: usize,
}

#[derive(Debug, Default)]
pub(super) struct TimelineIndex {
    by_turn: BTreeMap<String, BTreeMap<String, TimelineLocation>>,
    by_agent: BTreeMap<String, Vec<usize>>,
}

impl TimelineIndex {
    fn location(&self, agent_id: &str, turn_id: &str) -> Option<TimelineLocation> {
        self.by_turn
            .get(agent_id)
            .and_then(|turns| turns.get(turn_id))
            .copied()
    }

    fn insert(&mut self, agent_id: &str, turn_id: &str, timeline_index: usize) -> TimelineLocation {
        if let Some(location) = self.location(agent_id, turn_id) {
            return location;
        }

        let agent_turns = self.by_agent.entry(agent_id.to_owned()).or_default();
        let location = TimelineLocation {
            timeline_index,
            agent_index: agent_turns.len(),
        };
        agent_turns.push(timeline_index);
        self.by_turn
            .entry(agent_id.to_owned())
            .or_default()
            .insert(turn_id.to_owned(), location);
        location
    }

    fn agent_indices(&self, agent_id: &str) -> &[usize] {
        self.by_agent.get(agent_id).map_or(&[], Vec::as_slice)
    }

    fn clear(&mut self) {
        self.by_turn.clear();
        self.by_agent.clear();
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingPermission {
    pub(super) agent_id: String,
    pub(super) approval_id: String,
    pub(super) turn_id: String,
    pub(super) permission_kind: String,
    pub(super) reason: Option<String>,
    pub(super) command: Option<String>,
    pub(super) cwd: Option<String>,
    pub(super) grant_root: Option<String>,
    pub(super) available_decisions: Vec<corbit_client::AgentApprovalDecision>,
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
        self.composer_models
            .get(provider)
            .and_then(|selected| entry.models.iter().find(|model| model.id == *selected))
            .or_else(|| entry.models.iter().find(|model| model.is_default))
            .or_else(|| entry.models.first())
    }

    fn composer_reasoning_effort(
        &self,
        provider: &str,
    ) -> Option<corbit_client::AgentReasoningEffort> {
        let model = self.composer_model_info(provider)?;
        let supports = |effort| {
            model
                .supported_reasoning_efforts
                .iter()
                .any(|candidate| candidate.reasoning_effort == effort)
        };
        self.composer_reasoning_efforts
            .get(provider)
            .copied()
            .filter(|effort| supports(*effort))
            .or(model
                .default_reasoning_effort
                .filter(|effort| supports(*effort)))
            .or_else(|| {
                supports(corbit_client::AgentReasoningEffort::Medium)
                    .then_some(corbit_client::AgentReasoningEffort::Medium)
            })
            .or_else(|| {
                model
                    .supported_reasoning_efforts
                    .first()
                    .map(|effort| effort.reasoning_effort)
            })
    }

    pub(super) fn reconcile_composer_catalog(&mut self) {
        let Some(catalog) = &self.provider_catalog else {
            return;
        };
        let selections = catalog
            .providers
            .iter()
            .filter(|provider| provider.available)
            .filter_map(|provider| {
                let model = self
                    .composer_models
                    .get(&provider.provider_id)
                    .and_then(|selected| provider.models.iter().find(|model| model.id == *selected))
                    .or_else(|| provider.models.iter().find(|model| model.is_default))
                    .or_else(|| provider.models.first())?;
                let selected_effort = self
                    .composer_reasoning_efforts
                    .get(&provider.provider_id)
                    .copied()
                    .filter(|selected| {
                        model
                            .supported_reasoning_efforts
                            .iter()
                            .any(|effort| effort.reasoning_effort == *selected)
                    });
                let effort = selected_effort
                    .or(model.default_reasoning_effort)
                    .filter(|selected| {
                        model
                            .supported_reasoning_efforts
                            .iter()
                            .any(|effort| effort.reasoning_effort == *selected)
                    })
                    .or_else(|| {
                        model
                            .supported_reasoning_efforts
                            .iter()
                            .find(|effort| {
                                effort.reasoning_effort
                                    == corbit_client::AgentReasoningEffort::Medium
                            })
                            .map(|effort| effort.reasoning_effort)
                    })
                    .or_else(|| {
                        model
                            .supported_reasoning_efforts
                            .first()
                            .map(|effort| effort.reasoning_effort)
                    });
                Some((provider.provider_id.clone(), model.id.clone(), effort))
            })
            .collect::<Vec<_>>();
        for (provider, model, effort) in selections {
            self.composer_models.insert(provider.clone(), model);
            if let Some(effort) = effort {
                self.composer_reasoning_efforts.insert(provider, effort);
            } else {
                self.composer_reasoning_efforts.remove(&provider);
            }
        }
    }

    fn choose_composer_model(&mut self, provider: &str, model: &str, cx: &mut Context<Self>) {
        if self
            .provider_catalog_entry(provider)
            .is_some_and(|entry| entry.models.iter().any(|candidate| candidate.id == model))
        {
            self.composer_models
                .insert(provider.to_owned(), model.to_owned());
            self.reconcile_composer_catalog();
            cx.notify();
        }
    }

    fn composer_prompt_options(&self, provider: &str) -> corbit_client::AgentPromptOptions {
        let supports_turn_options = composer_supports_turn_options(provider);
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
            self.show_validation_error("所选模型提供商当前不可用", cx);
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

    pub(super) fn apply_timeline(&mut self, payload: corbit_client::AgentTimelinePayload) {
        use corbit_client::AgentTimelineEvent;

        let corbit_client::AgentTimelinePayload {
            agent_id,
            provider,
            event,
            ..
        } = payload;
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
                turn.status = match status {
                    corbit_client::AgentTurnStatus::Completed => TimelineStatus::Completed,
                    corbit_client::AgentTurnStatus::Interrupted => TimelineStatus::Interrupted,
                    corbit_client::AgentTurnStatus::Failed => TimelineStatus::Failed,
                };
                turn.error = error;
                turn.completed_at = Some(occurred_at);
                turn.duration_ms = duration_ms;
                let terminal_step_status = if turn.status == TimelineStatus::Completed {
                    corbit_client::AgentTimelineStepStatus::Completed
                } else {
                    corbit_client::AgentTimelineStepStatus::Failed
                };
                for step in &mut turn.steps {
                    if matches!(
                        step.status,
                        corbit_client::AgentTimelineStepStatus::Pending
                            | corbit_client::AgentTimelineStepStatus::InProgress
                    ) {
                        step.status = terminal_step_status;
                    }
                }
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
            corbit_client::AgentTimelineEvent::AssistantDelta { turn_id, delta, .. } => {
                self.timeline_turn_mut(agent_id, &turn_id)
                    .response
                    .push_str(&delta);
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

    pub(super) fn apply_permission(&mut self, payload: corbit_client::AgentPermissionPayload) {
        let agent_id = payload.agent_id;
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
                // Approval requests can arrive before the corresponding timeline event.
                // Reserve the turn now so the approval is always rendered in the
                // conversation instead of in a separate composer-adjacent surface.
                self.timeline_turn_mut(&agent_id, &turn_id);
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
            response: String::new(),
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
        self.collapsed_timeline_steps.clear();
        self.expanded_timeline_activity.clear();
        self.timeline_list_state.reset(0);
        self.timeline_list_agent_id = None;
        self.conversation_index_interaction.reset();
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

        let options = self.composer_prompt_options(&provider);
        let signature = serde_json::to_string(&(&agent_id, &text, &options))
            .unwrap_or_else(|_| format!("{agent_id}:{text}"));
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
        self.detail = "正在提交 Prompt…".into();
        let submitted_agent_id = agent_id.clone();
        self.prompt_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .prompt_with_options(agent_id, text, client_mutation_id, options)
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.prompt_in_flight = false;
                match result {
                    Ok(acknowledgement) => {
                        view.retry_prompt = None;
                        view.prompt_drafts.remove(&submitted_agent_id);
                        view.prompt_clear_agent_id = Some(submitted_agent_id);
                        view.prompt_attachments.clear();
                        view.reset_timeline_list_to_selected();
                        view.show_success(
                            format!("Prompt 已接受 · Turn {}", acknowledgement.turn_id),
                            cx,
                        );
                        view.schedule_ui_state_save(cx);
                    }
                    Err(error) => {
                        view.show_error(
                            format!("Prompt 提交失败：{error}；再次提交将复用原 mutation ID"),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
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
            .map(|turn| turn.response.clone())
            .filter(|response| !response.is_empty())
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
        if !self.collapsed_timeline_steps.remove(key) {
            self.collapsed_timeline_steps.insert(key.to_owned());
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
        cx: &mut Context<Self>,
    ) {
        if !self.expanded_timeline_activity.remove(key) {
            self.expanded_timeline_activity.insert(key.to_owned());
        }
        self.timeline_dirty_turns
            .insert((agent_id.to_owned(), turn_id.to_owned()));
        self.flush_timeline_list_updates();
        cx.notify();
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
        let collapsed = self.collapsed_timeline_steps.contains(&key);
        let (icon, title, summary, copy_text) = match &step.kind {
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
                AppIcon::Provider,
                title.clone().unwrap_or_else(|| "工具调用".to_owned()),
                tool_name.clone(),
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

        div()
            .v_flex()
            .rounded_lg()
            .border_1()
            .border_color(rgb(COLOR_BORDER_LIGHT))
            .bg(rgb(COLOR_SURFACE_UNDER))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(42.))
                    .px_3()
                    .child(Icon::new(icon).size(px(15.)).text_color(status_color))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .child(div().font_medium().child(title))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(summary),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(status_color)
                                    .child(status_label),
                            )
                            .child(
                                Button::new(("copy-timeline-step", control_id))
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::Copy))
                                    .tooltip("复制步骤内容")
                                    .disabled(copy_disabled)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.copy_timeline_text(copy_text.clone(), "步骤内容", cx);
                                    })),
                            )
                            .child(
                                Button::new(("toggle-timeline-step", control_id))
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::ChevronRight))
                                    .tooltip(if collapsed {
                                        "展开详情"
                                    } else {
                                        "收起详情"
                                    })
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.toggle_timeline_step(
                                            &toggle_agent_id,
                                            &toggle_turn_id,
                                            &toggle_key,
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            .when(!collapsed, |card| {
                card.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .border_t_1()
                        .border_color(rgb(COLOR_BORDER_LIGHT))
                        .px_3()
                        .py_3()
                        .child(detail),
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

    #[allow(clippy::too_many_lines)]
    fn render_active_timeline_step(step: &TimelineStep) -> Div {
        let is_running = matches!(
            step.status,
            corbit_client::AgentTimelineStepStatus::Pending
                | corbit_client::AgentTimelineStepStatus::InProgress
        );
        let (icon, summary, error) = match &step.kind {
            TimelineStepKind::Reasoning { text } => {
                let detail = text.lines().map(str::trim).find(|line| !line.is_empty());
                let mut summary = if is_running {
                    "正在分析任务".to_owned()
                } else {
                    "完成了任务分析".to_owned()
                };
                if let Some(detail) = detail {
                    summary.push_str(" · ");
                    summary.push_str(detail);
                }
                (AppIcon::Agent, summary, None)
            }
            TimelineStepKind::Plan { steps, .. } => (
                AppIcon::Tasks,
                if is_running {
                    format!("正在更新执行计划 · {} 个步骤", steps.len())
                } else {
                    format!("更新了执行计划 · {} 个步骤", steps.len())
                },
                None,
            ),
            TimelineStepKind::Command {
                command,
                exit_code,
                duration_ms,
                ..
            } => {
                let mut summary = if is_running {
                    "正在运行命令".to_owned()
                } else {
                    "运行了命令".to_owned()
                };
                if let Some(command) = command.lines().next().filter(|command| !command.is_empty())
                {
                    summary.push_str(" · ");
                    summary.push_str(command);
                }
                if let Some(duration_ms) = duration_ms {
                    summary.push_str(" · ");
                    summary.push_str(&Self::format_duration(*duration_ms));
                }
                let error = exit_code
                    .filter(|exit_code| *exit_code != 0)
                    .map(|exit_code| format!("命令退出码 {exit_code}"));
                (AppIcon::Terminal, summary, error)
            }
            TimelineStepKind::FileChange { changes } => {
                let mut summary = if is_running {
                    format!("正在修改文件 · {} 个文件", changes.len())
                } else {
                    format!("修改了 {} 个文件", changes.len())
                };
                if let Some(path) = changes.first().map(|change| change.path.as_str()) {
                    summary.push_str(" · ");
                    summary.push_str(path);
                }
                (AppIcon::Changes, summary, None)
            }
            TimelineStepKind::Diff { diff } => (
                AppIcon::Changes,
                if is_running {
                    format!("正在整理更改 · {} 行", diff.lines().count())
                } else {
                    format!("整理了代码更改 · {} 行", diff.lines().count())
                },
                None,
            ),
            TimelineStepKind::Tool {
                tool_name,
                title,
                error,
                duration_ms,
                ..
            } => {
                let tool_label = title.as_deref().unwrap_or(tool_name);
                let mut summary = if is_running {
                    format!("正在使用 {tool_label}")
                } else {
                    format!("使用了 {tool_label}")
                };
                if let Some(duration_ms) = duration_ms {
                    summary.push_str(" · ");
                    summary.push_str(&Self::format_duration(*duration_ms));
                }
                (AppIcon::Tool, summary, error.clone())
            }
        };
        let failed = matches!(
            step.status,
            corbit_client::AgentTimelineStepStatus::Failed
                | corbit_client::AgentTimelineStepStatus::Declined
        ) || error.is_some();
        let activity_color = if failed {
            rgb(COLOR_ERROR)
        } else {
            rgb(COLOR_TEXT_SECONDARY)
        };

        div()
            .v_flex()
            .gap_1()
            .py_1()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .text_size(font_px(FONT_SIZE_BASE))
                    .text_color(activity_color)
                    .child(
                        Icon::new(icon)
                            .size(px(17.))
                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                    )
                    .child(div().flex_1().min_w(px(0.)).truncate().child(summary)),
            )
            .when_some(error, |activity, error| {
                activity.child(
                    div()
                        .pl(px(25.))
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_ERROR))
                        .child(error),
                )
            })
    }

    fn active_timeline_activity_summary(steps: &[TimelineStep], has_diff: bool) -> Option<String> {
        let mut activities = Vec::new();
        for step in steps {
            let activity = match &step.kind {
                TimelineStepKind::Reasoning { .. } => "分析任务",
                TimelineStepKind::Plan { .. } => "更新计划",
                TimelineStepKind::Command { .. } => "运行命令",
                TimelineStepKind::FileChange { .. } => "修改文件",
                TimelineStepKind::Diff { .. } => "整理更改",
                TimelineStepKind::Tool { .. } => "调用工具",
            };
            if !activities.contains(&activity) {
                activities.push(activity);
            }
        }
        if has_diff && !activities.contains(&"整理更改") {
            activities.push("整理更改");
        }
        (!activities.is_empty()).then(|| activities.join(" · "))
    }

    fn render_active_timeline_activity(
        &self,
        index: usize,
        turn: &TimelineTurn,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let has_diff = turn.diff.as_ref().is_some_and(|diff| !diff.is_empty());
        let summary = Self::active_timeline_activity_summary(&turn.steps, has_diff)?;
        let key = format!("activity:{}:{}", turn.agent_id, turn.turn_id);
        let expanded = self.expanded_timeline_activity.contains(&key);
        let mut activity_rows = turn
            .steps
            .iter()
            .map(Self::render_active_timeline_step)
            .collect::<Vec<_>>();
        if let Some(diff) = turn.diff.as_ref().filter(|diff| !diff.is_empty()) {
            activity_rows.push(Self::render_active_timeline_step(&TimelineStep {
                item_id: "turn-diff".into(),
                status: corbit_client::AgentTimelineStepStatus::InProgress,
                kind: TimelineStepKind::Diff { diff: diff.clone() },
            }));
        }
        let toggle_key = key.clone();
        let toggle_agent_id = turn.agent_id.clone();
        let toggle_turn_id = turn.turn_id.clone();

        Some(
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .id(("toggle-active-timeline-activity", index))
                        .h_flex()
                        .w_full()
                        .min_h(px(32.))
                        .items_center()
                        .gap_2()
                        .rounded_md()
                        .px_1()
                        .cursor_pointer()
                        .text_size(font_px(FONT_SIZE_BASE))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .hover(|row| row.bg(rgb(COLOR_SURFACE_UNDER)))
                        .tooltip(move |window, cx| {
                            Tooltip::new(if expanded {
                                "收起思考活动"
                            } else {
                                "展开思考活动"
                            })
                            .build(window, cx)
                        })
                        .child(
                            Icon::new(AppIcon::Tool)
                                .size(px(17.))
                                .text_color(rgb(COLOR_TEXT_TERTIARY)),
                        )
                        .child(div().flex_1().min_w(px(0.)).truncate().child(summary))
                        .child(
                            Icon::new(AppIcon::ChevronRight)
                                .size(px(14.))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .when(expanded, |icon| icon.rotate(percentage(90. / 360.))),
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.toggle_timeline_activity(
                                &toggle_agent_id,
                                &toggle_turn_id,
                                &toggle_key,
                                cx,
                            );
                        })),
                )
                .when(expanded, |activity| {
                    activity.child(
                        div()
                            .id(("active-timeline-activity-details", index))
                            .max_h(px(320.))
                            .overflow_y_scrollbar()
                            .child(div().v_flex().gap_1().pr_3().children(activity_rows)),
                    )
                }),
        )
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

    fn format_one_decimal(value: u64, tenth: u64, suffix: &str) -> String {
        let rounded_tenths = value / tenth + u64::from(value % tenth >= tenth / 2);
        format!("{}.{}{suffix}", rounded_tenths / 10, rounded_tenths % 10)
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
        let used_label = usage.map(|usage| Self::format_token_count(usage.used_tokens));
        let context_label = usage.map(|usage| Self::format_token_count(usage.context_window));
        let mut track_color = rgb(COLOR_TEXT_TERTIARY);
        track_color.a = 0.38;
        let progress_color = rgb(COLOR_TEXT_SECONDARY);

        div()
            .id("composer-context-window")
            .relative()
            .flex_none()
            .size(px(20.))
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
                                .child("上下文窗口"),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_BASE))
                                .font_medium()
                                .child(if usage.is_some() {
                                    format!("{percent}% 已用")
                                } else {
                                    "暂无使用数据".to_owned()
                                }),
                        )
                        .when_some(
                            used_label.clone().zip(context_label.clone()),
                            |tooltip, (used, context)| {
                                tooltip.child(
                                    div().text_size(font_px(FONT_SIZE_SM)).child(format!(
                                        "已使用 {used} tokens，共 {context} tokens"
                                    )),
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
                        let arc = Arc::new().inner_radius(6.5).outer_radius(8.5);
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
        let response_is_empty = turn.response.is_empty();
        let response_agent_id = turn.agent_id.clone();
        let response_turn_id = turn.turn_id.clone();
        let retry_prompt = turn.prompt.clone();
        let prompt_group: SharedString = format!("timeline-prompt-{index}").into();
        let response_group: SharedString = format!("timeline-response-{index}").into();
        let is_in_progress = turn.status == TimelineStatus::InProgress;
        let active_activity = is_in_progress
            .then(|| self.render_active_timeline_activity(index, turn, cx))
            .flatten();
        let execution_steps = if is_in_progress {
            Vec::new()
        } else {
            turn.steps
                .iter()
                .enumerate()
                .map(|(step_index, step)| {
                    self.render_timeline_step(index, step_index, turn, step, cx)
                })
                .collect::<Vec<_>>()
        };
        let diff_card = if is_in_progress {
            None
        } else {
            turn.diff
                .as_ref()
                .filter(|diff| !diff.is_empty())
                .map(|diff| {
                    let step = TimelineStep {
                        item_id: "turn-diff".into(),
                        status: match turn.status {
                            TimelineStatus::InProgress => {
                                corbit_client::AgentTimelineStepStatus::InProgress
                            }
                            TimelineStatus::Completed => {
                                corbit_client::AgentTimelineStepStatus::Completed
                            }
                            TimelineStatus::Interrupted => {
                                corbit_client::AgentTimelineStepStatus::Declined
                            }
                            TimelineStatus::Failed => {
                                corbit_client::AgentTimelineStepStatus::Failed
                            }
                        },
                        kind: TimelineStepKind::Diff { diff: diff.clone() },
                    };
                    self.render_timeline_step(index, turn.steps.len(), turn, &step, cx)
                })
        };
        let metrics = {
            let mut parts = Vec::new();
            if let Some(usage) = &turn.usage {
                parts.push(format!(
                    "{} tokens",
                    Self::format_token_count(usage.total_tokens)
                ));
                parts.push(format!(
                    "输入 {} · 输出 {}",
                    Self::format_token_count(usage.input_tokens),
                    Self::format_token_count(usage.output_tokens)
                ));
                if let Some(cached) = usage.cached_input_tokens.filter(|tokens| *tokens > 0) {
                    parts.push(format!("缓存 {}", Self::format_token_count(cached)));
                }
                if let Some(reasoning) = usage.reasoning_output_tokens.filter(|tokens| *tokens > 0)
                {
                    parts.push(format!("推理 {}", Self::format_token_count(reasoning)));
                }
                if let Some(context_window) = usage.context_window.filter(|tokens| *tokens > 0) {
                    let percent = context_window_percent(usage.total_tokens, context_window);
                    parts.push(format!("上下文 {percent}%"));
                }
            }
            (!parts.is_empty()).then(|| parts.join(" · "))
        };
        let response = if turn.response.is_empty() && is_in_progress {
            None
        } else if turn.response.is_empty() {
            Some(
                div()
                    .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
                    .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(format!("{provider_label} 未返回文本内容"))
                    .into_any_element(),
            )
        } else if is_in_progress {
            let (preview, is_truncated) = streaming_response_preview(&turn.response);
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
                turn.response.clone(),
                window,
                cx,
            )
            .style(conversation_markdown_style())
            .selectable(true)
            .w_full()
            .text_size(font_px(CONVERSATION_BODY_FONT_SIZE))
            .line_height(font_px(CONVERSATION_BODY_LINE_HEIGHT));
            let response = if should_virtualize_response(&turn.response) {
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
                    .group(response_group.clone())
                    .v_flex()
                    .gap_4()
                    .child(
                        div()
                            .v_flex()
                            .gap_3()
                            .child(
                                div()
                                    .h_flex()
                                    .min_h(px(24.))
                                    .items_center()
                                    .justify_between()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(status_color)
                                    .child(
                                        div()
                                            .h_flex()
                                            .items_center()
                                            .gap_2()
                    .child(provider_logo(provider_id, 18.))
                                            .child(status),
                                    )
                                    .child(
                                        div()
                                            .invisible()
                                            .group_hover(response_group, gpui::Styled::visible)
                                            .child(
                                                Button::new(("copy-turn-response", index))
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(Icon::new(AppIcon::Copy))
                                                    .tooltip("复制回答")
                                                    .disabled(response_is_empty)
                                                    .on_click(cx.listener(
                                                        move |view, _, _, cx| {
                                                            view.copy_timeline_response(
                                                                &response_agent_id,
                                                                &response_turn_id,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(div().w_full().h(px(1.)).bg(rgb(COLOR_BORDER_LIGHT))),
                    )
                    .when_some(active_activity, gpui::ParentElement::child)
                    .children(execution_steps)
                    .when_some(diff_card, gpui::ParentElement::child)
                    .when_some(response, gpui::ParentElement::child)
                    .when(is_in_progress, |conversation| {
                        conversation.child(Self::render_thinking_shimmer(&turn.turn_id))
                    })
                    .when_some(metrics, |conversation, metrics| {
                        conversation.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child(provider_logo(provider_id, 16.))
                                .child(metrics),
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
        let permission_label = match permission.permission_kind.as_str() {
            "command" => "命令执行权限",
            "file-change" => "文件修改权限",
            _ => "Agent 权限",
        };
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
            .when_some(permission.reason.clone(), |panel, reason| {
                panel.child(
                    div()
                        .line_height(px(21.))
                        .text_color(rgb(COLOR_TEXT))
                        .child(reason),
                )
            })
            .when_some(permission.command.clone(), |panel, command| {
                panel.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .rounded_md()
                        .bg(rgb(COLOR_EDITOR))
                        .p_3()
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
                        ),
                )
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
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .when(can_accept, |row| {
                        row.child(
                            Button::new(("permission-accept", index))
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
                            Button::new(("permission-accept-session", index))
                                .outline()
                                .small()
                                .label("本会话允许")
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
                            Button::new(("permission-decline", index))
                                .danger()
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
                            Button::new(("permission-cancel", index))
                                .outline()
                                .small()
                                .label("取消")
                                .disabled(control_in_flight)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.resolve_permission(
                                        cancel_permission.clone(),
                                        corbit_client::AgentApprovalDecision::Cancel,
                                        cx,
                                    );
                                })),
                        )
                    }),
            )
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
        let turn_agent_id = turn.agent_id.clone();
        let turn_id = turn.turn_id.clone();
        let related_permissions = self
            .permissions
            .iter()
            .enumerate()
            .filter(|(_, permission)| {
                permission.agent_id == turn_agent_id && permission.turn_id == turn_id
            })
            .map(|(permission_index, permission)| {
                Self::render_permission(permission_index, permission, self.control_in_flight, cx)
            })
            .collect::<Vec<_>>();
        let permission_placeholder = is_permission_placeholder(turn);
        let rendered_turn = (!permission_placeholder)
            .then(|| self.render_timeline_turn(display_index, turn, provider_id, window, cx));

        div()
            .w_full()
            .px_8()
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(content_max_width())
                    .mx_auto()
                    .gap_3()
                    .when(
                        permission_placeholder && !related_permissions.is_empty(),
                        gpui::Styled::py_6,
                    )
                    .when_some(rendered_turn, gpui::ParentElement::child)
                    .children(related_permissions),
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
                div()
                    .id(indicator_id)
                    .h_flex()
                    .w(px(52.))
                    .h(px(38.))
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .occlude()
                    .rounded(px(19.))
                    .border_1()
                    .border_color(rgb(COLOR_BORDER_HEAVY))
                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                    .shadow_md()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(COLOR_EDITOR)))
                    .active(|style| style.bg(rgb(COLOR_BORDER_LIGHT)))
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
        let selected_agent_exists = selected_agent.is_some();
        let selected_agent_running =
            selected_agent.is_some_and(|agent| agent.status == corbit_client::AgentStatus::Running);
        let provider_id = selected_agent
            .map(|agent| agent.provider.clone())
            .unwrap_or_default();
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
            && active_turn.is_none();
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
            "请先使用“新建任务”开始任务"
        } else if !is_online {
            "Daemon 当前未连接"
        } else if !selected_agent_running {
            "请先在工作区管理中启动 Agent"
        } else if active_turn.is_some() {
            "Agent 正在处理当前任务"
        } else {
            "发送内容将创建一个新的 Turn"
        };
        let action_variant = composer_action_variant(cx);
        let composer_action = if let Some((agent_id, turn_id)) = active_turn.clone() {
            Button::new("interrupt-turn")
                .custom(action_variant)
                .with_size(px(34.))
                .rounded(px(17.))
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
                .with_size(px(34.))
                .rounded(px(17.))
                .icon(Icon::new(AppIcon::Send))
                .tooltip("发送问题 · 回车")
                .loading(self.prompt_in_flight)
                .disabled(!can_prompt)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.send_prompt(cx);
                }))
        };
        let supports_turn_options = composer_supports_turn_options(&provider_id);
        let provider_choices = self.provider_options();
        let provider_view = cx.entity();
        let selected_provider = provider_id.clone();
        let option_variant = composer_option_variant(cx);
        let provider_button = Button::new("composer-provider")
            .custom(option_variant)
            .xsmall()
            .rounded(px(10.))
                    .child(provider_logo(&provider_id, 16.))
            .child(Self::provider_label(&provider_id).to_owned())
            .dropdown_caret(true)
            .tooltip("切换当前项目的模型提供商")
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
            || "Provider 默认".to_owned(),
            |model| model.display_name.clone(),
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
            .dropdown_caret(true)
            .tooltip(if supports_turn_options {
                "选择本次 Turn 使用的模型"
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
                            div()
                                .v_flex()
                                .min_w_0()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .font_medium()
                                        .child(model.display_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(model.description.clone()),
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
            .label(reasoning_effort.map_or("默认推理", reasoning_effort_label))
            .dropdown_caret(true)
            .tooltip(if supports_turn_options {
                "选择本次 Turn 的推理程度"
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
                            div()
                                .v_flex()
                                .min_w_0()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .font_medium()
                                        .child(reasoning_effort_short_label(effort)),
                                )
                                .when_some(choice.description.clone(), |item, description| {
                                    item.child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                                            .child(description),
                                    )
                                })
                        })
                        .checked(reasoning_effort == Some(effort))
                        .on_click(move |_, _, cx| {
                            item_view.update(cx, |view, cx| {
                                view.composer_reasoning_efforts
                                    .insert(item_provider.clone(), effort);
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
        let active_turn_indicator = active_turn.as_ref().map(|(agent_id, turn_id)| {
            Self::render_active_turn_indicator(agent_id, turn_id, &timeline_list_state, cx)
        });
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
                                            .when(!can_prompt, |status| status.child(prompt_hint)),
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
                    )
                    .when_some(active_turn_indicator, |composer, indicator| {
                        composer.child(indicator)
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn only_long_completed_responses_use_block_virtualization() {
        assert!(!should_virtualize_response(
            &"x".repeat(LONG_RESPONSE_VIRTUALIZATION_BYTES - 1)
        ));
        assert!(should_virtualize_response(
            &"x".repeat(LONG_RESPONSE_VIRTUALIZATION_BYTES)
        ));
    }

    #[test]
    fn context_window_percentage_rounds_and_clamps() {
        assert_eq!(context_window_percent(211_000, 258_000), 82);
        assert_eq!(context_window_percent(0, 258_000), 0);
        assert_eq!(context_window_percent(300_000, 258_000), 100);
        assert_eq!(context_window_percent(1, 0), 0);
    }

    #[test]
    fn permission_modes_use_codex_copy() {
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::ReadOnly),
            PermissionModeCopy {
                title: "请求批准",
                description: "编辑外部文件和使用互联网时始终询问",
            }
        );
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::WorkspaceWrite),
            PermissionModeCopy {
                title: "帮我批准",
                description: "仅对检测到的风险操作请求批准",
            }
        );
        assert_eq!(
            permission_mode_copy(corbit_client::AgentPermissionMode::FullAccess),
            PermissionModeCopy {
                title: "完全访问权限",
                description: "可不受限制地访问互联网和你电脑上的任何文件",
            }
        );
    }

    #[test]
    fn permission_placeholder_does_not_render_a_fake_turn() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: None,
            prompt: String::new(),
            response: String::new(),
            steps: Vec::new(),
            diff: None,
            usage: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            status: TimelineStatus::InProgress,
            error: None,
        };

        assert!(is_permission_placeholder(&turn));
        remember_turn_provider(&mut turn, Some("codex"));
        remember_turn_provider(&mut turn, Some("claude"));
        assert_eq!(turn.provider.as_deref(), Some("codex"));
        assert!(is_permission_placeholder(&turn));

        turn.started_at = Some("2026-08-17T00:00:00Z".into());
        assert!(!is_permission_placeholder(&turn));

        turn.started_at = None;
        turn.response = "已开始处理".into();
        assert!(!is_permission_placeholder(&turn));
    }

    #[test]
    fn completed_turn_does_not_regress_when_duplicate_start_is_replayed() {
        let mut turn = TimelineTurn {
            agent_id: "agent-1".into(),
            turn_id: "turn-1".into(),
            provider: Some("codex".into()),
            prompt: "原始问题".into(),
            response: "已完成回答".into(),
            steps: Vec::new(),
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

        assert_eq!(entries.len(), CONVERSATION_INDEX_MAX_MARKERS);
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
    fn active_activity_summary_is_compact_and_deduplicated() {
        let completed = corbit_client::AgentTimelineStepStatus::Completed;
        let steps = vec![
            TimelineStep {
                item_id: "reasoning".into(),
                status: completed,
                kind: TimelineStepKind::Reasoning {
                    text: "分析结构".into(),
                },
            },
            TimelineStep {
                item_id: "tool".into(),
                status: completed,
                kind: TimelineStepKind::Tool {
                    tool_name: "read_file".into(),
                    title: Some("读取文件".into()),
                    input: None,
                    output: None,
                    error: None,
                    duration_ms: None,
                },
            },
            TimelineStep {
                item_id: "command-1".into(),
                status: completed,
                kind: TimelineStepKind::Command {
                    command: "rg timeline".into(),
                    cwd: None,
                    output: String::new(),
                    exit_code: Some(0),
                    duration_ms: None,
                },
            },
            TimelineStep {
                item_id: "command-2".into(),
                status: completed,
                kind: TimelineStepKind::Command {
                    command: "cargo check".into(),
                    cwd: None,
                    output: String::new(),
                    exit_code: Some(0),
                    duration_ms: None,
                },
            },
        ];

        assert_eq!(
            ConnectionView::active_timeline_activity_summary(&steps, true).as_deref(),
            Some("分析任务 · 调用工具 · 运行命令 · 整理更改")
        );
        assert_eq!(
            ConnectionView::active_timeline_activity_summary(&[], false),
            None
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
