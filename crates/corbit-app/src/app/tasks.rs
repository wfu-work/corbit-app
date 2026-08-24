use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TaskFilter {
    All,
    Active,
    Attention,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NewTaskRecoveryStage {
    Workspace,
    Create,
    Start,
    Prompt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PendingNewTaskRecovery {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    working_directory: Option<String>,
    provider: String,
    title: String,
    prompt: String,
    #[serde(default)]
    prompt_options: Option<corbit_client::AgentPromptOptions>,
    agent_id: Option<String>,
    stage: NewTaskRecoveryStage,
    #[serde(default)]
    workspace_create_mutation_id: String,
    create_mutation_id: String,
    start_mutation_id: String,
    prompt_mutation_id: String,
    cleanup_stop_mutation_id: String,
    cleanup_delete_mutation_id: String,
    error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NewTaskLocation {
    project_id: String,
    workspace_id: Option<String>,
    workspace_name: String,
    working_directory: String,
}

fn resolve_new_task_location(
    snapshot: &corbit_client::AuthoritativeSnapshot,
    selected_project_id: Option<&str>,
    selected_workspace_id: Option<&str>,
) -> Result<NewTaskLocation, &'static str> {
    let project_id = selected_project_id.ok_or("请先从左侧选择一个项目")?;
    let project = snapshot
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or("所选项目已不存在，请重新选择")?;
    let workspace = selected_workspace_id
        .and_then(|workspace_id| {
            snapshot.workspaces.iter().find(|workspace| {
                workspace.id == workspace_id
                    && workspace.project_id == project.id
                    && workspace.status == corbit_client::WorkspaceStatus::Active
            })
        })
        .or_else(|| {
            snapshot.workspaces.iter().find(|workspace| {
                workspace.project_id == project.id
                    && workspace.status == corbit_client::WorkspaceStatus::Active
            })
        });

    Ok(NewTaskLocation {
        project_id: project.id.clone(),
        workspace_id: workspace.map(|workspace| workspace.id.clone()),
        workspace_name: project.name.clone(),
        working_directory: project.root_path.clone(),
    })
}

impl PendingNewTaskRecovery {
    pub(super) fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }
}

enum NewTaskAttemptResult {
    Completed {
        agent_id: String,
        snapshot: Option<corbit_client::AuthoritativeSnapshot>,
        turn_id: String,
        provider: String,
        prompt_options: Option<corbit_client::AgentPromptOptions>,
    },
    Failed {
        recovery: Box<PendingNewTaskRecovery>,
        snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    },
}

fn failed_new_task_attempt(
    mut recovery: PendingNewTaskRecovery,
    snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    error: impl Into<String>,
) -> NewTaskAttemptResult {
    recovery.error = error.into();
    NewTaskAttemptResult::Failed {
        recovery: Box::new(recovery),
        snapshot,
    }
}

enum NewTaskCleanupResult {
    Completed(corbit_client::AuthoritativeSnapshot),
    Failed {
        recovery: Box<PendingNewTaskRecovery>,
        snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    },
}

async fn prepare_new_task_workspace(
    client: &corbit_client::DaemonRuntimeClient,
    recovery: &mut PendingNewTaskRecovery,
) -> Result<corbit_client::AuthoritativeSnapshot, String> {
    let project_id = recovery
        .project_id
        .clone()
        .ok_or_else(|| "任务恢复信息缺少项目 ID".to_owned())?;
    let workspace_name = recovery
        .workspace_name
        .clone()
        .ok_or_else(|| "任务恢复信息缺少默认工作区名称".to_owned())?;
    let working_directory = recovery
        .working_directory
        .clone()
        .ok_or_else(|| "任务恢复信息缺少默认工作目录".to_owned())?;
    let (created, snapshot) = client
        .mutate_and_snapshot(
            "workspace.create",
            json!({
                "projectId": project_id,
                "name": workspace_name,
                "workingDirectory": working_directory,
                "clientMutationId": recovery.workspace_create_mutation_id,
            }),
        )
        .await
        .map_err(|error| format!("准备项目工作区失败：{error}"))?;
    recovery.workspace_id = Some(created.resource_id);
    recovery.stage = NewTaskRecoveryStage::Create;
    Ok(snapshot)
}

async fn execute_new_task_attempt(
    client: corbit_client::DaemonRuntimeClient,
    mut recovery: PendingNewTaskRecovery,
) -> NewTaskAttemptResult {
    let mut latest_snapshot = None;

    if recovery.stage == NewTaskRecoveryStage::Workspace {
        match prepare_new_task_workspace(&client, &mut recovery).await {
            Ok(snapshot) => latest_snapshot = Some(snapshot),
            Err(error) => return failed_new_task_attempt(recovery, None, error),
        }
    }

    if recovery.stage == NewTaskRecoveryStage::Create {
        let Some(workspace_id) = recovery.workspace_id.clone() else {
            return failed_new_task_attempt(recovery, latest_snapshot, "任务恢复信息缺少工作区 ID");
        };
        match client
            .mutate_and_snapshot(
                "agent.create",
                json!({
                    "workspaceId": workspace_id,
                    "provider": recovery.provider,
                    "title": recovery.title,
                    "clientMutationId": recovery.create_mutation_id,
                }),
            )
            .await
        {
            Ok((created, snapshot)) => {
                recovery.agent_id = Some(created.resource_id);
                recovery.stage = NewTaskRecoveryStage::Start;
                latest_snapshot = Some(snapshot);
            }
            Err(error) => {
                return failed_new_task_attempt(
                    recovery,
                    latest_snapshot,
                    format!("创建任务失败：{error}"),
                );
            }
        }
    }

    let Some(agent_id) = recovery.agent_id.clone() else {
        return failed_new_task_attempt(recovery, latest_snapshot, "任务恢复信息缺少 Agent ID");
    };

    if recovery.stage == NewTaskRecoveryStage::Start {
        match client
            .mutate_and_snapshot(
                "agent.start",
                json!({
                    "agentId": agent_id,
                    "clientMutationId": recovery.start_mutation_id,
                }),
            )
            .await
        {
            Ok((_, snapshot)) => {
                recovery.stage = NewTaskRecoveryStage::Prompt;
                latest_snapshot = Some(snapshot);
            }
            Err(error) => {
                return failed_new_task_attempt(
                    recovery,
                    latest_snapshot,
                    format!("提供商启动失败：{error}"),
                );
            }
        }
    }

    let prompt_result = if let Some(options) = recovery.prompt_options.clone() {
        client
            .prompt_with_options(
                agent_id.clone(),
                recovery.prompt.clone(),
                recovery.prompt_mutation_id.clone(),
                options,
            )
            .await
    } else {
        client
            .prompt(
                agent_id.clone(),
                recovery.prompt.clone(),
                recovery.prompt_mutation_id.clone(),
            )
            .await
    };
    match prompt_result {
        Ok(acknowledgement) => NewTaskAttemptResult::Completed {
            agent_id,
            snapshot: latest_snapshot,
            turn_id: acknowledgement.turn_id,
            provider: recovery.provider,
            prompt_options: recovery.prompt_options,
        },
        Err(error) => failed_new_task_attempt(
            recovery,
            latest_snapshot,
            format!("Prompt 提交失败：{error}"),
        ),
    }
}

async fn execute_new_task_cleanup(
    client: corbit_client::DaemonRuntimeClient,
    mut recovery: PendingNewTaskRecovery,
    needs_stop: bool,
) -> NewTaskCleanupResult {
    let Some(agent_id) = recovery.agent_id.clone() else {
        recovery.error = "任务清理信息缺少 Agent ID".into();
        return NewTaskCleanupResult::Failed {
            recovery: Box::new(recovery),
            snapshot: None,
        };
    };
    let mut latest_snapshot = None;

    if needs_stop {
        match client
            .mutate_and_snapshot(
                "agent.stop",
                json!({
                    "agentId": agent_id,
                    "clientMutationId": recovery.cleanup_stop_mutation_id,
                }),
            )
            .await
        {
            Ok((_, snapshot)) => latest_snapshot = Some(snapshot),
            Err(error) => {
                recovery.error = format!("停止未完成任务失败：{error}");
                return NewTaskCleanupResult::Failed {
                    recovery: Box::new(recovery),
                    snapshot: None,
                };
            }
        }
    }

    match client
        .mutate_and_snapshot(
            "agent.delete",
            json!({
                "agentId": agent_id,
                "clientMutationId": recovery.cleanup_delete_mutation_id,
            }),
        )
        .await
    {
        Ok((_, snapshot)) => NewTaskCleanupResult::Completed(snapshot),
        Err(error) => {
            recovery.error = format!("删除未完成任务失败：{error}");
            NewTaskCleanupResult::Failed {
                recovery: Box::new(recovery),
                snapshot: latest_snapshot,
            }
        }
    }
}

impl ConnectionView {
    pub(super) fn reconcile_new_task_recovery(&mut self) {
        let Some(recovery) = self.new_task_recovery.as_mut() else {
            return;
        };
        let Some(agent_id) = recovery.agent_id.as_ref() else {
            return;
        };
        let status = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .agents
                .iter()
                .find(|agent| &agent.id == agent_id)
                .map(|agent| agent.status.clone())
        });
        match status {
            None => {
                self.new_task_recovery = None;
                self.new_task_cleanup_armed = false;
            }
            Some(corbit_client::AgentStatus::Running)
                if recovery.stage == NewTaskRecoveryStage::Start =>
            {
                recovery.stage = NewTaskRecoveryStage::Prompt;
                recovery.error = "Agent 已启动，可以继续提交首次 Prompt".into();
            }
            Some(_) => {}
        }
    }

    fn set_task_filter(&mut self, filter: TaskFilter, cx: &mut Context<Self>) {
        self.task_filter = filter;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn task_matches_filter(&self, agent: &corbit_client::AgentResource) -> bool {
        match self.task_filter {
            TaskFilter::All => true,
            TaskFilter::Active => matches!(
                agent.status,
                corbit_client::AgentStatus::Initializing | corbit_client::AgentStatus::Running
            ),
            TaskFilter::Attention => {
                agent.status == corbit_client::AgentStatus::Error
                    || self
                        .permissions
                        .iter()
                        .any(|permission| permission.agent_id == agent.id)
            }
            TaskFilter::Stopped => matches!(
                agent.status,
                corbit_client::AgentStatus::Idle | corbit_client::AgentStatus::Stopped
            ),
        }
    }

    pub(super) fn provider_options(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        if let Some(catalog) = &self.provider_catalog {
            return PROVIDERS
                .iter()
                .filter(|provider| {
                    catalog
                        .providers
                        .iter()
                        .any(|entry| entry.provider_id == provider.id && entry.available)
                })
                .map(|provider| (provider.id, provider.label, provider.description))
                .collect();
        }
        Vec::new()
    }

    pub(super) fn provider_label(provider: &str) -> &str {
        provider_label(provider)
    }

    pub(super) fn feature_enabled(&self, feature: &str) -> bool {
        self.server_info
            .as_ref()
            .and_then(|info| info.features.get(feature))
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn ensure_selected_provider(&mut self) {
        let options = self.provider_options();
        if options.is_empty() {
            return;
        }
        if !options
            .iter()
            .any(|(provider, _, _)| *provider == self.selected_provider)
        {
            self.selected_provider = options.first().map_or_else(
                || "codex".to_owned(),
                |(provider, _, _)| (*provider).to_owned(),
            );
        }
    }

    pub(super) fn reconcile_project_providers(&mut self) {
        let available = self
            .provider_options()
            .into_iter()
            .map(|(provider, _, _)| provider)
            .collect::<BTreeSet<_>>();
        self.project_providers
            .retain(|_, provider| available.contains(provider.as_str()));
    }

    pub(super) fn provider_is_available(&self, provider: &str) -> bool {
        self.provider_options()
            .iter()
            .any(|(candidate, _, _)| *candidate == provider)
    }

    pub(super) fn provider_prompt_blocker(&self, provider: &str) -> Option<String> {
        if self.provider_catalog.is_none() {
            return Some("正在读取 Daemon 的模型目录，请稍候".into());
        }
        if self.provider_catalog_error.is_some()
            && self
                .provider_catalog
                .as_ref()
                .is_none_or(|catalog| catalog.providers.is_empty())
        {
            return Some("模型目录读取失败，请重新连接 Daemon 后重试".into());
        }
        let entry = self
            .provider_catalog
            .as_ref()?
            .providers
            .iter()
            .find(|entry| entry.provider_id == provider);
        match entry {
            Some(entry)
                if entry.available
                    && (!provider_supports_turn_options(provider) || !entry.models.is_empty()) =>
            {
                None
            }
            Some(entry) if entry.available => Some(format!(
                "{} 当前没有报告可用模型，请刷新模型目录后重试",
                Self::provider_label(provider)
            )),
            Some(entry) => Some(entry.reason.as_deref().map_or_else(
                || format!("{} 当前不可用", Self::provider_label(provider)),
                |reason| format!("{} 当前不可用：{reason}", Self::provider_label(provider)),
            )),
            None => Some(format!(
                "Daemon 未报告 {} Provider，请切换到可用 Provider",
                Self::provider_label(provider)
            )),
        }
    }

    pub(super) fn project_provider(&self, project_id: &str) -> String {
        self.project_providers
            .get(project_id)
            .filter(|provider| self.provider_is_available(provider))
            .cloned()
            .or_else(|| {
                self.provider_is_available(&self.selected_provider)
                    .then(|| self.selected_provider.clone())
            })
            .or_else(|| {
                self.provider_options()
                    .first()
                    .map(|(provider, _, _)| (*provider).to_owned())
            })
            .unwrap_or_else(|| self.selected_provider.clone())
    }

    pub(super) fn selected_project_provider(&self) -> String {
        self.selected_project_id.as_deref().map_or_else(
            || self.selected_provider.clone(),
            |project_id| self.project_provider(project_id),
        )
    }

    fn new_task_model_info(
        &self,
        project_id: &str,
        provider: &str,
    ) -> Option<&corbit_client::ProviderModelInfo> {
        let entry = self
            .provider_catalog
            .as_ref()?
            .providers
            .iter()
            .find(|entry| entry.provider_id == provider && entry.available)?;
        self.composer_selections.project_model(project_id, entry)
    }

    fn new_task_reasoning_effort(
        &self,
        project_id: &str,
        provider: &str,
    ) -> Option<corbit_client::AgentReasoningEffort> {
        let entry = self
            .provider_catalog
            .as_ref()?
            .providers
            .iter()
            .find(|entry| entry.provider_id == provider && entry.available)?;
        self.composer_selections
            .project_reasoning_effort(project_id, entry)
    }

    fn new_task_prompt_options(
        &self,
        project_id: &str,
        provider: &str,
    ) -> corbit_client::AgentPromptOptions {
        let supports_turn_options = provider_supports_turn_options(provider);
        let supports_codex_configuration = provider == "codex";
        corbit_client::AgentPromptOptions {
            model: supports_turn_options
                .then(|| {
                    self.new_task_model_info(project_id, provider)
                        .map(|model| model.id.clone())
                })
                .flatten(),
            permission_mode: supports_turn_options.then_some(self.composer_permission_mode),
            reasoning_effort: supports_turn_options
                .then(|| self.new_task_reasoning_effort(project_id, provider))
                .flatten(),
            network_access: supports_codex_configuration
                .then_some(self.agent_configuration.network_access),
            reasoning_summary: supports_codex_configuration
                .then_some(self.agent_configuration.reasoning_summary),
            personality: supports_codex_configuration
                .then_some(self.agent_configuration.personality)
                .flatten(),
            attachments: Vec::new(),
        }
    }

    fn choose_new_task_model(
        &mut self,
        project_id: &str,
        provider: &str,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let entry = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog
                .providers
                .iter()
                .find(|entry| entry.provider_id == provider && entry.available)
                .cloned()
        });
        if entry.is_some_and(|entry| {
            self.composer_selections
                .choose_project_model(project_id, &entry, model)
        }) {
            self.schedule_ui_state_save(cx);
            cx.notify();
        }
    }

    fn choose_new_task_reasoning_effort(
        &mut self,
        project_id: &str,
        provider: &str,
        effort: corbit_client::AgentReasoningEffort,
        cx: &mut Context<Self>,
    ) {
        let entry = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog
                .providers
                .iter()
                .find(|entry| entry.provider_id == provider && entry.available)
                .cloned()
        });
        if entry.is_some_and(|entry| {
            self.composer_selections
                .choose_project_reasoning_effort(project_id, &entry, effort)
        }) {
            self.schedule_ui_state_save(cx);
            cx.notify();
        }
    }

    fn remember_new_task_agent_selection(
        &mut self,
        agent_id: &str,
        provider: &str,
        options: Option<&corbit_client::AgentPromptOptions>,
    ) {
        let Some(options) = options else {
            return;
        };
        let entry = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog
                .providers
                .iter()
                .find(|entry| entry.provider_id == provider && entry.available)
                .cloned()
        });
        if let Some(entry) = entry {
            self.composer_selections.set_agent_selection(
                agent_id,
                &entry,
                options.model.as_deref(),
                options.reasoning_effort,
            );
        }
    }

    pub(super) fn set_project_provider_preference(
        &mut self,
        project_id: &str,
        provider: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.provider_is_available(provider) {
            return;
        }
        self.project_providers
            .insert(project_id.to_owned(), provider.to_owned());
        self.reconcile_composer_catalog();
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn choose_new_task_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
        else {
            return;
        };
        self.selected_workspace_id = Some(workspace.id.clone());
        self.selected_project_id = Some(workspace.project_id.clone());
        self.main_section = MainSection::NewTask;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    pub(super) fn choose_default_provider(&mut self, provider: &str, cx: &mut Context<Self>) {
        if self
            .provider_options()
            .iter()
            .any(|(candidate, _, _)| *candidate == provider)
        {
            provider.clone_into(&mut self.selected_provider);
            self.schedule_ui_state_save(cx);
            cx.notify();
        }
    }

    pub(super) fn create_new_task(&mut self, cx: &mut Context<Self>) {
        if self.new_task_in_flight {
            self.show_warning("任务正在创建，请稍候", cx);
            return;
        }
        if self.new_task_recovery.is_some() {
            self.show_validation_error("请先重试或清理上一次未完成的任务", cx);
            return;
        }
        let prompt = Self::input_value(&self.new_task_prompt, cx);
        if prompt.is_empty() {
            self.show_validation_error("请先描述要完成的任务", cx);
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.show_validation_error("正在同步项目状态，请稍后重试", cx);
            return;
        };
        let location = match resolve_new_task_location(
            snapshot,
            self.selected_project_id.as_deref(),
            self.selected_workspace_id.as_deref(),
        ) {
            Ok(location) => location,
            Err(message) => {
                self.show_validation_error(message, cx);
                return;
            }
        };
        let provider = self.project_provider(&location.project_id);
        if let Some(message) = self.provider_prompt_blocker(&provider) {
            self.show_validation_error(message, cx);
            return;
        }
        if !matches!(self.state, corbit_client::ConnectionState::Online) {
            self.show_validation_error("请等待 Daemon 连接完成", cx);
            return;
        }
        self.selected_project_id = Some(location.project_id.clone());
        self.selected_workspace_id
            .clone_from(&location.workspace_id);
        let title = prompt
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("新任务")
            .trim()
            .chars()
            .take(48)
            .collect::<String>();
        let stage = if location.workspace_id.is_some() {
            NewTaskRecoveryStage::Create
        } else {
            NewTaskRecoveryStage::Workspace
        };
        let prompt_options = provider_supports_turn_options(&provider)
            .then(|| self.new_task_prompt_options(&location.project_id, &provider));
        let recovery = PendingNewTaskRecovery {
            project_id: Some(location.project_id),
            workspace_id: location.workspace_id,
            workspace_name: Some(location.workspace_name),
            working_directory: Some(location.working_directory),
            provider,
            title,
            prompt,
            prompt_options,
            agent_id: None,
            stage,
            workspace_create_mutation_id: format!("new_task_workspace_{}", uuid::Uuid::new_v4()),
            create_mutation_id: format!("new_task_{}", uuid::Uuid::new_v4()),
            start_mutation_id: format!("new_task_start_{}", uuid::Uuid::new_v4()),
            prompt_mutation_id: format!("new_task_prompt_{}", uuid::Uuid::new_v4()),
            cleanup_stop_mutation_id: format!("new_task_cleanup_stop_{}", uuid::Uuid::new_v4()),
            cleanup_delete_mutation_id: format!("new_task_cleanup_delete_{}", uuid::Uuid::new_v4()),
            error: String::new(),
        };
        self.run_new_task_attempt(recovery, cx);
    }

    fn run_new_task_attempt(&mut self, recovery: PendingNewTaskRecovery, cx: &mut Context<Self>) {
        if self.new_task_in_flight {
            self.show_warning("任务操作正在执行，请稍候", cx);
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

        let stage = recovery.stage;
        let provider = recovery.provider.clone();
        self.new_task_recovery = Some(recovery.clone());
        self.new_task_cleanup_armed = false;
        self.new_task_in_flight = true;
        self.operation_in_flight = true;
        self.detail = match stage {
            NewTaskRecoveryStage::Workspace => "正在准备项目工作区…".into(),
            NewTaskRecoveryStage::Create => {
                format!("正在创建 {} 任务…", Self::provider_label(&provider))
            }
            NewTaskRecoveryStage::Start => "正在重新启动未完成的任务…".into(),
            NewTaskRecoveryStage::Prompt => "正在重新提交首次 Prompt…".into(),
        };
        self.schedule_ui_state_save(cx);
        self.new_task_task = Some(cx.spawn(async move |view, cx| {
            let result = execute_new_task_attempt(client, recovery).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.new_task_in_flight = false;
                view.operation_in_flight = false;
                match result {
                    NewTaskAttemptResult::Completed {
                        agent_id,
                        snapshot,
                        turn_id,
                        provider,
                        prompt_options,
                    } => {
                        if let Some(snapshot) = snapshot {
                            view.snapshot = Some(snapshot);
                        }
                        view.remember_new_task_agent_selection(
                            &agent_id,
                            &provider,
                            prompt_options.as_ref(),
                        );
                        view.selected_agent_id = Some(agent_id);
                        view.reconcile_selection();
                        view.main_section = MainSection::Conversation;
                        view.new_task_recovery = None;
                        view.new_task_cleanup_armed = false;
                        view.new_task_clear_requested = true;
                        view.reset_timeline_list_to_selected();
                        view.show_success(format!("任务已启动 · Turn {turn_id}"), cx);
                    }
                    NewTaskAttemptResult::Failed { recovery, snapshot } => {
                        let recovery = *recovery;
                        if let Some(snapshot) = snapshot {
                            view.snapshot = Some(snapshot);
                        }
                        if let Some(project_id) = recovery.project_id.clone() {
                            view.selected_project_id = Some(project_id);
                        }
                        if let Some(workspace_id) = recovery.workspace_id.clone() {
                            view.selected_workspace_id = Some(workspace_id);
                        }
                        view.reconcile_selection();
                        if let Some(agent_id) = recovery.agent_id.clone() {
                            view.remember_new_task_agent_selection(
                                &agent_id,
                                &recovery.provider,
                                recovery.prompt_options.as_ref(),
                            );
                            view.selected_agent_id = Some(agent_id);
                            view.reconcile_selection();
                            view.main_section = MainSection::Conversation;
                        } else {
                            view.main_section = MainSection::NewTask;
                        }
                        view.show_error(recovery.error.clone(), cx);
                        view.new_task_recovery = Some(recovery);
                    }
                }
                view.schedule_ui_state_save(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn retry_new_task_recovery(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = self.new_task_recovery.clone() else {
            return;
        };
        self.run_new_task_attempt(recovery, cx);
    }

    pub(super) fn request_new_task_cleanup(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = self.new_task_recovery.clone() else {
            return;
        };
        let Some(agent_id) = recovery.agent_id.clone() else {
            self.new_task_recovery = None;
            self.new_task_cleanup_armed = false;
            self.show_info("已放弃恢复此次任务创建", cx);
            self.schedule_ui_state_save(cx);
            return;
        };
        if !self.new_task_cleanup_armed {
            self.new_task_cleanup_armed = true;
            self.show_warning("清理会停止并删除未完成任务；请再次点击确认", cx);
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
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.show_validation_error("正在同步任务状态，请稍后重试", cx);
            return;
        };
        let Some(agent) = snapshot.agents.iter().find(|agent| agent.id == agent_id) else {
            self.new_task_recovery = None;
            self.new_task_cleanup_armed = false;
            self.show_info("未完成任务已不存在，无需继续清理", cx);
            self.schedule_ui_state_save(cx);
            return;
        };
        let needs_stop = agent.status != corbit_client::AgentStatus::Stopped;
        self.new_task_in_flight = true;
        self.operation_in_flight = true;
        self.detail = "正在清理未完成任务…".into();
        self.new_task_task = Some(cx.spawn(async move |view, cx| {
            let result = execute_new_task_cleanup(client, recovery, needs_stop).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.new_task_in_flight = false;
                view.operation_in_flight = false;
                match result {
                    NewTaskCleanupResult::Completed(snapshot) => {
                        view.snapshot = Some(snapshot);
                        view.new_task_recovery = None;
                        view.new_task_cleanup_armed = false;
                        view.reconcile_selection();
                        view.show_success("未完成任务已停止并删除", cx);
                    }
                    NewTaskCleanupResult::Failed { recovery, snapshot } => {
                        let recovery = *recovery;
                        if let Some(snapshot) = snapshot {
                            view.snapshot = Some(snapshot);
                            view.reconcile_selection();
                        }
                        view.show_error(recovery.error.clone(), cx);
                        view.new_task_recovery = Some(recovery);
                    }
                }
                view.schedule_ui_state_save(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_new_task_recovery_banner(
        &self,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let recovery = self.new_task_recovery.as_ref()?;
        let (title, retry_label) = match recovery.stage {
            NewTaskRecoveryStage::Workspace => ("项目工作区尚未准备完成", "重试准备"),
            NewTaskRecoveryStage::Create => ("任务创建尚未完成", "重试创建"),
            NewTaskRecoveryStage::Start => ("任务已创建，等待启动", "重试启动"),
            NewTaskRecoveryStage::Prompt => ("Agent 已启动，等待首次 Prompt", "重试提交"),
        };
        let recovery_detail = if recovery.error.trim().is_empty() {
            match recovery.stage {
                NewTaskRecoveryStage::Workspace => "正在准备默认工作区，请稍候…".into(),
                NewTaskRecoveryStage::Create => "正在创建任务，请稍候…".into(),
                NewTaskRecoveryStage::Start => "正在启动 Agent，请稍候…".into(),
                NewTaskRecoveryStage::Prompt => "正在提交首次 Prompt，请稍候…".into(),
            }
        } else {
            recovery.error.clone()
        };
        let has_agent = recovery.agent_id.is_some();
        let cleanup_label = if !has_agent {
            "放弃恢复"
        } else if self.new_task_cleanup_armed {
            "确认停止并删除"
        } else {
            "清理未完成任务"
        };
        let cleanup_button = Button::new("new-task-recovery-cleanup")
            .small()
            .icon(Icon::new(if has_agent {
                AppIcon::Delete
            } else {
                AppIcon::Close
            }))
            .label(cleanup_label)
            .disabled(self.new_task_in_flight)
            .on_click(cx.listener(|view, _, _, cx| {
                view.request_new_task_cleanup(cx);
            }));
        let cleanup_button = if has_agent {
            cleanup_button.danger()
        } else {
            cleanup_button.outline()
        };

        Some(
            div()
                .v_flex()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(COLOR_WARNING))
                .bg(rgb(COLOR_SURFACE_UNDER))
                .p_4()
                .child(
                    div()
                        .h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .h_flex()
                                .size(px(28.))
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .bg(rgb(COLOR_SURFACE_SECONDARY))
                                .child(
                                    Icon::new(AppIcon::Refresh)
                                        .size(px(15.))
                                        .text_color(rgb(COLOR_WARNING)),
                                ),
                        )
                        .child(
                            div()
                                .v_flex()
                                .gap_1()
                                .child(div().font_medium().child(title))
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .line_height(px(20.))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(recovery_detail),
                                ),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .pl(px(40.))
                        .child(
                            Button::new("new-task-recovery-retry")
                                .primary()
                                .small()
                                .icon(Icon::new(AppIcon::Refresh))
                                .label(retry_label)
                                .loading(self.new_task_in_flight)
                                .disabled(!is_online || self.new_task_in_flight)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.retry_new_task_recovery(cx);
                                })),
                        )
                        .child(cleanup_button),
                ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_new_task_panel(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let selected_project = self.snapshot.as_ref().and_then(|snapshot| {
            let project_id = self.selected_project_id.as_ref()?;
            snapshot
                .projects
                .iter()
                .find(|project| &project.id == project_id)
                .map(|project| (project.name.clone(), project.root_path.clone()))
        });
        let effective_workspace_id = self.snapshot.as_ref().and_then(|snapshot| {
            resolve_new_task_location(
                snapshot,
                self.selected_project_id.as_deref(),
                self.selected_workspace_id.as_deref(),
            )
            .ok()
            .and_then(|location| location.workspace_id)
        });
        let workspaces = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .filter(|workspace| {
                        workspace.status == corbit_client::WorkspaceStatus::Active
                            && self.selected_project_id.as_ref() == Some(&workspace.project_id)
                    })
                    .enumerate()
                    .map(|(index, workspace)| {
                        let workspace_id = workspace.id.clone();
                        Button::new(("new-task-workspace", index))
                            .ghost()
                            .small()
                            .h(navigation_row_height())
                            .w_full()
                            .justify_start()
                            .selected(effective_workspace_id.as_ref() == Some(&workspace.id))
                            .icon(Icon::new(AppIcon::Workspace))
                            .label(format!(
                                "{}  ·  {}",
                                workspace.name, workspace.working_directory
                            ))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.choose_new_task_workspace(&workspace_id, cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let provider_options = self.provider_options();
        let has_providers = !provider_options.is_empty();
        let selected_project_provider = self.selected_project_provider();
        let selected_provider_description = provider_options
            .iter()
            .find(|(provider, _, _)| *provider == selected_project_provider)
            .map_or(
                "当前 Daemon 没有提供可用 Agent。",
                |(_, label, description)| {
                    if description.is_empty() {
                        *label
                    } else {
                        *description
                    }
                },
            )
            .to_owned();
        let providers = provider_options
            .into_iter()
            .enumerate()
            .map(|(index, (provider, label, _))| {
                Button::new(("new-task-provider", index))
                    .outline()
                    .small()
                    .selected(selected_project_provider == provider)
                    .label(label)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        let Some(project_id) = view.selected_project_id.clone() else {
                            return;
                        };
                        view.set_project_provider_preference(&project_id, provider, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let selected_project_id = self.selected_project_id.clone();
        let prompt_blocker = self.provider_prompt_blocker(&selected_project_provider);
        let supports_turn_options = provider_supports_turn_options(&selected_project_provider);
        let selected_model = selected_project_id
            .as_deref()
            .and_then(|project_id| self.new_task_model_info(project_id, &selected_project_provider))
            .cloned();
        let model_label = selected_model.as_ref().map_or_else(
            || {
                if prompt_blocker.is_some() {
                    "模型不可用".to_owned()
                } else {
                    "Provider 默认".to_owned()
                }
            },
            |model| model_display_name(&model.id, &model.display_name),
        );
        let model_choices =
            self.provider_catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog.providers.iter().find(|entry| {
                        entry.provider_id == selected_project_provider && entry.available
                    })
                })
                .map(|entry| entry.models.clone())
                .unwrap_or_default();
        let model_view = cx.entity();
        let model_project_id = selected_project_id.clone().unwrap_or_default();
        let model_provider = selected_project_provider.clone();
        let selected_model_for_menu = selected_model.clone();
        let model_button = Button::new("new-task-model")
            .outline()
            .small()
            .label(model_label)
            .tooltip(if supports_turn_options {
                "选择模型"
            } else {
                "当前 Provider 使用自己的默认模型"
            })
            .disabled(
                !is_online
                    || self.new_task_in_flight
                    || self.new_task_recovery.is_some()
                    || selected_project_id.is_none()
                    || !supports_turn_options
                    || model_choices.is_empty(),
            )
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(260.)).max_w(px(420.));
                for model in model_choices.clone() {
                    let item_view = model_view.clone();
                    let item_project_id = model_project_id.clone();
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
                                view.choose_new_task_model(
                                    &item_project_id,
                                    &item_provider,
                                    &item_model,
                                    cx,
                                );
                            });
                        }),
                    );
                }
                menu
            });
        let reasoning_effort = selected_project_id.as_deref().and_then(|project_id| {
            self.new_task_reasoning_effort(project_id, &selected_project_provider)
        });
        let reasoning_choices = selected_model
            .as_ref()
            .map(|model| model.supported_reasoning_efforts.clone())
            .unwrap_or_default();
        let reasoning_view = cx.entity();
        let reasoning_project_id = selected_project_id.clone().unwrap_or_default();
        let reasoning_provider = selected_project_provider.clone();
        let reasoning_button = Button::new("new-task-reasoning")
            .outline()
            .small()
            .label(reasoning_effort.map_or("默认", reasoning_effort_short_label))
            .tooltip(if supports_turn_options {
                "选择推理等级"
            } else {
                "当前 Provider 管理自己的推理程度"
            })
            .disabled(
                !is_online
                    || self.new_task_in_flight
                    || self.new_task_recovery.is_some()
                    || selected_project_id.is_none()
                    || !supports_turn_options
                    || reasoning_choices.is_empty(),
            )
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(240.)).max_w(px(360.));
                for choice in reasoning_choices.clone() {
                    let effort = choice.reasoning_effort;
                    let item_view = reasoning_view.clone();
                    let item_project_id = reasoning_project_id.clone();
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
                                view.choose_new_task_reasoning_effort(
                                    &item_project_id,
                                    &item_provider,
                                    effort,
                                    cx,
                                );
                            });
                        }),
                    );
                }
                menu
            });
        let can_create = is_online
            && !self.new_task_in_flight
            && self.new_task_recovery.is_none()
            && selected_project.is_some()
            && has_providers
            && prompt_blocker.is_none()
            && !self.new_task_prompt.read(cx).value().trim().is_empty();
        let new_task_hint = prompt_blocker.unwrap_or(selected_provider_description);
        let recovery_banner = self.render_new_task_recovery_banner(is_online, cx);
        let has_workspaces = !workspaces.is_empty();
        let project_card = match selected_project.as_ref() {
            Some((name, root_path)) => div()
                .h_flex()
                .items_center()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(COLOR_BORDER))
                .bg(rgb(COLOR_SURFACE_UNDER))
                .p_3()
                .child(
                    div()
                        .h_flex()
                        .size(px(32.))
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .child(Icon::new(AppIcon::Project).size(px(16.))),
                )
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_1()
                        .child(div().font_medium().child(name.clone()))
                        .child(
                            div()
                                .truncate()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(root_path.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("当前项目"),
                ),
            None => div()
                .v_flex()
                .items_center()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(COLOR_BORDER))
                .bg(rgb(COLOR_SURFACE_UNDER))
                .p_5()
                .child(Icon::new(AppIcon::Project).text_color(rgb(COLOR_TEXT_TERTIARY)))
                .child(div().font_medium().child("先选择一个项目"))
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("从左侧项目列表选择代码文件夹，或点击“+”添加项目。"),
                ),
        };

        div().size_full().child(
            div().size_full().overflow_y_scrollbar().child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(content_max_width())
                    .mx_auto()
                    .px_8()
                    .pt(px(72.))
                    .pb_8()
                    .gap_6()
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .gap_3()
                            .child(brand_mark(38.))
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_HEADING))
                                    .font_semibold()
                                    .child("开始一个任务"),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("描述目标即可；Corbit 会自动准备工作区、创建 Agent 并进入对话。"),
                            ),
                    )
                    .when_some(recovery_banner, gpui::ParentElement::child)
                    .child(project_card)
                    .child(
                        div()
                            .v_flex()
                            .rounded_xl()
                            .border_1()
                            .border_color(rgb(COLOR_BORDER_HEAVY))
                            .bg(rgb(COLOR_SURFACE))
                            .p_3()
                            .gap_3()
                            .child(Input::new(&self.new_task_prompt).appearance(false))
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
                                                div()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                    .child("执行 Agent"),
                                            )
                                            .children(providers),
                                    )
                                    .child(model_button)
                                    .child(reasoning_button)
                                    .child(
                                        Button::new("start-new-task")
                                            .primary()
                                            .small()
                                            .icon(Icon::new(AppIcon::Send))
                                            .label("开始任务")
                                            .loading(self.new_task_in_flight)
                                            .disabled(!can_create)
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.create_new_task(cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(format!(
                                        "{new_task_hint} · 提交后自动创建并启动，无需预先选择 Agent"
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("工作目录"),
                            )
                            .when(selected_project.is_some() && !has_workspaces, |list| {
                                list.child(
                                    div()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(rgb(COLOR_BORDER))
                                        .bg(rgb(COLOR_SURFACE_UNDER))
                                        .p_4()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(
                                            "首次提交时会自动使用项目根目录创建默认工作区，无需额外设置。",
                                        ),
                                )
                            })
                            .when(selected_project.is_none(), |list| {
                                list.child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                                        .child("选择项目后将自动匹配工作目录。"),
                                )
                            })
                            .children(workspaces),
                    ),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_tasks_panel(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let agents = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .rev()
                    .filter(|agent| self.task_matches_filter(agent))
                    .enumerate()
                    .map(|(index, agent)| {
                        let agent_id = agent.id.clone();
                        let action_agent_id = agent.id.clone();
                        let settings_agent_id = agent.id.clone();
                        let workspace = snapshot
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.id == agent.workspace_id);
                        let status = match agent.status {
                            corbit_client::AgentStatus::Initializing => "正在初始化",
                            corbit_client::AgentStatus::Idle => "可启动",
                            corbit_client::AgentStatus::Running => "运行中",
                            corbit_client::AgentStatus::Error => "需要处理",
                            corbit_client::AgentStatus::Stopped => "已停止",
                        };
                        let status_color = match agent.status {
                            corbit_client::AgentStatus::Running => rgb(COLOR_SUCCESS),
                            corbit_client::AgentStatus::Error => rgb(COLOR_ERROR),
                            corbit_client::AgentStatus::Initializing => rgb(COLOR_WARNING),
                            corbit_client::AgentStatus::Idle
                            | corbit_client::AgentStatus::Stopped => rgb(COLOR_TEXT_TERTIARY),
                        };
                        let action_button = match agent.status {
                            corbit_client::AgentStatus::Initializing
                            | corbit_client::AgentStatus::Running => {
                                Button::new(("task-stop", index))
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::Stop))
                                    .tooltip("停止任务")
                                    .disabled(!is_online || self.operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.stop_agent(&action_agent_id, cx);
                                    }))
                            }
                            corbit_client::AgentStatus::Idle
                            | corbit_client::AgentStatus::Error => {
                                Button::new(("task-start", index))
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::Play))
                                    .tooltip("启动任务")
                                    .disabled(!is_online || self.operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.start_agent(&action_agent_id, cx);
                                    }))
                            }
                            corbit_client::AgentStatus::Stopped => {
                                let confirming = self.delete_confirmation
                                    == Some(DeleteTarget::Agent(agent.id.clone()));
                                Button::new(("task-delete", index))
                                    .danger()
                                    .small()
                                    .icon(Icon::new(AppIcon::Delete))
                                    .tooltip(if confirming {
                                        "确认删除任务"
                                    } else {
                                        "删除任务"
                                    })
                                    .disabled(!is_online || self.operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.request_agent_delete(&action_agent_id, cx);
                                    }))
                            }
                        };
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .border_b_1()
                            .border_color(rgb(COLOR_BORDER_LIGHT))
                            .child(
                                Button::new(("task-overview-row", index))
                                    .ghost()
                                    .flex_1()
                                    .justify_start()
                                    .selected(self.selected_agent_id.as_ref() == Some(&agent.id))
                                    .child(
                                        div()
                                            .h_flex()
                                            .w_full()
                                            .items_center()
                                            .gap_3()
                                            .py_2()
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .size(px(28.))
                                                    .flex_none()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded_md()
                                                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                                                    .child(Icon::new(AppIcon::Agent).size(px(15.))),
                                            )
                                            .child(
                                                div()
                                                    .v_flex()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .items_start()
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .truncate()
                                                            .text_size(font_px(FONT_SIZE_BASE))
                                                            .font_medium()
                                                            .child(agent.title.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_size(font_px(FONT_SIZE_XS))
                                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                            .child(format!(
                                                                "{} · {}",
                                                                Self::provider_label(
                                                                    &agent.provider
                                                                ),
                                                                workspace.map_or(
                                                                    "未知工作区",
                                                                    |workspace| workspace
                                                                        .name
                                                                        .as_str()
                                                                )
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .gap_2()
                                                    .text_size(font_px(FONT_SIZE_SM))
                                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                                    .child(
                                                        div()
                                                            .size(px(6.))
                                                            .rounded_full()
                                                            .bg(status_color),
                                                    )
                                                    .child(status),
                                            ),
                                    )
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.select_agent(&agent_id, cx);
                                    })),
                            )
                            .child(action_button)
                            .child(
                                Button::new(("task-settings", index))
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::Settings))
                                    .tooltip("任务设置")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.select_agent_in_settings(&settings_agent_id, cx);
                                    })),
                            )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let (running, total) = self.snapshot.as_ref().map_or((0, 0), |snapshot| {
            (
                snapshot
                    .agents
                    .iter()
                    .filter(|agent| agent.status == corbit_client::AgentStatus::Running)
                    .count(),
                snapshot.agents.len(),
            )
        });
        let filtered_total = agents.len();
        let has_any_tasks = total > 0;
        let filter_buttons = [
            (TaskFilter::All, "全部"),
            (TaskFilter::Active, "运行中"),
            (TaskFilter::Attention, "需要处理"),
            (TaskFilter::Stopped, "未运行"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (filter, label))| {
            Button::new(("task-filter", index))
                .ghost()
                .small()
                .selected(self.task_filter == filter)
                .label(label)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.set_task_filter(filter, cx);
                }))
        })
        .collect::<Vec<_>>();

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
                            .h_flex()
                            .justify_between()
                            .items_end()
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_HEADING))
                                            .font_semibold()
                                            .child("任务"),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                                            .child(format!(
                                                "{running} 个运行中 · 共 {total} 个任务"
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("tasks-new-task")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(AppIcon::Add))
                                    .label("新建任务")
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::NewTask, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .border_b_1()
                            .border_color(rgb(COLOR_BORDER_LIGHT))
                            .pb_3()
                            .child(div().h_flex().gap_1().children(filter_buttons))
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(format!("显示 {filtered_total} 个")),
                            ),
                    )
                    .when(!has_any_tasks, |panel| {
                        panel.child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_BORDER))
                                .bg(rgb(COLOR_SURFACE_UNDER))
                                .p_5()
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child("还没有任务。创建一个任务后，它会出现在这里。"),
                        )
                    })
                    .when(has_any_tasks && agents.is_empty(), |panel| {
                        panel.child(
                            div()
                                .v_flex()
                                .items_center()
                                .gap_2()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_BORDER))
                                .bg(rgb(COLOR_SURFACE_UNDER))
                                .p_6()
                                .child(
                                    Icon::new(AppIcon::Search).text_color(rgb(COLOR_TEXT_TERTIARY)),
                                )
                                .child(div().font_medium().child("当前筛选下没有任务"))
                                .child(
                                    Button::new("task-filter-reset")
                                        .ghost()
                                        .small()
                                        .label("查看全部任务")
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.set_task_filter(TaskFilter::All, cx);
                                        })),
                                ),
                        )
                    })
                    .children(agents),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_recovery_round_trips_without_regenerating_mutation_ids() {
        let recovery = PendingNewTaskRecovery {
            project_id: Some("project-1".into()),
            workspace_id: Some("workspace-1".into()),
            workspace_name: Some("Corbit".into()),
            working_directory: Some("/work/corbit".into()),
            provider: "codex".into(),
            title: "继续完成界面逻辑".into(),
            prompt: "完成任务恢复流程并补齐测试".into(),
            prompt_options: Some(corbit_client::AgentPromptOptions {
                model: Some("gpt-5.4".into()),
                permission_mode: Some(corbit_client::AgentPermissionMode::WorkspaceWrite),
                reasoning_effort: Some(corbit_client::AgentReasoningEffort::High),
                network_access: Some(true),
                reasoning_summary: Some(corbit_client::AgentReasoningSummary::Concise),
                personality: Some(corbit_client::AgentPersonality::Pragmatic),
                attachments: Vec::new(),
            }),
            agent_id: Some("agent-1".into()),
            stage: NewTaskRecoveryStage::Prompt,
            workspace_create_mutation_id: "workspace-fixed".into(),
            create_mutation_id: "create-fixed".into(),
            start_mutation_id: "start-fixed".into(),
            prompt_mutation_id: "prompt-fixed".into(),
            cleanup_stop_mutation_id: "cleanup-stop-fixed".into(),
            cleanup_delete_mutation_id: "cleanup-delete-fixed".into(),
            error: "Prompt 提交失败：连接中断".into(),
        };

        let encoded = serde_json::to_string(&recovery).expect("recovery should serialize");
        let decoded: PendingNewTaskRecovery =
            serde_json::from_str(&encoded).expect("recovery should deserialize");

        assert_eq!(decoded, recovery);
        assert_eq!(decoded.workspace_create_mutation_id, "workspace-fixed");
        assert_eq!(decoded.create_mutation_id, "create-fixed");
        assert_eq!(decoded.start_mutation_id, "start-fixed");
        assert_eq!(decoded.prompt_mutation_id, "prompt-fixed");
        assert_eq!(decoded.cleanup_stop_mutation_id, "cleanup-stop-fixed");
        assert_eq!(decoded.cleanup_delete_mutation_id, "cleanup-delete-fixed");
        assert_eq!(
            decoded
                .prompt_options
                .as_ref()
                .and_then(|options| options.model.as_deref()),
            Some("gpt-5.4")
        );
    }

    #[test]
    fn legacy_new_task_recovery_remains_readable() {
        let legacy = serde_json::json!({
            "workspaceId": "workspace-1",
            "provider": "codex",
            "title": "Legacy task",
            "prompt": "Continue",
            "agentId": "agent-1",
            "stage": "prompt",
            "createMutationId": "create-fixed",
            "startMutationId": "start-fixed",
            "promptMutationId": "prompt-fixed",
            "cleanupStopMutationId": "cleanup-stop-fixed",
            "cleanupDeleteMutationId": "cleanup-delete-fixed",
            "error": ""
        });

        let decoded: PendingNewTaskRecovery =
            serde_json::from_value(legacy).expect("legacy recovery should deserialize");

        assert_eq!(decoded.project_id, None);
        assert_eq!(decoded.workspace_id.as_deref(), Some("workspace-1"));
        assert!(decoded.workspace_create_mutation_id.is_empty());
        assert_eq!(decoded.prompt_options, None);
    }

    #[test]
    fn new_project_uses_its_root_as_the_automatic_workspace() {
        let snapshot = corbit_client::AuthoritativeSnapshot {
            schema_version: 1,
            generated_at: String::new(),
            revision: 1,
            projects: vec![corbit_client::ProjectResource {
                id: "project-1".into(),
                name: "Corbit".into(),
                root_path: "/work/corbit".into(),
                created_at: String::new(),
                updated_at: String::new(),
                extensions: std::collections::BTreeMap::new(),
            }],
            workspaces: Vec::new(),
            agents: Vec::new(),
            extensions: std::collections::BTreeMap::new(),
        };

        let location = resolve_new_task_location(&snapshot, Some("project-1"), None)
            .expect("a selected project should be enough to start a task");

        assert_eq!(location.project_id, "project-1");
        assert_eq!(location.workspace_id, None);
        assert_eq!(location.workspace_name, "Corbit");
        assert_eq!(location.working_directory, "/work/corbit");
    }

    #[test]
    fn task_creation_falls_back_to_an_active_workspace_in_the_selected_project() {
        let mut snapshot = corbit_client::AuthoritativeSnapshot {
            schema_version: 1,
            generated_at: String::new(),
            revision: 1,
            projects: vec![corbit_client::ProjectResource {
                id: "project-1".into(),
                name: "Corbit".into(),
                root_path: "/work/corbit".into(),
                created_at: String::new(),
                updated_at: String::new(),
                extensions: std::collections::BTreeMap::new(),
            }],
            workspaces: Vec::new(),
            agents: Vec::new(),
            extensions: std::collections::BTreeMap::new(),
        };
        snapshot.workspaces = vec![
            corbit_client::WorkspaceResource {
                id: "workspace-archived".into(),
                project_id: "project-1".into(),
                name: "Archived".into(),
                working_directory: "/work/corbit/archived".into(),
                status: corbit_client::WorkspaceStatus::Archived,
                created_at: String::new(),
                updated_at: String::new(),
                extensions: std::collections::BTreeMap::new(),
            },
            corbit_client::WorkspaceResource {
                id: "workspace-active".into(),
                project_id: "project-1".into(),
                name: "Active".into(),
                working_directory: "/work/corbit".into(),
                status: corbit_client::WorkspaceStatus::Active,
                created_at: String::new(),
                updated_at: String::new(),
                extensions: std::collections::BTreeMap::new(),
            },
        ];

        let location =
            resolve_new_task_location(&snapshot, Some("project-1"), Some("workspace-archived"))
                .expect("an active workspace should be selected automatically");

        assert_eq!(location.workspace_id.as_deref(), Some("workspace-active"));
    }
}
