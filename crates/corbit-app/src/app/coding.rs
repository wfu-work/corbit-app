use super::settings::settings_page_header;
use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GitMergeMethod {
    #[default]
    Merge,
    Squash,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GitReviewPresentation {
    #[default]
    Inline,
    Separate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct GitPreferences {
    pub(super) branch_prefix: String,
    pub(super) merge_method: GitMergeMethod,
    pub(super) force_with_lease: bool,
    pub(super) draft_pull_requests: bool,
    pub(super) review_presentation: GitReviewPresentation,
    pub(super) auto_merge_when_ready: bool,
    pub(super) monitoring_instructions: String,
    pub(super) commit_instructions: String,
}

impl Default for GitPreferences {
    fn default() -> Self {
        Self {
            branch_prefix: "corbit/".into(),
            merge_method: GitMergeMethod::Merge,
            force_with_lease: false,
            draft_pull_requests: true,
            review_presentation: GitReviewPresentation::Inline,
            auto_merge_when_ready: false,
            monitoring_instructions: String::new(),
            commit_instructions: String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct SshConnectionProfile {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) host: String,
    pub(super) user: String,
    pub(super) port: u16,
    pub(super) identity_file: String,
}

impl Default for SshConnectionProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            user: String::new(),
            port: 22,
            identity_file: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct CodingPreferences {
    pub(super) git: GitPreferences,
    pub(super) ssh_connections: Vec<SshConnectionProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SshConnectionTestState {
    Testing,
    Reachable(String),
    Failed(String),
}

#[derive(Clone, Debug)]
struct HookSource {
    title: String,
    source: String,
    path: PathBuf,
    state: HookSourceState,
}

#[derive(Clone, Debug)]
enum HookSourceState {
    Ready(usize),
    Invalid(String),
    Declared,
}

fn coding_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    value: impl IntoElement,
) -> Div {
    div()
        .h_flex()
        .min_h(px(58.))
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap_6()
        .child(
            div()
                .v_flex()
                .min_w(px(0.))
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .font_medium()
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .line_height(px(18.))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(description.into()),
                ),
        )
        .child(value)
}

fn coding_divider() -> Div {
    div().h(px(1.)).w_full().bg(rgb(COLOR_BORDER_LIGHT))
}

fn coding_option_group(options: impl IntoIterator<Item = Button>) -> Div {
    div()
        .h_flex()
        .flex_none()
        .gap_1()
        .rounded(px(8.))
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .p(px(2.))
        .children(options)
}

fn compact_status_badge(label: impl Into<SharedString>, color: u32) -> Div {
    div()
        .h_flex()
        .flex_none()
        .items_center()
        .gap_2()
        .rounded_full()
        .border_1()
        .border_color(rgb(COLOR_BORDER_LIGHT))
        .bg(rgb(COLOR_SURFACE_UNDER))
        .px_2()
        .py_1()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(COLOR_TEXT_SECONDARY))
        .child(div().size(px(6.)).rounded_full().bg(rgb(color)))
        .child(label.into())
}

fn input_field(label: &'static str, description: &'static str, input: Input) -> Div {
    div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .font_medium()
                        .child(label),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(description),
                ),
        )
        .child(input)
}

fn validate_ssh_component(value: &str, field: &str) -> Result<(), String> {
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
        || value.starts_with('-')
    {
        return Err(format!("{field}包含不支持的字符"));
    }
    Ok(())
}

fn validate_branch_prefix(prefix: &str) -> Result<(), &'static str> {
    if prefix.is_empty() {
        return Err("请输入分支前缀");
    }
    if prefix.starts_with('/')
        || prefix.contains("..")
        || prefix.ends_with('.')
        || prefix
            .chars()
            .any(|character| character.is_whitespace() || "~^:?*[\\".contains(character))
    {
        return Err("分支前缀包含 Git 不支持的字符");
    }
    Ok(())
}

fn test_ssh_profile(profile: &SshConnectionProfile) -> Result<String, String> {
    let mut command = Command::new("ssh");
    command
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=6")
        .arg("-o")
        .arg("ConnectionAttempts=1")
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-p")
        .arg(profile.port.to_string())
        .stdin(Stdio::null());
    if !profile.identity_file.is_empty() {
        command.arg("-i").arg(&profile.identity_file);
    }
    let destination = if profile.user.is_empty() {
        profile.host.clone()
    } else {
        format!("{}@{}", profile.user, profile.host)
    };
    let output = command
        .arg(destination)
        .arg("printf corbit-ssh-ready")
        .output()
        .map_err(|error| format!("无法启动系统 SSH：{error}"))?;
    if output.status.success() && output.stdout == b"corbit-ssh-ready" {
        return Ok("SSH 握手和远程命令验证成功".into());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("SSH 连接未通过验证")
        .trim();
    Err(if detail.contains("Host key verification failed") {
        "主机密钥尚未信任，请先核对主机指纹并在终端完成首次连接".into()
    } else if detail.contains("Permission denied") {
        "身份验证失败，请检查 SSH Agent、用户名或身份文件".into()
    } else {
        detail.chars().take(220).collect()
    })
}

fn ssh_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ssh").join("config"))
}

fn imported_ssh_profiles() -> Result<Vec<SshConnectionProfile>, String> {
    let path = ssh_config_path().ok_or_else(|| "无法确定当前用户目录".to_owned())?;
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    let mut profiles = Vec::new();
    let mut current = Vec::<SshConnectionProfile>::new();

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(keyword) = fields.next() else {
            continue;
        };
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                profiles.append(&mut current);
                current = fields
                    .filter(|host| !host.contains(['*', '?', '!']) && !host.starts_with('-'))
                    .map(|host| SshConnectionProfile {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: host.to_owned(),
                        host: host.to_owned(),
                        ..SshConnectionProfile::default()
                    })
                    .collect();
            }
            "user" => {
                if let Some(value) = fields.next() {
                    for profile in &mut current {
                        value.clone_into(&mut profile.user);
                    }
                }
            }
            "port" => {
                if let Some(port) = fields.next().and_then(|value| value.parse().ok()) {
                    for profile in &mut current {
                        profile.port = port;
                    }
                }
            }
            "identityfile" => {
                if let Some(value) = fields.next() {
                    for profile in &mut current {
                        value.clone_into(&mut profile.identity_file);
                    }
                }
            }
            _ => {}
        }
    }
    profiles.extend(current);
    Ok(profiles)
}

fn corbit_home() -> Option<PathBuf> {
    std::env::var_os("CORBIT_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".corbit")))
}

fn hook_count(value: &Value) -> usize {
    if let Some(hooks) = value.get("hooks").and_then(Value::as_array) {
        return hooks.len();
    }
    match value {
        Value::Array(values) => values.len(),
        Value::Object(values) => values.len(),
        _ => 0,
    }
}

fn hook_source_from_path(title: String, source: String, path: PathBuf) -> Option<HookSource> {
    if !path.is_file() {
        return None;
    }
    let state = match fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => HookSourceState::Ready(hook_count(&value)),
            Err(error) => HookSourceState::Invalid(format!("JSON 无效：{error}")),
        },
        Err(error) => HookSourceState::Invalid(format!("无法读取：{error}")),
    };
    Some(HookSource {
        title,
        source,
        path,
        state,
    })
}

fn plugin_hook_path(plugin: &corbit_client::PluginRecord) -> PathBuf {
    let relative = plugin
        .manifest
        .hooks
        .as_deref()
        .unwrap_or("hooks/hooks.json");
    PathBuf::from(&plugin.installed_path).join(relative)
}

impl ConnectionView {
    fn persist_coding_preferences(&mut self, success: &'static str, cx: &mut Context<Self>) {
        match self.ui_preferences(cx).save() {
            Ok(()) => {
                self.ui_state_error = None;
                self.show_success(success, cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.ui_state_error = Some(message.clone());
                self.show_error(format!("编码设置保存失败：{message}"), cx);
            }
        }
    }

    pub(super) fn add_ssh_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = Self::input_value(&self.ssh_connection_name, cx);
        let host = Self::input_value(&self.ssh_connection_host, cx);
        let user = Self::input_value(&self.ssh_connection_user, cx);
        let port = Self::input_value(&self.ssh_connection_port, cx)
            .parse::<u16>()
            .map_err(|_| "SSH 端口必须是 1 到 65535 之间的数字".to_owned());
        let identity_file = Self::input_value(&self.ssh_connection_identity_file, cx);

        let result = (|| {
            if host.is_empty() {
                return Err("请输入 SSH 主机或配置别名".to_owned());
            }
            validate_ssh_component(&host, "SSH 主机")?;
            if !user.is_empty() {
                validate_ssh_component(&user, "SSH 用户名")?;
            }
            let port = port?;
            if self
                .coding_preferences
                .ssh_connections
                .iter()
                .any(|profile| profile.host == host && profile.user == user && profile.port == port)
            {
                return Err("这个 SSH 连接已经存在".to_owned());
            }
            Ok(SshConnectionProfile {
                id: uuid::Uuid::new_v4().to_string(),
                name: if name.is_empty() { host.clone() } else { name },
                host,
                user,
                port,
                identity_file,
            })
        })();

        match result {
            Ok(profile) => {
                self.coding_preferences.ssh_connections.push(profile);
                for input in [
                    &self.ssh_connection_name,
                    &self.ssh_connection_host,
                    &self.ssh_connection_user,
                    &self.ssh_connection_identity_file,
                ] {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                self.ssh_connection_port
                    .update(cx, |input, cx| input.set_value("22", window, cx));
                self.persist_coding_preferences("SSH 连接已保存", cx);
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    pub(super) fn import_ssh_config(&mut self, cx: &mut Context<Self>) {
        match imported_ssh_profiles() {
            Ok(profiles) => {
                let mut imported = 0;
                for profile in profiles {
                    let exists = self
                        .coding_preferences
                        .ssh_connections
                        .iter()
                        .any(|saved| saved.name == profile.name || saved.host == profile.host);
                    if !exists {
                        self.coding_preferences.ssh_connections.push(profile);
                        imported += 1;
                    }
                }
                if imported == 0 {
                    self.show_info("~/.ssh/config 中没有可导入的新主机", cx);
                } else {
                    self.persist_coding_preferences("SSH 配置已导入", cx);
                }
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    pub(super) fn request_ssh_connection_delete(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.pending_ssh_connection_delete.as_deref() != Some(profile_id.as_str()) {
            self.pending_ssh_connection_delete = Some(profile_id);
            self.show_warning("再次点击即可移除这个 SSH 连接；不会删除 ~/.ssh/config", cx);
            return;
        }
        self.coding_preferences
            .ssh_connections
            .retain(|profile| profile.id != profile_id);
        self.ssh_connection_tests.remove(&profile_id);
        self.pending_ssh_connection_delete = None;
        self.persist_coding_preferences("SSH 连接已移除", cx);
    }

    pub(super) fn test_ssh_connection(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.ssh_connection_task.is_some() {
            self.show_warning("另一个 SSH 连接正在测试", cx);
            return;
        }
        let Some(profile) = self
            .coding_preferences
            .ssh_connections
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
        else {
            return;
        };
        self.ssh_connection_tests
            .insert(profile_id.clone(), SshConnectionTestState::Testing);
        let test = cx.background_spawn(async move { test_ssh_profile(&profile) });
        self.ssh_connection_task = Some(cx.spawn(async move |view, cx| {
            let result = test.await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.ssh_connection_task = None;
                let state = match result {
                    Ok(detail) => SshConnectionTestState::Reachable(detail),
                    Err(detail) => SshConnectionTestState::Failed(detail),
                };
                view.ssh_connection_tests.insert(profile_id, state);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_git_text_settings(&mut self, cx: &mut Context<Self>) {
        let branch_prefix = Self::input_value(&self.git_branch_prefix, cx);
        if let Err(message) = validate_branch_prefix(&branch_prefix) {
            self.show_error(message, cx);
            return;
        }
        self.coding_preferences.git.branch_prefix = branch_prefix;
        self.coding_preferences.git.monitoring_instructions = self
            .git_monitoring_instructions
            .read(cx)
            .value()
            .to_string();
        self.coding_preferences.git.commit_instructions =
            self.git_commit_instructions.read(cx).value().to_string();
        self.persist_coding_preferences("Git 设置已保存", cx);
    }

    fn update_git_preference(
        &mut self,
        update: impl FnOnce(&mut GitPreferences),
        cx: &mut Context<Self>,
    ) {
        update(&mut self.coding_preferences.git);
        self.persist_coding_preferences("Git 设置已更新", cx);
    }

    fn hook_sources(&self) -> Vec<HookSource> {
        let mut sources = Vec::new();
        if let Some(home) = corbit_home() {
            for path in [
                home.join("hooks.json"),
                home.join("hooks").join("hooks.json"),
            ] {
                if let Some(source) =
                    hook_source_from_path("用户钩子".into(), "用户配置".into(), path)
                {
                    sources.push(source);
                }
            }
        }
        if let Some(project) = self.selected_project_id.as_ref().and_then(|project_id| {
            self.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .projects
                    .iter()
                    .find(|project| &project.id == project_id)
            })
        }) {
            let path = Path::new(&project.root_path)
                .join(".corbit")
                .join("hooks.json");
            if let Some(source) = hook_source_from_path(
                format!("{} 的项目钩子", project.name),
                "项目配置".into(),
                path,
            ) {
                sources.push(source);
            }
        }
        for plugin in self
            .plugins
            .iter()
            .filter(|plugin| plugin.enabled && plugin.components.has_hooks)
        {
            let path = plugin_hook_path(plugin);
            let state = if path.is_file() {
                hook_source_from_path(
                    plugin.manifest.name.clone(),
                    "已启用插件".into(),
                    path.clone(),
                )
                .map_or(HookSourceState::Declared, |source| source.state)
            } else {
                HookSourceState::Declared
            };
            sources.push(HookSource {
                title: plugin.manifest.name.clone(),
                source: "已启用插件".into(),
                path,
                state,
            });
        }
        sources
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_ssh_connection_settings(&self, cx: &mut Context<Self>) -> Div {
        let mut page = settings_page_header(
            "连接",
            "管理从这台 Mac 发起的 SSH 连接；凭证继续由 SSH Agent 和系统 SSH 配置负责。",
        );

        if self.coding_preferences.ssh_connections.is_empty() {
            page = page.child(
                settings_card("来自这台 Mac 的 SSH 连接").child(
                    div()
                        .v_flex()
                        .items_center()
                        .gap_3()
                        .py_6()
                        .child(
                            Icon::new(AppIcon::Terminal)
                                .size(px(24.))
                                .text_color(rgb(COLOR_TEXT_SECONDARY)),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child("通过 SSH 连接到远程开发主机"),
                        )
                        .child(
                            settings_action_button("ssh-import-empty", cx)
                                .label("从 ~/.ssh/config 导入")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.import_ssh_config(cx);
                                })),
                        ),
                ),
            );
        } else {
            let mut connections = settings_card("来自这台 Mac 的 SSH 连接");
            for (index, profile) in self.coding_preferences.ssh_connections.iter().enumerate() {
                let test_id = profile.id.clone();
                let delete_id = profile.id.clone();
                let destination = if profile.user.is_empty() {
                    format!("{}:{}", profile.host, profile.port)
                } else {
                    format!("{}@{}:{}", profile.user, profile.host, profile.port)
                };
                let test_state = self.ssh_connection_tests.get(&profile.id);
                let (status, status_color) = match test_state {
                    Some(SshConnectionTestState::Testing) => ("正在测试".to_owned(), COLOR_WARNING),
                    Some(SshConnectionTestState::Reachable(_)) => {
                        ("连接正常".to_owned(), COLOR_SUCCESS)
                    }
                    Some(SshConnectionTestState::Failed(_)) => ("连接失败".to_owned(), COLOR_ERROR),
                    None => ("尚未测试".to_owned(), COLOR_TEXT_TERTIARY),
                };
                let detail = test_state.and_then(|state| match state {
                    SshConnectionTestState::Reachable(detail)
                    | SshConnectionTestState::Failed(detail) => Some(detail.clone()),
                    SshConnectionTestState::Testing => None,
                });
                if index > 0 {
                    connections = connections.child(coding_divider());
                }
                connections = connections.child(
                    div()
                        .v_flex()
                        .gap_3()
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .v_flex()
                                        .min_w(px(0.))
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(font_px(FONT_SIZE_SM))
                                                .font_medium()
                                                .child(profile.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .font_family(mono_font_family())
                                                .text_size(font_px(FONT_SIZE_XS))
                                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                .child(destination),
                                        ),
                                )
                                .child(compact_status_badge(status, status_color)),
                        )
                        .when_some(detail, |row, detail| {
                            row.child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .line_height(px(18.))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child(detail),
                            )
                        })
                        .child(
                            div()
                                .h_flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    settings_action_button(("ssh-test", index), cx)
                                        .label("测试连接")
                                        .loading(matches!(
                                            test_state,
                                            Some(SshConnectionTestState::Testing)
                                        ))
                                        .disabled(self.ssh_connection_task.is_some())
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.test_ssh_connection(test_id.clone(), cx);
                                        })),
                                )
                                .child(
                                    settings_danger_action_button(("ssh-delete", index), cx)
                                        .label(
                                            if self.pending_ssh_connection_delete.as_deref()
                                                == Some(profile.id.as_str())
                                            {
                                                "确认移除"
                                            } else {
                                                "移除"
                                            },
                                        )
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.request_ssh_connection_delete(
                                                delete_id.clone(),
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                );
            }
            page = page.child(connections);
        }

        page.child(
            settings_card("添加 SSH 连接")
                .child(input_field(
                    "名称",
                    "用于在 Corbit 中识别这台远程主机。",
                    settings_input(&self.ssh_connection_name),
                ))
                .child(input_field(
                    "主机",
                    "可以填写主机名、IP 地址或 ~/.ssh/config 中的 Host 别名。",
                    settings_input(&self.ssh_connection_host),
                ))
                .child(
                    div()
                        .h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            input_field(
                                "用户名",
                                "留空时使用系统 SSH 配置。",
                                settings_input(&self.ssh_connection_user),
                            )
                            .flex_1(),
                        )
                        .child(
                            input_field(
                                "端口",
                                "默认使用 22。",
                                settings_input(&self.ssh_connection_port),
                            )
                            .w(px(160.)),
                        ),
                )
                .child(input_field(
                    "身份文件",
                    "可选；留空时使用 SSH Agent 或系统配置，不保存密码。",
                    settings_input(&self.ssh_connection_identity_file),
                ))
                .child(
                    div()
                        .h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            settings_action_button("ssh-import-config", cx)
                                .label("导入 SSH 配置")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.import_ssh_config(cx);
                                })),
                        )
                        .child(
                            settings_primary_action_button("ssh-add-connection", cx)
                                .label("添加")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.add_ssh_connection(window, cx);
                                })),
                        ),
                ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_git_settings(&self, cx: &mut Context<Self>) -> Div {
        let git_status = self
            .git_version
            .clone()
            .unwrap_or_else(|| "未检测到 Git".into());
        let git_ready = self.git_version.is_some();
        settings_page_header(
            "Git",
            "配置 Corbit 在项目中使用 Git 和 Pull Request 的默认方式。",
        )
        .child(settings_card("本机 Git").child(coding_row(
            "Git 命令",
            "Corbit 的仓库状态和 Diff 由 Daemon 执行，本机设置用于连接与工作流检查。",
            compact_status_badge(
                git_status,
                if git_ready {
                    COLOR_SUCCESS
                } else {
                    COLOR_ERROR
                },
            ),
        )))
        .child(
            settings_card("分支与 Pull Request")
                .child(coding_row(
                    "分支前缀",
                    "创建新任务分支时使用的前缀。",
                    settings_input(&self.git_branch_prefix).w(px(250.)),
                ))
                .child(coding_divider())
                .child(coding_row(
                    "Pull Request 合并方法",
                    "选择默认合并方式。",
                    coding_option_group([
                        Button::new("git-merge-method-merge")
                            .ghost()
                            .small()
                            .selected(
                                self.coding_preferences.git.merge_method == GitMergeMethod::Merge,
                            )
                            .label("合并")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.update_git_preference(
                                    |git| git.merge_method = GitMergeMethod::Merge,
                                    cx,
                                );
                            })),
                        Button::new("git-merge-method-squash")
                            .ghost()
                            .small()
                            .selected(
                                self.coding_preferences.git.merge_method == GitMergeMethod::Squash,
                            )
                            .label("压缩合并")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.update_git_preference(
                                    |git| git.merge_method = GitMergeMethod::Squash,
                                    cx,
                                );
                            })),
                    ]),
                ))
                .child(coding_divider())
                .child(coding_row(
                    "始终强制推送",
                    "开启后仅允许使用相对安全的 --force-with-lease。",
                    settings_switch(
                        "git-force-with-lease",
                        self.coding_preferences.git.force_with_lease,
                    )
                    .on_click(cx.listener(|view, checked, _, cx| {
                        view.update_git_preference(|git| git.force_with_lease = *checked, cx);
                    })),
                ))
                .child(coding_divider())
                .child(coding_row(
                    "创建草稿 Pull Request",
                    "新建 PR 时默认使用草稿状态。",
                    settings_switch(
                        "git-draft-pull-request",
                        self.coding_preferences.git.draft_pull_requests,
                    )
                    .on_click(cx.listener(|view, checked, _, cx| {
                        view.update_git_preference(|git| git.draft_pull_requests = *checked, cx);
                    })),
                ))
                .child(coding_divider())
                .child(coding_row(
                    "审查结果呈现方式",
                    "选择在当前聊天中呈现，或启动单独审查任务。",
                    coding_option_group([
                        Button::new("git-review-inline")
                            .ghost()
                            .small()
                            .selected(
                                self.coding_preferences.git.review_presentation
                                    == GitReviewPresentation::Inline,
                            )
                            .label("内联")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.update_git_preference(
                                    |git| {
                                        git.review_presentation = GitReviewPresentation::Inline;
                                    },
                                    cx,
                                );
                            })),
                        Button::new("git-review-separate")
                            .ghost()
                            .small()
                            .selected(
                                self.coding_preferences.git.review_presentation
                                    == GitReviewPresentation::Separate,
                            )
                            .label("单独")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.update_git_preference(
                                    |git| {
                                        git.review_presentation = GitReviewPresentation::Separate;
                                    },
                                    cx,
                                );
                            })),
                    ]),
                )),
        )
        .child(
            settings_card("监控并修复 Pull Request")
                .child(coding_row(
                    "准备就绪时自动合并",
                    "仅保存工作流偏好；实际合并仍需要仓库权限和明确授权。",
                    settings_switch(
                        "git-auto-merge",
                        self.coding_preferences.git.auto_merge_when_ready,
                    )
                    .on_click(cx.listener(|view, checked, _, cx| {
                        view.update_git_preference(|git| git.auto_merge_when_ready = *checked, cx);
                    })),
                ))
                .child(coding_divider())
                .child(input_field(
                    "监控说明",
                    "补充检查、测试和合并时需要遵循的项目约束。",
                    settings_input(&self.git_monitoring_instructions),
                )),
        )
        .child(
            settings_card("提交说明")
                .child(input_field(
                    "提交信息指引",
                    "将添加到生成提交信息时使用的项目级说明中。",
                    settings_input(&self.git_commit_instructions),
                ))
                .child(
                    div().h_flex().justify_end().child(
                        settings_primary_action_button("git-save-settings", cx)
                            .label("保存 Git 设置")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.save_git_text_settings(cx);
                            })),
                    ),
                ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_hook_settings(&self, cx: &mut Context<Self>) -> Div {
        let sources = self.hook_sources();
        let page = settings_page_header(
            "钩子",
            "通过用户配置、项目配置和已启用的插件发现任务生命周期钩子。",
        );
        if sources.is_empty() {
            return page.child(
                settings_card("未找到钩子")
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("已配置的钩子将显示在此处。"),
                    )
                    .child(
                        div().h_flex().justify_end().child(
                            settings_action_button("hooks-refresh-empty", cx)
                                .icon(Icon::new(AppIcon::Refresh))
                                .label("刷新")
                                .loading(self.plugin_operation_in_flight)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.load_plugins(cx);
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        }

        let mut card = settings_card(format!("已发现的钩子来源 · {}", sources.len()));
        for (index, source) in sources.into_iter().enumerate() {
            if index > 0 {
                card = card.child(coding_divider());
            }
            let (status, color, detail) = match &source.state {
                HookSourceState::Ready(count) => (
                    format!("{count} 个钩子"),
                    COLOR_SUCCESS,
                    "配置可读取".to_owned(),
                ),
                HookSourceState::Invalid(error) => ("配置错误".into(), COLOR_ERROR, error.clone()),
                HookSourceState::Declared => (
                    "已声明".into(),
                    COLOR_WARNING,
                    "插件声明了 Hooks，但当前文件未能直接读取".into(),
                ),
            };
            card = card.child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .v_flex()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .font_medium()
                                            .child(source.title),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child(source.source),
                                    ),
                            )
                            .child(compact_status_badge(status, color)),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(mono_font_family())
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(source.path.display().to_string()),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(detail),
                    ),
            );
        }
        page.child(card).child(
            div().h_flex().justify_end().child(
                settings_action_button("hooks-refresh", cx)
                    .icon(Icon::new(AppIcon::Refresh))
                    .label("刷新")
                    .loading(self.plugin_operation_in_flight)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.load_plugins(cx);
                        cx.notify();
                    })),
            ),
        )
    }
}

pub(super) fn detect_git_version() -> Option<String> {
    let output = Command::new("git").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    let version = version.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_preferences_accept_partial_json() {
        let preferences: CodingPreferences =
            serde_json::from_str(r#"{"git":{"branchPrefix":"feature/"}}"#)
                .expect("partial coding preferences should decode");
        assert_eq!(preferences.git.branch_prefix, "feature/");
        assert!(preferences.git.draft_pull_requests);
        assert!(preferences.ssh_connections.is_empty());
    }

    #[test]
    fn rejects_unsafe_branch_prefixes() {
        assert!(validate_branch_prefix("corbit/").is_ok());
        assert!(validate_branch_prefix("bad prefix/").is_err());
        assert!(validate_branch_prefix("../escape").is_err());
    }

    #[test]
    fn hook_count_accepts_supported_shapes() {
        assert_eq!(hook_count(&serde_json::json!({ "hooks": [{}, {}] })), 2);
        assert_eq!(hook_count(&serde_json::json!([{}, {}, {}])), 3);
    }
}
