use super::*;
use chrono::{DateTime, Local};

fn format_device_pairing_time(value: &str) -> String {
    DateTime::parse_from_rfc3339(value).map_or_else(
        |_| value.to_owned(),
        |timestamp| {
            timestamp
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

fn device_pairing_field(label: &'static str, description: &'static str, input: Input) -> Div {
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
                .line_height(px(18.))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child(description),
        )
        .child(input)
}

fn device_pairing_step(
    number: &'static str,
    title: &'static str,
    description: &'static str,
) -> Div {
    div()
        .h_flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .flex_none()
                .size(px(24.))
                .rounded_full()
                .border_1()
                .border_color(rgb(COLOR_BORDER_HEAVY))
                .bg(rgb(COLOR_SURFACE_SECONDARY))
                .flex()
                .items_center()
                .justify_center()
                .text_size(font_px(FONT_SIZE_XS))
                .font_semibold()
                .child(number),
        )
        .child(
            div()
                .v_flex()
                .min_w(px(0.))
                .gap_1()
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .font_medium()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .line_height(px(18.))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(description),
                ),
        )
}

fn device_pairing_tutorial_card() -> SettingsCard {
    settings_card("使用教程")
        .child(device_pairing_step(
            "1",
            "准备可访问的连接地址",
            "同一局域网可填写这台 Mac 的局域网 IP；跨网络连接请使用 HTTPS 或 Relay 地址，不能使用 127.0.0.1。",
        ))
        .child(settings_row_divider())
        .child(device_pairing_step(
            "2",
            "生成并打开一次性链接",
            "复制配对链接后，在 Corbit 手机端或远程客户端中打开。链接只能使用一次，并且会在指定时间后失效。",
        ))
        .child(settings_row_divider())
        .child(device_pairing_step(
            "3",
            "完成配对并刷新列表",
            "远程客户端确认连接后，返回此页面刷新设备列表。每台设备都会获得独立凭证，可单独撤销而不影响其它设备。",
        ))
        .child(
            div()
                .h_flex()
                .items_start()
                .gap_2()
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(COLOR_BORDER_LIGHT))
                .bg(rgb(COLOR_SURFACE_SECONDARY))
                .p_3()
                .child(
                    Icon::new(AppIcon::Info)
                        .size(px(14.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY)),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .line_height(px(18.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(
                            "安全提示：配对链接等同于临时访问邀请，请只发送给你信任的设备；设备遗失或停用后应立即撤销其凭证。",
                        ),
                ),
        )
}

fn paired_device_status_badge() -> Div {
    div()
        .h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .rounded_full()
        .border_1()
        .border_color(rgb(COLOR_BORDER_LIGHT))
        .bg(rgb(COLOR_SURFACE_UNDER))
        .px_2()
        .py_1()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(COLOR_TEXT_SECONDARY))
        .child(div().size(px(5.)).rounded_full().bg(rgb(COLOR_SUCCESS)))
        .child("已授权")
}

impl ConnectionView {
    pub(super) fn resource_section_label(&self) -> &'static str {
        match self.resource_section {
            ResourceSection::General => "常规",
            ResourceSection::Appearance => "外观",
            ResourceSection::Notifications => "通知",
            ResourceSection::Configuration => "配置",
            ResourceSection::Providers => "提供商",
            ResourceSection::ComputerControl => "电脑操控",
            ResourceSection::AppSnapshot => "应用快照",
            ResourceSection::Plugins => "插件",
            ResourceSection::Browser => "浏览器连接",
            ResourceSection::Shortcuts => "快捷键",
            ResourceSection::Projects | ResourceSection::Workspaces | ResourceSection::Agents => {
                "项目"
            }
            ResourceSection::SshConnections => "连接",
            ResourceSection::Git => "Git",
            ResourceSection::Hooks => "钩子",
            ResourceSection::Daemon => "本地服务",
            ResourceSection::Devices => "远程设备",
            ResourceSection::About => "关于软件",
            ResourceSection::ThirdPartyLicenses => "开源许可",
        }
    }

    pub(super) fn apply_appearance_preferences(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        configure_codex_theme(self.appearance, Some(window), cx);
        self.appearance_error = self.appearance.save().err().map(|error| error.to_string());
        cx.notify();
    }

    fn set_color_scheme(
        &mut self,
        color_scheme: ColorScheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.color_scheme = color_scheme;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_interface_text_size(
        &mut self,
        interface_text_size: InterfaceTextSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.interface_text_size = interface_text_size;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_contrast(
        &mut self,
        contrast: ContrastLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.contrast = contrast;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_translucent_sidebar(
        &mut self,
        translucent_sidebar: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.translucent_sidebar = translucent_sidebar;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_interface_font(
        &mut self,
        interface_font: InterfaceFont,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.interface_font = interface_font;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_code_text_size(
        &mut self,
        code_text_size: CodeTextSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.code_text_size = code_text_size;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_code_font(&mut self, code_font: CodeFont, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance.code_font = code_font;
        self.apply_appearance_preferences(window, cx);
    }

    fn set_content_width(
        &mut self,
        content_width: ContentWidth,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance.content_width = content_width;
        self.apply_appearance_preferences(window, cx);
    }

    fn sync_appearance_color_pickers(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pickers = [
            (&self.appearance_accent_color, self.appearance.accent_color),
            (
                &self.appearance_light_background,
                self.appearance.light_background,
            ),
            (
                &self.appearance_light_foreground,
                self.appearance.light_foreground,
            ),
            (
                &self.appearance_dark_background,
                self.appearance.dark_background,
            ),
            (
                &self.appearance_dark_foreground,
                self.appearance.dark_foreground,
            ),
        ];
        for (picker, color) in pickers {
            picker.update(cx, |picker, cx| {
                picker.set_value(gpui_rgb(color), window, cx);
            });
        }
    }

    fn copy_appearance_theme(&mut self, cx: &mut Context<Self>) {
        match self.appearance.share_code() {
            Ok(theme) => {
                cx.write_to_clipboard(ClipboardItem::new_string(theme));
                self.appearance_error = None;
                self.show_success("外观配置已复制到剪贴板", cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.appearance_error = Some(message.clone());
                self.show_error(message, cx);
            }
        }
    }

    fn import_appearance_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = Self::input_value(&self.appearance_theme_code, cx);
        if theme.is_empty() {
            let message = "请先粘贴要导入的外观配置".to_owned();
            self.appearance_error = Some(message.clone());
            self.show_error(message, cx);
            return;
        }
        match AppearancePreferences::from_share_code(&theme) {
            Ok(appearance) => {
                self.appearance = appearance;
                self.sync_appearance_color_pickers(window, cx);
                self.apply_appearance_preferences(window, cx);
                self.show_success("外观配置已导入", cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.appearance_error = Some(message.clone());
                self.show_error(message, cx);
            }
        }
    }

    fn reset_appearance_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance = AppearancePreferences::default();
        self.sync_appearance_color_pickers(window, cx);
        self.apply_appearance_preferences(window, cx);
        self.show_success("外观设置已恢复默认", cx);
    }

    pub(super) fn load_devices(&mut self, cx: &mut Context<Self>) {
        if self.device_operation_in_flight
            || !matches!(self.state, corbit_client::ConnectionState::Online)
            || !self.feature_enabled("devicePairing")
        {
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            return;
        };
        self.device_operation_in_flight = true;
        self.detail = "正在读取已配对设备…".into();
        self.device_task = Some(cx.spawn(async move |view, cx| {
            let result = client.devices().await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.device_operation_in_flight = false;
                match result {
                    Ok(devices) => {
                        view.devices = devices;
                        view.pending_revoke_device_id = None;
                        view.detail = format!("已同步 {} 台配对设备", view.devices.len());
                    }
                    Err(error) => view.show_error(format!("读取设备失败：{error}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn create_pairing_offer(&mut self, cx: &mut Context<Self>) {
        if self.device_operation_in_flight {
            self.show_warning("设备操作正在执行，请稍候", cx);
            return;
        }
        let endpoint = Self::input_value(&self.pairing_endpoint, cx);
        let host_name = Self::input_value(&self.pairing_host_name, cx);
        if endpoint.is_empty() || host_name.is_empty() {
            self.show_validation_error("请输入手机可访问的 Daemon 地址和主机名称", cx);
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
        self.device_operation_in_flight = true;
        self.pairing_offer = None;
        self.detail = "正在创建一次性配对链接…".into();
        self.device_task = Some(cx.spawn(async move |view, cx| {
            let result = client.create_pairing(endpoint, host_name).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.device_operation_in_flight = false;
                match result {
                    Ok(offer) => {
                        view.pairing_offer = Some(offer);
                        view.show_success("一次性配对链接已创建，请在过期前于手机端使用", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("创建配对链接失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn request_device_revoke(&mut self, device_id: &str, cx: &mut Context<Self>) {
        if self.pending_revoke_device_id.as_deref() != Some(device_id) {
            self.pending_revoke_device_id = Some(device_id.to_owned());
            self.show_warning("撤销后该设备会立即断开；请再次点击确认", cx);
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
        let revoked_id = device_id.to_owned();
        self.device_operation_in_flight = true;
        self.detail = "正在撤销设备凭证…".into();
        self.device_task = Some(cx.spawn(async move |view, cx| {
            let result = client.revoke_device(revoked_id.clone()).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.device_operation_in_flight = false;
                match result {
                    Ok(()) => {
                        view.devices.retain(|device| device.id != revoked_id);
                        view.pending_revoke_device_id = None;
                        view.show_success("设备已撤销", cx);
                    }
                    Err(error) => view.show_error(format!("撤销设备失败：{error}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn copy_pairing_uri(&mut self, uri: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(uri));
        self.show_success("配对链接已复制到剪贴板", cx);
    }

    fn save_connection_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let endpoint = Self::input_value(&self.connection_endpoint, cx);
        let pending_token = Self::input_value(&self.connection_token, cx);
        let credential = connection::resolve_credentials(&endpoint);
        self.credential_source = credential.source;
        self.system_credential_present = credential.system_credential_present;

        let token = if pending_token.is_empty() {
            credential.token
        } else {
            Some(pending_token.clone())
        };
        let Some(token) = token else {
            let message = credential.error.unwrap_or_else(|| {
                "未找到可用凭证，请输入 Daemon Token 或确认本机 Daemon 已启动".into()
            });
            self.connection_settings_error = Some(message.clone());
            self.show_error(message, cx);
            return;
        };
        let config = match corbit_client::ClientConfig::desktop(&endpoint, token) {
            Ok(config) => config,
            Err(error) => {
                let message = format!("连接配置无效：{error}");
                self.connection_settings_error = Some(message.clone());
                self.show_error(message, cx);
                return;
            }
        };
        let normalized_endpoint = config.endpoint.to_string();

        if !pending_token.is_empty() {
            if let Err(error) = connection::save_system_credential(&pending_token) {
                let message = error.to_string();
                self.connection_settings_error = Some(message.clone());
                self.show_error(message, cx);
                return;
            }
            self.connection_token
                .update(cx, |input, cx| input.set_value("", window, cx));
        }

        let preferences = ConnectionPreferences {
            endpoint: normalized_endpoint,
        };
        if let Err(error) = preferences.save() {
            let message = error.to_string();
            self.connection_settings_error = Some(message.clone());
            self.show_error(message, cx);
            return;
        }

        let previous_endpoint = self.daemon_endpoint.clone();
        let resolved_endpoint = preferences.resolved_endpoint();
        self.daemon_endpoint = resolved_endpoint.value;
        self.endpoint_environment_override = resolved_endpoint.environment_override;
        self.connection_preferences = preferences;
        self.connection_endpoint.update(cx, |input, cx| {
            input.set_value(self.connection_preferences.endpoint.clone(), window, cx);
        });
        if Self::input_value(&self.pairing_endpoint, cx) == previous_endpoint {
            self.pairing_endpoint.update(cx, |input, cx| {
                input.set_value(self.daemon_endpoint.clone(), window, cx);
            });
        }
        self.connection_settings_error = None;
        self.show_success("连接设置已保存，正在重新连接", cx);
        self.connect(cx);
    }

    fn detect_local_daemon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.endpoint_environment_override {
            let message =
                "CORBIT_DAEMON_URL 正在覆盖界面设置；请先移除该环境变量后再检测本机默认地址"
                    .to_owned();
            self.connection_settings_error = Some(message.clone());
            self.show_error(message, cx);
            return;
        }

        let preferences = ConnectionPreferences::default();
        if let Err(error) = preferences.save() {
            let message = error.to_string();
            self.connection_settings_error = Some(message.clone());
            self.show_error(message, cx);
            return;
        }

        let previous_endpoint = self.daemon_endpoint.clone();
        let resolved_endpoint = preferences.resolved_endpoint();
        self.daemon_endpoint = resolved_endpoint.value;
        self.endpoint_environment_override = resolved_endpoint.environment_override;
        self.connection_preferences = preferences;
        self.connection_endpoint.update(cx, |input, cx| {
            input.set_value(self.connection_preferences.endpoint.clone(), window, cx);
        });
        if Self::input_value(&self.pairing_endpoint, cx) == previous_endpoint {
            self.pairing_endpoint.update(cx, |input, cx| {
                input.set_value(self.daemon_endpoint.clone(), window, cx);
            });
        }
        self.connection_settings_error = None;
        self.show_info("正在检测本机 Daemon…", cx);
        self.connect(cx);
    }

    fn refresh_daemon_diagnostics(&mut self, cx: &mut Context<Self>) {
        if self.daemon_action_task.is_some() {
            return;
        }
        self.local_daemon_status = local_daemon::DaemonStatus::checking();
        let endpoint = self.daemon_endpoint.clone();
        let endpoint_for_request = endpoint.clone();
        self.daemon_action_task = Some(cx.spawn(async move |view, cx| {
            let result = local_daemon::diagnose(endpoint).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.daemon_action_task = None;
                if view.daemon_endpoint != endpoint_for_request {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(status) => {
                        view.local_daemon_status = status;
                        view.show_success("Daemon 诊断信息已刷新", cx);
                    }
                    Err(error) => {
                        view.local_daemon_status = local_daemon::DaemonStatus::failed(&error);
                        view.show_error(format!("Daemon 检测失败：{error:#}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn restart_local_daemon(&mut self, cx: &mut Context<Self>) {
        if self.daemon_action_task.is_some() || self.daemon_preflight_task.is_some() {
            return;
        }
        self.connection_generation = self.connection_generation.wrapping_add(1);
        self.daemon_preflight_task = None;
        self.event_task = None;
        self.runtime = None;
        self.clear_connection_bound_state();
        self.state = corbit_client::ConnectionState::Connecting;
        self.detail = "正在安全重启本机 Corbit Daemon".into();
        self.local_daemon_status = local_daemon::DaemonStatus::restarting();
        let endpoint = self.daemon_endpoint.clone();
        self.daemon_action_task = Some(cx.spawn(async move |view, cx| {
            let result = local_daemon::restart_owned(endpoint).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.daemon_action_task = None;
                match result {
                    Ok(status) => {
                        view.local_daemon_status = status;
                        view.show_success("本机 Daemon 已安全重启，正在重新连接", cx);
                        view.connect(cx);
                    }
                    Err(error) => {
                        view.state = corbit_client::ConnectionState::Offline;
                        view.local_daemon_status = local_daemon::DaemonStatus::failed(&error);
                        view.show_error(format!("无法安全重启本机 Daemon：{error:#}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn open_daemon_logs(&mut self, cx: &mut Context<Self>) {
        match local_daemon::open_log_directory() {
            Ok(()) => self.show_info("已打开 Daemon 日志目录", cx),
            Err(error) => self.show_error(format!("无法打开 Daemon 日志目录：{error:#}"), cx),
        }
    }

    fn copy_daemon_diagnostics(&mut self, cx: &mut Context<Self>) {
        let diagnostics = self.local_daemon_status.diagnostics(&self.daemon_endpoint);
        cx.write_to_clipboard(ClipboardItem::new_string(diagnostics));
        self.show_success("Daemon 诊断信息已复制到剪贴板", cx);
    }

    fn delete_saved_connection_credential(&mut self, cx: &mut Context<Self>) {
        match connection::delete_system_credential() {
            Ok(removed) => {
                let credential = connection::resolve_credentials(&self.daemon_endpoint);
                self.credential_source = credential.source;
                self.system_credential_present = credential.system_credential_present;
                self.connection_settings_error = credential.error;
                let message: String = if removed {
                    if self.credential_source == Some(CredentialSource::Environment) {
                        "已移除系统凭证；本次启动仍使用 CORBIT_AUTH_TOKEN".into()
                    } else if self.credential_source == Some(CredentialSource::LocalDaemon) {
                        "已移除手动 Token；后续将自动使用本机 Daemon 凭据".into()
                    } else {
                        "已从系统凭证存储中移除 Daemon Token".into()
                    }
                } else {
                    "系统凭证存储中没有 Corbit Token".into()
                };
                self.show_success(message, cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.connection_settings_error = Some(message.clone());
                self.show_error(message, cx);
            }
        }
    }

    fn persist_general_preferences(&mut self, cx: &mut Context<Self>) {
        match self.ui_preferences(cx).save() {
            Ok(()) => {
                self.ui_state_error = None;
                cx.notify();
            }
            Err(error) => {
                let message = error.to_string();
                self.ui_state_error = Some(message.clone());
                self.show_error(format!("常规设置保存失败：{message}"), cx);
            }
        }
    }

    fn set_startup_destination(&mut self, destination: StartupDestination, cx: &mut Context<Self>) {
        self.general_preferences.startup_destination = destination;
        self.persist_general_preferences(cx);
    }

    fn set_close_window_behavior(&mut self, behavior: CloseWindowBehavior, cx: &mut Context<Self>) {
        self.general_preferences.close_window_behavior = behavior;
        self.persist_general_preferences(cx);
    }

    fn set_save_prompt_drafts(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.save_prompt_drafts = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_follow_up_behavior(&mut self, behavior: FollowUpBehavior, cx: &mut Context<Self>) {
        self.general_preferences.follow_up_behavior = behavior;
        self.persist_general_preferences(cx);
    }

    fn set_prompt_submit_behavior(
        &mut self,
        behavior: PromptSubmitBehavior,
        cx: &mut Context<Self>,
    ) {
        self.general_preferences.prompt_submit_behavior = behavior;
        self.persist_general_preferences(cx);
    }

    fn set_auto_follow_output(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.auto_follow_output = enabled;
        if enabled {
            self.scroll_selected_timeline_to_latest();
        }
        self.persist_general_preferences(cx);
    }

    fn set_prevent_sleep_while_running(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.prevent_sleep_while_running = enabled;
        if let Err(error) = self.sync_sleep_prevention() {
            self.general_preferences.prevent_sleep_while_running = false;
            self.show_error(error.to_string(), cx);
            return;
        }
        self.persist_general_preferences(cx);
    }

    fn set_notify_permission_requests(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.notify_permission_requests = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_notify_task_completion(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.notify_task_completion = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_notify_task_failure(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.notify_task_failure = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_notification_sound(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.general_preferences.notification_sound = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_general_permission_mode(
        &mut self,
        mode: corbit_client::AgentPermissionMode,
        cx: &mut Context<Self>,
    ) {
        self.composer_permission_mode = mode;
        self.persist_general_preferences(cx);
    }

    fn choose_general_default_model(
        &mut self,
        provider_id: &str,
        model_id: &str,
        cx: &mut Context<Self>,
    ) {
        let provider = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog
                .providers
                .iter()
                .find(|provider| provider.provider_id == provider_id && provider.available)
                .cloned()
        });
        if provider.is_some_and(|provider| {
            self.composer_selections
                .choose_default_model(&provider, model_id)
        }) {
            self.persist_general_preferences(cx);
        }
    }

    fn choose_general_default_reasoning(
        &mut self,
        provider_id: &str,
        effort: corbit_client::AgentReasoningEffort,
        cx: &mut Context<Self>,
    ) {
        let provider = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog
                .providers
                .iter()
                .find(|provider| provider.provider_id == provider_id && provider.available)
                .cloned()
        });
        if provider.is_some_and(|provider| {
            self.composer_selections
                .choose_default_reasoning_effort(&provider, effort)
        }) {
            self.persist_general_preferences(cx);
        }
    }

    fn set_configuration_network_access(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.agent_configuration.network_access = enabled;
        self.persist_general_preferences(cx);
    }

    fn set_configuration_reasoning_summary(
        &mut self,
        summary: corbit_client::AgentReasoningSummary,
        cx: &mut Context<Self>,
    ) {
        self.agent_configuration.reasoning_summary = summary;
        self.persist_general_preferences(cx);
    }

    fn set_configuration_personality(
        &mut self,
        personality: Option<corbit_client::AgentPersonality>,
        cx: &mut Context<Self>,
    ) {
        self.agent_configuration.personality = personality;
        self.persist_general_preferences(cx);
    }

    fn reset_selected_project_configuration(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project_id.clone() else {
            return;
        };
        let removed_provider = self.project_providers.remove(&project_id).is_some();
        let removed_model = self.composer_selections.clear_project(&project_id);
        if removed_provider || removed_model {
            self.persist_general_preferences(cx);
            self.show_success("当前项目已恢复使用用户配置", cx);
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_configuration_settings(&self, cx: &mut Context<Self>) -> Div {
        let provider_choices = self.provider_options();
        let selected_provider_label = provider_label(&self.selected_provider).to_owned();
        let provider_view = cx.entity();
        let selected_provider_for_menu = self.selected_provider.clone();
        let provider_button = settings_select_button("configuration-default-provider", cx)
            .label(selected_provider_label)
            .disabled(provider_choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for (provider, label, _) in provider_choices.clone() {
                    let item_view = provider_view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(selected_provider_for_menu == provider)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_default_provider(provider, cx);
                                });
                            }),
                    );
                }
                menu
            });

        let provider_entry = self.provider_catalog.as_ref().and_then(|catalog| {
            catalog.providers.iter().find(|provider| {
                provider.provider_id == self.selected_provider && provider.available
            })
        });
        let selected_model = provider_entry
            .and_then(|provider| self.composer_selections.default_model(provider).cloned());
        let model_choices = provider_entry
            .map(|provider| provider.models.clone())
            .unwrap_or_default();
        let selected_model_id = selected_model.as_ref().map(|model| model.id.clone());
        let model_provider_id = self.selected_provider.clone();
        let model_view = cx.entity();
        let model_button = settings_select_button("configuration-default-model", cx)
            .label(selected_model.as_ref().map_or_else(
                || "Provider 默认".to_owned(),
                |model| model_display_name(&model.id, &model.display_name),
            ))
            .disabled(model_choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu).max_w(px(380.));
                for model in model_choices.clone() {
                    let item_view = model_view.clone();
                    let item_provider = model_provider_id.clone();
                    let item_model = model.id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(model_display_name(&model.id, &model.display_name))
                            .checked(selected_model_id.as_deref() == Some(model.id.as_str()))
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_general_default_model(
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

        let permission_mode = self.composer_permission_mode;
        let permission_view = cx.entity();
        let permission_button = settings_select_button("configuration-permission", cx)
            .label(configuration_permission_label(permission_mode))
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for mode in [
                    corbit_client::AgentPermissionMode::ReadOnly,
                    corbit_client::AgentPermissionMode::WorkspaceWrite,
                    corbit_client::AgentPermissionMode::FullAccess,
                ] {
                    let item_view = permission_view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(configuration_permission_label(mode))
                            .checked(permission_mode == mode)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.set_general_permission_mode(mode, cx);
                                });
                            }),
                    );
                }
                menu
            });

        let selected_reasoning = provider_entry
            .and_then(|provider| self.composer_selections.default_reasoning_effort(provider));
        let reasoning_choices = selected_model
            .as_ref()
            .map(|model| model.supported_reasoning_efforts.clone())
            .unwrap_or_default();
        let reasoning_provider_id = self.selected_provider.clone();
        let reasoning_view = cx.entity();
        let reasoning_button = settings_select_button("configuration-default-reasoning", cx)
            .label(selected_reasoning.map_or("默认", reasoning_effort_short_label))
            .disabled(reasoning_choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for choice in reasoning_choices.clone() {
                    let effort = choice.reasoning_effort;
                    let item_view = reasoning_view.clone();
                    let item_provider = reasoning_provider_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(reasoning_effort_short_label(effort))
                            .checked(selected_reasoning == Some(effort))
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_general_default_reasoning(
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

        let reasoning_summary = self.agent_configuration.reasoning_summary;
        let summary_view = cx.entity();
        let summary_button = settings_select_button("configuration-reasoning-summary", cx)
            .label(reasoning_summary_label(reasoning_summary))
            .disabled(self.selected_provider != "codex")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for summary in [
                    corbit_client::AgentReasoningSummary::Auto,
                    corbit_client::AgentReasoningSummary::Concise,
                    corbit_client::AgentReasoningSummary::Detailed,
                    corbit_client::AgentReasoningSummary::None,
                ] {
                    let item_view = summary_view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(reasoning_summary_label(summary))
                            .checked(reasoning_summary == summary)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.set_configuration_reasoning_summary(summary, cx);
                                });
                            }),
                    );
                }
                menu
            });

        let personality = self.agent_configuration.personality;
        let personality_view = cx.entity();
        let personality_button = settings_select_button("configuration-personality", cx)
            .label(personality_label(personality))
            .disabled(self.selected_provider != "codex")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for choice in [
                    None,
                    Some(corbit_client::AgentPersonality::Friendly),
                    Some(corbit_client::AgentPersonality::Pragmatic),
                    Some(corbit_client::AgentPersonality::None),
                ] {
                    let item_view = personality_view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(personality_label(choice))
                            .checked(personality == choice)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.set_configuration_personality(choice, cx);
                                });
                            }),
                    );
                }
                menu
            });

        let selected_project = self.selected_project_id.as_deref().and_then(|project_id| {
            self.snapshot
                .as_ref()?
                .projects
                .iter()
                .find(|project| project.id == project_id)
        });
        let project_has_overrides = self
            .selected_project_id
            .as_deref()
            .is_some_and(|project_id| {
                self.project_providers.contains_key(project_id)
                    || self.composer_selections.project_has_overrides(project_id)
            });
        let project_name = selected_project
            .map_or_else(|| "尚未选择项目".to_owned(), |project| project.name.clone());
        let codex_selected = self.selected_provider == "codex";
        let full_access = permission_mode == corbit_client::AgentPermissionMode::FullAccess;
        let network_description = if !codex_selected {
            "当前 Provider 未开放逐轮网络控制，此设置仅应用于 Codex。"
        } else if full_access {
            "完全访问模式已包含网络访问；切换回沙盒后会恢复此处的选择。"
        } else {
            "允许沙盒内运行的命令访问网络，网页搜索工具不受此开关控制。"
        };

        settings_page_header("配置", "配置新任务的权限、网络访问和智能体回答方式。")
            .child(
                settings_section(
                    "智能体默认设置",
                    "这些用户配置会用于新任务，并可在对话输入框中调整。",
                )
                .child(
                    appearance_row_group()
                        .child(appearance_setting_row(
                            "提供商",
                            "作为新项目和未单独选择 Provider 的任务默认值。",
                            provider_button,
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "默认模型",
                            "项目没有单独选择模型时使用此模型。",
                            model_button,
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "批准与沙盒",
                            "选择请求批准、自动审查低风险操作或完全访问。",
                            permission_button,
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "允许网络访问",
                            network_description,
                            settings_switch(
                                "configuration-network-access",
                                full_access || self.agent_configuration.network_access,
                            )
                            .disabled(!codex_selected || full_access)
                            .on_click(cx.listener(
                                |view, checked, _, cx| {
                                    view.set_configuration_network_access(*checked, cx);
                                },
                            )),
                        )),
                ),
            )
            .child(
                settings_section("模型功能", "根据当前模型能力设置推理深度和回答呈现方式。").child(
                    appearance_row_group()
                        .child(appearance_setting_row(
                            "默认推理等级",
                            "可选等级由当前模型能力决定。",
                            reasoning_button,
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "推理摘要",
                            "控制 Codex 对推理过程进行总结的详细程度。",
                            summary_button,
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "回答风格",
                            "选择模型默认、友好自然、简洁务实或不使用预设风格。",
                            personality_button,
                        )),
                ),
            )
            .child(
                settings_section("项目配置", "项目可以覆盖用户级 Provider、模型和推理等级。")
                    .child(
                        appearance_row_group().child(appearance_setting_row(
                            "当前项目",
                            "新建任务页当前选择的项目。",
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .max_w(px(220.))
                                        .truncate()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(project_name),
                                )
                                .child(
                                    settings_action_button("configuration-reset-project", cx)
                                        .label(if project_has_overrides {
                                            "恢复用户配置"
                                        } else {
                                            "正在继承"
                                        })
                                        .disabled(!project_has_overrides)
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.reset_selected_project_configuration(cx);
                                        })),
                                ),
                        )),
                    ),
            )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_general_settings(&self, cx: &mut Context<Self>) -> Div {
        settings_page_header("常规", "配置应用启动、后台运行与对话输入。")
            .child(
                settings_section("启动与后台", "控制 Corbit 启动、关闭窗口和草稿恢复行为。").child(
                    appearance_row_group()
                        .child(appearance_setting_row(
                            "启动后打开",
                            "选择每次启动 Corbit 时首先进入的页面。",
                            appearance_option_group([
                                Button::new("general-startup-restore")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.startup_destination
                                            == StartupDestination::RestoreLast,
                                    )
                                    .label("上次页面")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_startup_destination(
                                            StartupDestination::RestoreLast,
                                            cx,
                                        );
                                    })),
                                Button::new("general-startup-new-task")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.startup_destination
                                            == StartupDestination::NewTask,
                                    )
                                    .label("新建任务")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_startup_destination(
                                            StartupDestination::NewTask,
                                            cx,
                                        );
                                    })),
                                Button::new("general-startup-tasks")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.startup_destination
                                            == StartupDestination::Tasks,
                                    )
                                    .label("任务列表")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_startup_destination(StartupDestination::Tasks, cx);
                                    })),
                            ]),
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "关闭窗口后",
                            "保留在菜单栏可继续接收任务状态并快速重新打开。",
                            appearance_option_group([
                                Button::new("general-close-menu-bar")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.close_window_behavior
                                            == CloseWindowBehavior::KeepInMenuBar,
                                    )
                                    .label("保留在菜单栏")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_close_window_behavior(
                                            CloseWindowBehavior::KeepInMenuBar,
                                            cx,
                                        );
                                    })),
                                Button::new("general-close-quit")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.close_window_behavior
                                            == CloseWindowBehavior::Quit,
                                    )
                                    .label("退出应用")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_close_window_behavior(
                                            CloseWindowBehavior::Quit,
                                            cx,
                                        );
                                    })),
                            ]),
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "恢复未发送内容",
                            "在当前设备保存新任务与对话输入框中的草稿。",
                            settings_switch(
                                "general-save-drafts",
                                self.general_preferences.save_prompt_drafts,
                            )
                            .on_click(cx.listener(
                                |view, checked, _, cx| {
                                    view.set_save_prompt_drafts(*checked, cx);
                                },
                            )),
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "任务运行时防止系统休眠",
                            if sleep_prevention::SleepPreventer::supported() {
                                "仅在存在运行中任务时保持系统唤醒，任务结束后自动恢复。"
                            } else {
                                "当前平台暂不支持此功能。"
                            },
                            settings_switch(
                                "general-prevent-sleep",
                                self.general_preferences.prevent_sleep_while_running,
                            )
                            .disabled(!sleep_prevention::SleepPreventer::supported())
                            .on_click(cx.listener(
                                |view, checked, _, cx| {
                                    view.set_prevent_sleep_while_running(*checked, cx);
                                },
                            )),
                        )),
                ),
            )
            .child(
                settings_section("对话与输入", "控制任务运行期间的后续消息和键盘发送方式。").child(
                    appearance_row_group()
                        .child(appearance_setting_row(
                            "任务运行时发送消息",
                            "不能立即引导的 Provider 或带附件消息会自动安全排队。",
                            appearance_option_group([
                                Button::new("general-follow-up-steer")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.follow_up_behavior
                                            == FollowUpBehavior::SteerCurrent,
                                    )
                                    .label("立即调整")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_follow_up_behavior(
                                            FollowUpBehavior::SteerCurrent,
                                            cx,
                                        );
                                    })),
                                Button::new("general-follow-up-queue")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.follow_up_behavior
                                            == FollowUpBehavior::QueueNext,
                                    )
                                    .label("排队到下一轮")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_follow_up_behavior(
                                            FollowUpBehavior::QueueNext,
                                            cx,
                                        );
                                    })),
                            ]),
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "发送快捷键",
                            "另一种组合始终用于插入换行。",
                            appearance_option_group([
                                Button::new("general-submit-enter")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.prompt_submit_behavior
                                            == PromptSubmitBehavior::Enter,
                                    )
                                    .label("Enter")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_prompt_submit_behavior(
                                            PromptSubmitBehavior::Enter,
                                            cx,
                                        );
                                    })),
                                Button::new("general-submit-command-enter")
                                    .ghost()
                                    .small()
                                    .selected(
                                        self.general_preferences.prompt_submit_behavior
                                            == PromptSubmitBehavior::CommandEnter,
                                    )
                                    .label("⌘ Enter")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_prompt_submit_behavior(
                                            PromptSubmitBehavior::CommandEnter,
                                            cx,
                                        );
                                    })),
                            ]),
                        ))
                        .child(settings_row_divider())
                        .child(appearance_setting_row(
                            "自动跟随最新输出",
                            "任务运行时保持对话视图位于最新内容。",
                            settings_switch(
                                "general-auto-follow",
                                self.general_preferences.auto_follow_output,
                            )
                            .on_click(cx.listener(
                                |view, checked, _, cx| {
                                    view.set_auto_follow_output(*checked, cx);
                                },
                            )),
                        )),
                ),
            )
    }

    pub(super) fn render_notification_settings(&self, cx: &mut Context<Self>) -> Div {
        let notifications_enabled = self.general_preferences.notify_permission_requests
            || self.general_preferences.notify_task_completion
            || self.general_preferences.notify_task_failure;

        settings_page_header("通知", "选择任务状态变化时显示的桌面提醒与提示音。").child(
            settings_section("桌面通知", "控制哪些任务事件可以发送系统通知。").child(
                appearance_row_group()
                    .child(appearance_setting_row(
                        "等待授权时通知",
                        "权限请求到达时显示桌面通知。",
                        settings_switch(
                            "notifications-permission",
                            self.general_preferences.notify_permission_requests,
                        )
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_notify_permission_requests(*checked, cx);
                        })),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "任务完成时通知",
                        "任务正常完成时显示桌面通知。",
                        settings_switch(
                            "notifications-completion",
                            self.general_preferences.notify_task_completion,
                        )
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_notify_task_completion(*checked, cx);
                        })),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "任务失败时通知",
                        "任务失败或被中断时显示桌面通知。",
                        settings_switch(
                            "notifications-failure",
                            self.general_preferences.notify_task_failure,
                        )
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_notify_task_failure(*checked, cx);
                        })),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "通知声音",
                        "桌面通知出现时播放系统默认提示音。",
                        settings_switch(
                            "notifications-sound",
                            self.general_preferences.notification_sound,
                        )
                        .disabled(!notifications_enabled)
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_notification_sound(*checked, cx);
                        })),
                    )),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_daemon_settings(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let (version, protocol, server_id) = self.server_info.as_ref().map_or_else(
            || ("—".to_owned(), "—".to_owned(), "尚未同步".to_owned()),
            |info| {
                (
                    info.version.clone(),
                    info.protocol_version.to_string(),
                    info.server_id.clone(),
                )
            },
        );
        let enabled_features = self.server_info.as_ref().map_or(0, |info| {
            info.features.values().filter(|enabled| **enabled).count()
        });
        let daemon_synchronized = self.server_info.is_some();
        let provider_catalog_synchronized = self.provider_catalog.is_some();
        let provider_options = self.provider_options();
        let provider_summary = if self.provider_catalog_error.is_some() {
            "目录读取失败".to_owned()
        } else if provider_catalog_synchronized {
            provider_options
                .iter()
                .map(|(_, label, _)| *label)
                .collect::<Vec<_>>()
                .join("、")
        } else if daemon_synchronized {
            "正在读取本机 Provider".to_owned()
        } else {
            "尚未同步".to_owned()
        };
        let providers_available = provider_catalog_synchronized && !provider_options.is_empty();
        let is_connecting = matches!(
            self.state,
            corbit_client::ConnectionState::Connecting
                | corbit_client::ConnectionState::Authenticating
                | corbit_client::ConnectionState::Reconnecting { .. }
        );
        let status_color = if is_online {
            rgb(COLOR_SUCCESS)
        } else {
            rgb(COLOR_TEXT_TERTIARY)
        };
        let credential_configured = self.credential_source.is_some();
        let credential_label = self
            .credential_source
            .map_or("未配置", CredentialSource::label);
        let credential_color = if credential_configured {
            rgb(COLOR_SUCCESS)
        } else {
            rgb(COLOR_WARNING)
        };
        let system_store_supported = connection::system_store_supported();
        let credential_help = if system_store_supported {
            match self.credential_source {
                Some(CredentialSource::Environment) => {
                    "环境变量在本次启动期间优先；新 Token 仍会安全保存到 macOS 钥匙串。"
                }
                Some(CredentialSource::LocalDaemon) => {
                    "已安全读取当前用户的本机 Daemon 凭据；如 Daemon 使用自定义 Token，可在此手动保存。"
                }
                Some(CredentialSource::SystemStore) => {
                    "当前使用手动保存的 macOS 钥匙串 Token；输入框留空会继续使用。"
                }
                None => {
                    "本机默认地址会自动读取 Daemon 凭据；自定义或远程 Daemon 可手动保存 Token。"
                }
            }
        } else {
            "当前平台暂不保存 Token，请通过 CORBIT_AUTH_TOKEN 提供凭证。"
        };
        let daemon_status = &self.local_daemon_status;
        let daemon_status_color = match daemon_status.phase {
            local_daemon::DaemonPhase::Ready => rgb(COLOR_SUCCESS),
            local_daemon::DaemonPhase::Blocked | local_daemon::DaemonPhase::Failed => {
                rgb(COLOR_WARNING)
            }
            _ => rgb(COLOR_TEXT_TERTIARY),
        };
        let daemon_action_busy =
            self.daemon_action_task.is_some() || self.daemon_preflight_task.is_some();
        let daemon_can_restart = !daemon_action_busy
            && daemon_status.phase != local_daemon::DaemonPhase::Unmanaged
            && (daemon_status.phase == local_daemon::DaemonPhase::Offline
                || daemon_status.desktop_owned);

        settings_page_header("本地服务", "连接配置、本机运行管理和当前服务能力。")
            .when_some(self.connection_settings_error.clone(), |page, error| {
                page.child(
                    div()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(COLOR_ERROR))
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .px_4()
                        .py_3()
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_ERROR))
                        .child(format!("连接设置未能完成：{error}")),
                )
            })
            .child(
                settings_card("Daemon 连接")
                    .child(setting_value_row(
                        "状态",
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().size(px(6.)).rounded_full().bg(status_color))
                            .child(if is_online { "已连接" } else { "未连接" }),
                    ))
                    .child(setting_text_row(
                        if self.endpoint_environment_override {
                            "当前地址（环境变量）"
                        } else {
                            "当前地址"
                        },
                        self.daemon_endpoint.clone(),
                    ))
                    .child(setting_value_row(
                        "凭证",
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().size(px(6.)).rounded_full().bg(credential_color))
                            .child(credential_label),
                    ))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .line_height(px(18.))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(
                                "Corbit 启动时会自动检测 127.0.0.1:6768；自定义地址与 Token 仍可在下方手动覆盖。",
                            ),
                    )
                    .child(settings_row_divider())
                    .child(setting_value_row(
                        "Daemon 地址",
                        div()
                            .w(px(330.))
                            .max_w_full()
                            .child(settings_input(&self.connection_endpoint)),
                    ))
                    .child(setting_value_row(
                        "访问 Token",
                        div().w(px(330.)).max_w_full().child(
                            settings_input(&self.connection_token)
                                .mask_toggle()
                                .disabled(!system_store_supported),
                        ),
                    ))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .line_height(px(18.))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(credential_help),
                    )
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .gap_2()
                            .pt_1()
                            .child(
                                settings_primary_action_button("settings-save-connection", cx)
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("保存并重新连接")
                                    .loading(is_connecting)
                                    .disabled(is_connecting)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.save_connection_settings(window, cx);
                                    })),
                            )
                            .child(
                                settings_action_button("settings-detect-local-daemon", cx)
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("检测本机 Daemon")
                                    .disabled(is_connecting || self.endpoint_environment_override)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.detect_local_daemon(window, cx);
                                    })),
                            )
                            .when(
                                self.system_credential_present && system_store_supported,
                                |actions| {
                                    actions.child(
                                        settings_danger_action_button(
                                            "settings-delete-connection-credential",
                                            cx,
                                        )
                                            .label("移除已存 Token")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.delete_saved_connection_credential(cx);
                                            })),
                                    )
                                },
                            )
                            .child(
                                settings_action_button("settings-reconnect", cx)
                                    .label("按当前设置重连")
                                    .loading(is_connecting)
                                    .disabled(is_connecting)
                                    .on_click(cx.listener(|view, _, _, cx| view.connect(cx))),
                            ),
                    ),
            )
            .child(
                settings_card("本机 Daemon 管理")
                    .child(setting_value_row(
                        "运行状态",
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().size(px(6.)).rounded_full().bg(daemon_status_color))
                            .child(daemon_status.phase.label()),
                    ))
                    .child(setting_text_row(
                        "期望版本",
                        daemon_status.expected_version.clone(),
                    ))
                    .child(setting_text_row(
                        "运行版本",
                        daemon_status.version.clone().unwrap_or_else(|| "—".into()),
                    ))
                    .child(setting_text_row(
                        "进程所有权",
                        if daemon_status.desktop_owned {
                            "Corbit Desktop"
                        } else {
                            "外部或无"
                        },
                    ))
                    .child(setting_text_row(
                        "托管方式",
                        daemon_status
                            .launch_mode
                            .map_or_else(|| "外部进程".to_owned(), |mode| mode.label().to_owned()),
                    ))
                    .child(setting_text_row(
                        "进程 PID",
                        daemon_status
                            .pid
                            .map_or_else(|| "—".into(), |pid| pid.to_string()),
                    ))
                    .child(setting_text_row(
                        "Node.js",
                        daemon_status.node.as_ref().map_or_else(
                            || "—".into(),
                            |path| path.display().to_string(),
                        ),
                    ))
                    .child(setting_text_row(
                        "私有运行包",
                        daemon_status.runtime_path.as_ref().map_or_else(
                            || "—".into(),
                            |path| path.display().to_string(),
                        ),
                    ))
                    .child(setting_text_row(
                        "日志文件",
                        daemon_status.log_path.as_ref().map_or_else(
                            || "—".into(),
                            |path| path.display().to_string(),
                        ),
                    ))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .line_height(px(18.))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(daemon_status.detail.clone()),
                    )
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .gap_2()
                            .pt_1()
                            .child(
                                settings_action_button(
                                    "settings-refresh-daemon-diagnostics",
                                    cx,
                                )
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("重新检测")
                                    .loading(daemon_status.phase.is_busy())
                                    .disabled(daemon_action_busy)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.refresh_daemon_diagnostics(cx);
                                    })),
                            )
                            .child(
                                settings_primary_action_button(
                                    "settings-restart-local-daemon",
                                    cx,
                                )
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("安全重启")
                                    .loading(daemon_status.phase == local_daemon::DaemonPhase::Restarting)
                                    .disabled(!daemon_can_restart)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.restart_local_daemon(cx);
                                    })),
                            )
                            .child(
                                settings_action_button("settings-open-daemon-logs", cx)
                                    .icon(Icon::new(AppIcon::FolderOpen))
                                    .label("打开日志")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.open_daemon_logs(cx);
                                    })),
                            )
                            .child(
                                settings_quiet_action_button(
                                    "settings-copy-daemon-diagnostics",
                                )
                                    .icon(Icon::new(AppIcon::Copy))
                                    .label("复制诊断")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.copy_daemon_diagnostics(cx);
                                    })),
                            ),
                    ),
            )
            .child(
                settings_card("Daemon 信息")
                    .child(setting_text_row("版本", version))
                    .child(setting_text_row("协议", protocol))
                    .child(setting_text_row("Server ID", server_id))
                    .child(setting_text_row(
                        "已启用能力",
                        format!("{enabled_features} 项"),
                    )),
            )
            .child(
                settings_card("运行能力")
                    .child(capability_row(
                        "Agent 会话",
                        provider_summary,
                        providers_available,
                    ))
                    .child(capability_row(
                        "设备配对",
                        capability_status(
                            daemon_synchronized,
                            self.feature_enabled("devicePairing"),
                        ),
                        self.feature_enabled("devicePairing"),
                    ))
                    .child(capability_row(
                        "集成终端",
                        capability_status(
                            daemon_synchronized,
                            self.feature_enabled("terminal"),
                        ),
                        self.feature_enabled("terminal"),
                    ))
                    .child(capability_row(
                        "Relay",
                        capability_status(daemon_synchronized, self.feature_enabled("relay")),
                        self.feature_enabled("relay"),
                    ))
                    .child(capability_row(
                        "推送通知",
                        capability_status(
                            daemon_synchronized,
                            self.feature_enabled("pushNotifications"),
                        ),
                        self.feature_enabled("pushNotifications"),
                    )),
            )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_appearance_settings(&self, cx: &mut Context<Self>) -> Div {
        let theme_choices =
            div()
                .h_flex()
                .w_full()
                .gap_3()
                .children(
                    ColorScheme::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(index, option)| {
                            appearance_theme_card(
                                index,
                                option,
                                self.appearance.color_scheme == option,
                                self.appearance,
                            )
                            .on_click(cx.listener(
                                move |view, _, window, cx| {
                                    view.set_color_scheme(option, window, cx);
                                },
                            ))
                        }),
                );
        let contrast = appearance_option_group(ContrastLevel::ALL.into_iter().enumerate().map(
            |(index, option)| {
                Button::new(("appearance-contrast", index))
                    .ghost()
                    .small()
                    .selected(self.appearance.contrast == option)
                    .label(option.label())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.set_contrast(option, window, cx);
                    }))
            },
        ));
        let interface_font =
            appearance_option_group(InterfaceFont::ALL.into_iter().enumerate().map(
                |(index, option)| {
                    Button::new(("appearance-interface-font", index))
                        .ghost()
                        .small()
                        .selected(self.appearance.interface_font == option)
                        .label(option.label())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_interface_font(option, window, cx);
                        }))
                },
            ));
        let interface_text_size =
            appearance_option_group(InterfaceTextSize::ALL.into_iter().enumerate().map(
                |(index, option)| {
                    Button::new(("appearance-interface-text", index))
                        .ghost()
                        .small()
                        .selected(self.appearance.interface_text_size == option)
                        .label(option.label())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_interface_text_size(option, window, cx);
                        }))
                },
            ));
        let code_font = appearance_option_group(CodeFont::ALL.into_iter().enumerate().map(
            |(index, option)| {
                Button::new(("appearance-code-font", index))
                    .ghost()
                    .small()
                    .selected(self.appearance.code_font == option)
                    .label(option.label())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.set_code_font(option, window, cx);
                    }))
            },
        ));
        let code_text_size =
            appearance_option_group(CodeTextSize::ALL.into_iter().enumerate().map(
                |(index, option)| {
                    Button::new(("appearance-code-text", index))
                        .ghost()
                        .small()
                        .selected(self.appearance.code_text_size == option)
                        .label(option.label())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_code_text_size(option, window, cx);
                        }))
                },
            ));
        let content_width = appearance_option_group(ContentWidth::ALL.into_iter().enumerate().map(
            |(index, option)| {
                Button::new(("appearance-content-width", index))
                    .ghost()
                    .small()
                    .selected(self.appearance.content_width == option)
                    .label(option.label())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.set_content_width(option, window, cx);
                    }))
            },
        ));

        settings_page_header(
            "外观",
            "自定义 Corbit 的主题颜色、侧栏材质、字体和内容密度。",
        )
        .when_some(self.appearance_error.clone(), |page, error| {
            page.child(
                div()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(COLOR_ERROR))
                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                    .px_4()
                    .py_3()
                    .text_size(font_px(FONT_SIZE_SM))
                    .text_color(rgb(COLOR_ERROR))
                    .child(format!("外观设置未能完成：{error}")),
            )
        })
        .child(
            settings_section("主题", "选择基础外观，系统模式会自动响应 macOS 外观变化。")
                .child(theme_choices),
        )
        .child(
            settings_section(
                "颜色",
                "强调色全局共享，浅色与深色主题分别保存背景和前景色。",
            )
            .child(
                appearance_row_group()
                    .child(appearance_setting_row(
                        "强调色",
                        "用于链接、焦点、选择状态和主题标记。",
                        appearance_color_value(
                            &self.appearance_accent_color,
                            self.appearance.accent_color,
                        ),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "对比度",
                        "调整边框、分隔线和次要文字的清晰程度。",
                        contrast,
                    )),
            )
            .child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_3()
                    .child(appearance_palette_card(
                        "浅色主题",
                        self.appearance.light_background,
                        self.appearance.light_foreground,
                        &self.appearance_light_background,
                        &self.appearance_light_foreground,
                    ))
                    .child(appearance_palette_card(
                        "深色主题",
                        self.appearance.dark_background,
                        self.appearance.dark_foreground,
                        &self.appearance_dark_background,
                        &self.appearance_dark_foreground,
                    )),
            ),
        )
        .child(
            settings_section("界面", "控制导航材质、界面字体与整体显示比例。").child(
                appearance_row_group()
                    .child(appearance_setting_row(
                        "半透明侧栏",
                        "使用 macOS 模糊材质，让侧栏更接近 Codex 的层次。",
                        settings_switch(
                            "appearance-translucent-sidebar",
                            self.appearance.translucent_sidebar,
                        )
                        .on_click(cx.listener(
                            |view, checked, window, cx| {
                                view.set_translucent_sidebar(*checked, window, cx);
                            },
                        )),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "界面字体",
                        "系统字体最贴近原生 Codex，现代无衬线提供更舒展的阅读感。",
                        interface_font,
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "界面字号",
                        "同步调整导航、标题、正文和控件文字。",
                        interface_text_size,
                    )),
            ),
        )
        .child(
            settings_section("代码", "独立调整命令、Diff 和文件内容的字体显示。").child(
                appearance_row_group()
                    .child(appearance_setting_row(
                        "代码字体",
                        "在系统等宽字体和经典等宽字体之间切换。",
                        code_font,
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "代码字号",
                        "应用于命令、Diff、终端输出和文件内容。",
                        code_text_size,
                    )),
            ),
        )
        .child(
            settings_section(
                "布局",
                "保持 Codex 的聚焦阅读宽度，或按工作需要扩展内容区域。",
            )
            .child(appearance_row_group().child(appearance_setting_row(
                "内容宽度",
                "控制任务、对话、搜索结果和设置内容的最大阅读宽度。",
                content_width,
            ))),
        )
        .child(
            settings_section("预览", "所有选项都会立即反映在当前窗口。").child(
                div()
                    .v_flex()
                    .gap_3()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(COLOR_BORDER))
                    .bg(rgb(COLOR_SURFACE))
                    .p_4()
                    .child(
                        div()
                            .font_medium()
                            .child("Corbit 会保持清晰、安静且一致的界面层级。"),
                    )
                    .child(
                        div()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(COLOR_BORDER))
                            .bg(rgb(COLOR_EDITOR))
                            .p_3()
                            .font_family(mono_font_family())
                            .text_size(font_px(FONT_SIZE_MONO))
                            .child("$ corbit task run --workspace current"),
                    ),
            ),
        )
        .child(
            settings_section("共享主题", "复制完整外观配置，或粘贴另一份配置立即导入。").child(
                appearance_row_group()
                    .child(appearance_setting_row(
                        "外观配置",
                        "配置采用可读 JSON，包含主题、颜色、字体和布局偏好。",
                        div()
                            .h_flex()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .child(settings_input(&self.appearance_theme_code).w(px(250.)))
                            .child(
                                settings_action_button("appearance-theme-import", cx)
                                    .label("导入")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.import_appearance_theme(window, cx);
                                    })),
                            )
                            .child(
                                settings_action_button("appearance-theme-copy", cx)
                                    .icon(Icon::new(AppIcon::Copy))
                                    .label("复制")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.copy_appearance_theme(cx);
                                    })),
                            ),
                    ))
                    .child(settings_row_divider())
                    .child(appearance_setting_row(
                        "恢复默认",
                        "恢复 Codex 风格的默认主题、字体和布局。",
                        settings_action_button("appearance-theme-reset", cx)
                            .icon(Icon::new(AppIcon::Refresh))
                            .label("恢复默认")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.reset_appearance_theme(window, cx);
                            })),
                    )),
            ),
        )
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child("外观设置会自动保存，并在下次启动 Corbit 时恢复。"),
        )
    }

    pub(super) fn render_provider_settings(&self, cx: &mut Context<Self>) -> Div {
        let daemon_synchronized = self.provider_catalog.is_some();
        let has_catalog_entries = self
            .provider_catalog
            .as_ref()
            .is_some_and(|catalog| !catalog.providers.is_empty());
        let available_provider_count = self.provider_options().len();
        let selected_provider_label = PROVIDERS
            .iter()
            .find(|provider| provider.id == self.selected_provider)
            .map_or_else(
                || self.selected_provider.clone(),
                |provider| provider.label.to_owned(),
            );
        let availability_summary = if self.provider_catalog_error.is_some() && has_catalog_entries {
            "刷新失败，继续使用上次同步结果".to_owned()
        } else if self.provider_catalog_error.is_some() {
            "Provider 目录读取失败".to_owned()
        } else if daemon_synchronized {
            format!("当前 Daemon 已启用 {available_provider_count} 个")
        } else if self.server_info.is_some() {
            "正在读取 Daemon Provider 目录".to_owned()
        } else {
            "连接 Daemon 后自动检测".to_owned()
        };
        let cards = PROVIDERS
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                provider_setting_card(
                    index,
                    provider,
                    self.selected_provider == provider.id,
                    self.provider_is_available(provider.id),
                    daemon_synchronized,
                    cx,
                )
            })
            .collect::<Vec<_>>();

        settings_page_header(
            "提供商",
            "选择创建新任务与 Agent 时默认使用的执行后端。",
        )
            .child(
                settings_card("默认设置")
                    .child(setting_text_row("当前默认", selected_provider_label))
                    .child(settings_row_divider())
                    .child(setting_text_row("适用范围", "新建任务与新建 Agent"))
                    .child(settings_row_divider())
                    .child(setting_text_row("保存方式", "自动保存在本机界面偏好中")),
            )
            .child(provider_catalog_settings_card(
                self,
                daemon_synchronized,
                has_catalog_entries,
                cx,
            ))
            .child(
                settings_section(
                    "可选择的提供商",
                    "可用状态由当前连接的 Daemon 实时检测；未启用的提供商不会创建会话。",
                )
                .children(cards),
            )
            .child(
                settings_card("连接与凭证")
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .line_height(px(19.))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(
                                "Corbit 不在此页面保存模型 API 密钥。登录状态、命令与凭证由运行 Daemon 的主机管理。",
                            ),
                    )
                    .child(settings_row_divider())
                    .child(setting_text_row("能力来源", availability_summary))
                    .child(settings_row_divider())
                    .child(setting_text_row(
                        "凭证位置",
                        "Daemon 主机上的提供商工具",
                    ))
                    .child(settings_row_divider())
                    .child(setting_text_row(
                        "已有 Agent",
                        "更改默认项不会切换已有会话",
                    )),
            )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_device_settings(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let daemon_synchronized = self.server_info.is_some();
        let supported = self.feature_enabled("devicePairing");
        let device_count = self.devices.len();
        let device_rows = self
            .devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let device_id = device.id.clone();
                let armed = self.pending_revoke_device_id.as_ref() == Some(&device.id);
                let paired_at = format_device_pairing_time(&device.created_at);
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(COLOR_BORDER_HEAVY))
                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                    .p_3()
                    .child(
                        div()
                            .flex_none()
                            .size(px(38.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(rgb(COLOR_BORDER_LIGHT))
                            .bg(rgb(COLOR_SURFACE_UNDER))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(AppIcon::Device).size(px(18.))),
                    )
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .h_flex()
                                    .min_w(px(0.))
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_size(font_px(FONT_SIZE_BASE))
                                            .font_medium()
                                            .child(device.name.clone()),
                                    )
                                    .child(paired_device_status_badge()),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .gap_1()
                                    .min_w(px(0.))
                                    .child(
                                        Icon::new(AppIcon::User)
                                            .size(px(12.))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .child(format!("客户端 ID · {}", device.client_id)),
                                    )
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY)),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(format!("配对时间 · {paired_at}")),
                            ),
                    )
                    .child(
                        settings_danger_action_button(("revoke-device", index), cx)
                            .label(if armed { "确认撤销" } else { "撤销" })
                            .disabled(self.device_operation_in_flight)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.request_device_revoke(&device_id, cx);
                            })),
                    )
            })
            .collect::<Vec<_>>();

        settings_page_header(
            "远程设备",
            "连接手机或远程客户端，生成一次性配对凭证，并随时查看或撤销设备访问。",
        )
            .when(!daemon_synchronized, |page| {
                page.child(
                    div()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .bg(rgb(COLOR_SURFACE_UNDER))
                        .p_4()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("连接并同步 Daemon 后，将自动检测设备配对能力。"),
                )
            })
            .when(daemon_synchronized && !supported, |page| {
                page.child(
                    div()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .bg(rgb(COLOR_SURFACE_UNDER))
                        .p_4()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("当前 Daemon 未启用设备配对能力。"),
                )
            })
            .when(!supported, |page| {
                page.child(device_pairing_tutorial_card())
            })
            .when(supported, |page| {
                page.child(
                    settings_card("配对新设备")
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .line_height(px(19.))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(
                                    "生成一个短时有效且只能使用一次的配对链接，用于向受信任的远程客户端签发独立设备凭证。",
                                ),
                        )
                        .when(!is_online, |card| {
                            card.child(
                                div()
                                    .rounded(px(10.))
                                    .border_1()
                                    .border_color(rgb(COLOR_BORDER_LIGHT))
                                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                                    .p_3()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child(
                                        "Daemon 当前未连接；恢复连接后即可生成配对链接并刷新设备列表。",
                                    ),
                            )
                        })
                        .child(device_pairing_field(
                            "连接地址",
                            "必须是手机或远程客户端能够访问的 Daemon 地址；跨设备连接不要使用 127.0.0.1。",
                            settings_input(&self.pairing_endpoint),
                        ))
                        .child(device_pairing_field(
                            "主机显示名称",
                            "用于让远程客户端识别当前 Corbit 主机，不是手机或远程设备的名称。",
                            settings_input(&self.pairing_host_name),
                        ))
                        .child(
                            div()
                                .h_flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    settings_primary_action_button("create-pairing-offer", cx)
                                        .icon(Icon::new(AppIcon::Add))
                                        .label("生成一次性配对链接")
                                        .loading(self.device_operation_in_flight)
                                        .disabled(!is_online || self.device_operation_in_flight)
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.create_pairing_offer(cx);
                                        })),
                                )
                        )
                        .children(self.pairing_offer.as_ref().map(|offer| {
                            let pairing_uri = offer.pairing_uri.clone();
                            let expires_at = format_device_pairing_time(&offer.expires_at);
                            div()
                                .v_flex()
                                .gap_3()
                                .rounded(px(10.))
                                .border_1()
                                .border_color(rgb(COLOR_BORDER_HEAVY))
                                .bg(rgb(COLOR_SURFACE_SECONDARY))
                                .p_3()
                                .child(
                                    div()
                                        .h_flex()
                                        .flex_wrap()
                                        .justify_between()
                                        .gap_2()
                                        .child(
                                            div()
                                                .h_flex()
                                                .gap_2()
                                                .child(
                                                    Icon::new(AppIcon::Success)
                                                        .size(px(15.))
                                                        .text_color(rgb(COLOR_SUCCESS)),
                                                )
                                                .child(
                                                    div()
                                                        .font_medium()
                                                        .child("配对链接已生成"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .rounded_full()
                                                .border_1()
                                                .border_color(rgb(COLOR_BORDER_LIGHT))
                                                .bg(rgb(COLOR_SURFACE_UNDER))
                                                .px_2()
                                                .py_1()
                                                .text_size(font_px(FONT_SIZE_XS))
                                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                .child(format!("有效至 {expires_at}")),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(rgb(COLOR_BORDER_LIGHT))
                                        .bg(rgb(COLOR_SURFACE_UNDER))
                                        .p_2()
                                        .truncate()
                                        .font_family(mono_font_family())
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .child(offer.pairing_uri.clone()),
                                )
                                .when_some(
                                    offer.tls_certificate_sha256.clone(),
                                    |offer_card, fingerprint| {
                                        offer_card.child(
                                            div()
                                                .truncate()
                                                .font_family(mono_font_family())
                                                .text_size(font_px(FONT_SIZE_XS))
                                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                .child(format!("TLS 指纹 · {fingerprint}")),
                                        )
                                    },
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .flex_wrap()
                                        .items_center()
                                        .justify_between()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_size(font_px(FONT_SIZE_XS))
                                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                .child("链接仅可使用一次，请在过期前完成配对。"),
                                        )
                                        .child(
                                            settings_action_button("copy-pairing-uri", cx)
                                                .icon(Icon::new(AppIcon::Copy))
                                                .label("复制配对链接")
                                                .on_click(cx.listener(move |view, _, _, cx| {
                                                    view.copy_pairing_uri(pairing_uri.clone(), cx);
                                                })),
                                        ),
                                )
                        })),
                )
                .child(device_pairing_tutorial_card())
                .child(
                    settings_card(format!("已配对设备 · {device_count}"))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .justify_between()
                                .flex_wrap()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .line_height(px(19.))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(
                                            "每台设备使用独立凭证。撤销某台设备不会影响其它已配对客户端。",
                                        ),
                                )
                                .child(
                                    settings_action_button("refresh-devices", cx)
                                        .icon(Icon::new(AppIcon::Refresh))
                                        .label("刷新列表")
                                        .loading(self.device_operation_in_flight)
                                        .disabled(!is_online || self.device_operation_in_flight)
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.load_devices(cx);
                                        })),
                                ),
                        )
                        .when(device_rows.is_empty(), |card| {
                            card.child(
                                div()
                                    .v_flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded(px(10.))
                                    .border_1()
                                    .border_color(rgb(COLOR_BORDER_LIGHT))
                                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                                    .px_4()
                                    .py_6()
                                    .child(
                                        div()
                                            .size(px(38.))
                                            .rounded(px(10.))
                                            .bg(rgb(COLOR_SURFACE_UNDER))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(Icon::new(AppIcon::Device).size(px(18.))),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .font_medium()
                                            .child("还没有已配对设备"),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(420.))
                                            .text_center()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .line_height(px(18.))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child(
                                                "生成一次性链接并在远程客户端完成确认后，设备会显示在这里。",
                                            ),
                                    ),
                            )
                        })
                        .children(device_rows),
                )
            })
    }

    pub(super) fn render_shortcut_settings() -> Div {
        settings_page_header("快捷键", "使用全局导航快捷键快速切换 Corbit 的主要功能。")
            .child(
                settings_card("导航")
                    .child(shortcut_row("新建任务", "⌘ N", "Ctrl N"))
                    .child(shortcut_row("搜索", "⌘ K", "Ctrl K"))
                    .child(shortcut_row("任务", "⌘ 1", "Ctrl 1"))
                    .child(shortcut_row("活动", "⇧ ⌘ A", "Ctrl Shift A"))
                    .child(shortcut_row("设置", "⌘ ,", "Ctrl ,")),
            )
            .child(
                settings_card("使用说明")
                    .child(setting_text_row("作用范围", "Corbit 窗口内任意页面"))
                    .child(setting_text_row("搜索", "打开后自动聚焦搜索框"))
                    .child(setting_text_row("平台", "macOS / Windows / Linux")),
            )
    }

    pub(super) fn render_about_settings(&self, cx: &mut Context<Self>) -> Div {
        settings_page_header("关于软件", "原生桌面客户端与本地 Agent 工作空间。")
            .child(
                div()
                    .h_flex()
                    .gap_4()
                    .items_center()
                    .py_3()
                    .child(brand_mark(48.))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_HEADING))
                                    .font_semibold()
                                    .child(if build_info::is_development() {
                                        "Corbit Dev"
                                    } else {
                                        "Corbit"
                                    }),
                            )
                            .child(
                                div()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child(build_info::version_label()),
                            ),
                    ),
            )
            .child(
                settings_card("版本与构建")
                    .child(setting_text_row("桌面版本", env!("CARGO_PKG_VERSION")))
                    .child(setting_text_row("渠道", build_info::channel_label()))
                    .child(setting_text_row("构建配置", build_info::PROFILE))
                    .child(setting_text_row("目标", build_info::TARGET)),
            )
            .child(
                settings_card("产品架构")
                    .child(setting_text_row("界面", "GPUI 原生桌面应用"))
                    .child(setting_text_row(
                        "执行服务",
                        format!(
                            "Corbit Daemon {}",
                            self.local_daemon_status.expected_version
                        ),
                    ))
                    .child(setting_text_row("通信", "版本化 WebSocket + RPC"))
                    .child(setting_text_row("状态", "权威快照与事件游标恢复")),
            )
            .child(
                settings_card("数据与安全")
                    .child(setting_text_row("界面状态", "仅保存在当前设备"))
                    .child(setting_text_row("项目文件", "由所连接的 Daemon 访问"))
                    .child(setting_text_row(
                        "凭证",
                        "Bearer Token，不写入普通配置或日志",
                    ))
                    .child(setting_text_row("模型登录", "由 Provider 工具自行管理")),
            )
            .child(
                settings_card("软件许可")
                    .child(setting_text_row("Corbit", "未公开授权（UNLICENSED）"))
                    .child(setting_text_row("第三方组件", "遵循各组件独立许可与条款"))
                    .child(
                        div().pt_1().child(
                            settings_action_button("settings-about-open-third-party-licenses", cx)
                                .icon(Icon::new(AppIcon::File))
                                .label("查看开源许可")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.set_resource_section(
                                        ResourceSection::ThirdPartyLicenses,
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
    }

    pub(super) fn render_third_party_license_settings() -> Div {
        settings_page_header(
            "开源许可",
            "Corbit Desktop 与随包 Daemon 使用的软件组件及资源声明。",
        )
        .child(
            settings_card("许可说明")
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .line_height(px(20.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(
                            "下列组件保留各自版权，并分别受对应许可证或商业条款约束。第三方许可只适用于相应组件，不改变 Corbit 自身的授权状态。",
                        ),
                )
                .child(setting_text_row("Corbit 产品代码", "UNLICENSED"))
                .child(setting_text_row(
                    "开源组件",
                    "MIT、Apache-2.0、ISC 等独立许可",
                ))
                .child(setting_text_row(
                    "商业组件",
                    "按供应商条款使用，不标记为开源软件",
                )),
        )
        .child(third_party_license_card(
            "桌面客户端主要依赖",
            DESKTOP_THIRD_PARTY_NOTICES,
        ))
        .child(third_party_license_card(
            "随包 Daemon 直接依赖",
            DAEMON_THIRD_PARTY_NOTICES,
        ))
        .child(third_party_license_card(
            "图标与服务标识",
            RESOURCE_THIRD_PARTY_NOTICES,
        ))
        .child(
            settings_card("条款说明")
                .child(setting_text_row(
                    "双重许可",
                    "MIT OR Apache-2.0 表示可按其中任一许可证使用",
                ))
                .child(setting_text_row(
                    "商标",
                    "OpenAI、Codex、Anthropic 与 Claude 标识归各自权利人所有",
                ))
                .child(setting_text_row(
                    "完整条款",
                    "以组件来源页面和随组件提供的许可证文本为准",
                )),
        )
    }
}

#[derive(Clone, Copy)]
struct ThirdPartyNotice {
    button_id: &'static str,
    name: &'static str,
    version: &'static str,
    license: &'static str,
    description: &'static str,
    source: &'static str,
}

const DESKTOP_THIRD_PARTY_NOTICES: &[ThirdPartyNotice] = &[
    ThirdPartyNotice {
        button_id: "license-gpui",
        name: "GPUI",
        version: "0.2.2",
        license: "Apache-2.0",
        description: "Zed Industries 的 GPU 加速桌面 UI 框架。",
        source: "https://crates.io/crates/gpui/0.2.2",
    },
    ThirdPartyNotice {
        button_id: "license-gpui-component",
        name: "GPUI Component",
        version: "0.5.1",
        license: "Apache-2.0",
        description: "Longbridge 提供的 GPUI 桌面组件库。",
        source: "https://crates.io/crates/gpui-component/0.5.1",
    },
    ThirdPartyNotice {
        button_id: "license-tokio",
        name: "Tokio",
        version: "1.49.0",
        license: "MIT",
        description: "Rust 异步运行时与网络基础设施。",
        source: "https://crates.io/crates/tokio/1.49.0",
    },
    ThirdPartyNotice {
        button_id: "license-reqwest",
        name: "Reqwest",
        version: "0.13.4",
        license: "MIT OR Apache-2.0",
        description: "Daemon HTTP 客户端。",
        source: "https://crates.io/crates/reqwest/0.13.4",
    },
    ThirdPartyNotice {
        button_id: "license-tokio-tungstenite",
        name: "Tokio Tungstenite",
        version: "0.30.0",
        license: "MIT",
        description: "异步 WebSocket 通信。",
        source: "https://crates.io/crates/tokio-tungstenite/0.30.0",
    },
    ThirdPartyNotice {
        button_id: "license-serde",
        name: "Serde / serde_json",
        version: "1.0.228 / 1.0.149",
        license: "MIT OR Apache-2.0",
        description: "配置、协议与状态数据序列化。",
        source: "https://crates.io/crates/serde/1.0.228",
    },
    ThirdPartyNotice {
        button_id: "license-rust-foundation",
        name: "Rust 基础库",
        version: "当前锁定版本",
        license: "MIT OR Apache-2.0",
        description: "anyhow、async-channel、base64、chrono、futures、http、thiserror、url 与 uuid。",
        source: "https://crates.io/",
    },
    ThirdPartyNotice {
        button_id: "license-desktop-platform",
        name: "桌面平台库",
        version: "当前锁定版本",
        license: "MIT OR Apache-2.0",
        description: "image、objc2、objc2-foundation、security-framework 与 tray-icon。",
        source: "https://crates.io/",
    },
];

const DAEMON_THIRD_PARTY_NOTICES: &[ThirdPartyNotice] = &[
    ThirdPartyNotice {
        button_id: "license-acp-sdk",
        name: "Agent Client Protocol SDK",
        version: "0.17.1",
        license: "Apache-2.0",
        description: "兼容 ACP 的 Agent 会话协议实现。",
        source: "https://www.npmjs.com/package/@agentclientprotocol/sdk/v/0.17.1",
    },
    ThirdPartyNotice {
        button_id: "license-claude-agent-sdk",
        name: "Claude Agent SDK",
        version: "0.3.232",
        license: "Anthropic 商业条款",
        description: "用于 Claude Provider；该 SDK 不作为开源组件标记。",
        source: "https://www.anthropic.com/legal/commercial-terms",
    },
    ThirdPartyNotice {
        button_id: "license-fastify",
        name: "Fastify / @fastify/websocket",
        version: "5.12.0 / 11.3.0",
        license: "MIT",
        description: "Daemon HTTP 与 WebSocket 服务。",
        source: "https://www.npmjs.com/package/fastify/v/5.12.0",
    },
    ThirdPartyNotice {
        button_id: "license-ajv",
        name: "Ajv",
        version: "8.20.0",
        license: "MIT",
        description: "JSON Schema 校验。",
        source: "https://www.npmjs.com/package/ajv/v/8.20.0",
    },
    ThirdPartyNotice {
        button_id: "license-fflate",
        name: "fflate",
        version: "0.8.3",
        license: "MIT",
        description: "压缩数据处理。",
        source: "https://www.npmjs.com/package/fflate/v/0.8.3",
    },
    ThirdPartyNotice {
        button_id: "license-node-pty",
        name: "node-pty",
        version: "1.1.0",
        license: "MIT",
        description: "本机终端伪终端支持。",
        source: "https://www.npmjs.com/package/node-pty/v/1.1.0",
    },
    ThirdPartyNotice {
        button_id: "license-qrcode",
        name: "qrcode",
        version: "1.5.4",
        license: "MIT",
        description: "设备配对二维码生成。",
        source: "https://www.npmjs.com/package/qrcode/v/1.5.4",
    },
];

const RESOURCE_THIRD_PARTY_NOTICES: &[ThirdPartyNotice] = &[
    ThirdPartyNotice {
        button_id: "license-lucide",
        name: "Lucide Icons",
        version: "资源快照",
        license: "ISC",
        description: "Corbit 导航与操作图标，许可文本随资源目录保留。",
        source: "https://lucide.dev/license",
    },
    ThirdPartyNotice {
        button_id: "license-openai-mark",
        name: "OpenAI / Codex 服务标识",
        version: "资源快照",
        license: "商标声明",
        description: "仅用于识别 Codex Provider，不表示 OpenAI 对 Corbit 的认可。",
        source: "https://openai.com/brand/",
    },
    ThirdPartyNotice {
        button_id: "license-anthropic-mark",
        name: "Anthropic / Claude 服务标识",
        version: "资源快照",
        license: "商标声明",
        description: "仅用于识别 Claude Provider，不表示 Anthropic 对 Corbit 的认可。",
        source: "https://www.anthropic.com/legal",
    },
];

fn third_party_license_card(
    title: &'static str,
    notices: &'static [ThirdPartyNotice],
) -> SettingsCard {
    settings_card(title).children(notices.iter().copied().map(third_party_license_row))
}

fn third_party_license_row(notice: ThirdPartyNotice) -> Div {
    let source = notice.source;
    div()
        .v_flex()
        .gap_1()
        .py_1()
        .child(
            div()
                .h_flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .h_flex()
                        .min_w(px(0.))
                        .gap_2()
                        .child(div().font_medium().child(notice.name))
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(notice.version),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_none()
                        .gap_2()
                        .child(
                            div()
                                .rounded_md()
                                .bg(rgb(COLOR_SURFACE_SECONDARY))
                                .px_2()
                                .py_1()
                                .text_size(font_px(FONT_SIZE_XS))
                                .child(notice.license),
                        )
                        .child(
                            settings_quiet_action_button(notice.button_id)
                                .icon(Icon::new(AppIcon::ExternalLink))
                                .label("来源")
                                .on_click(move |_, _, cx| cx.open_url(source)),
                        ),
                ),
        )
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .line_height(px(18.))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(notice.description),
        )
}

fn provider_setting_card(
    index: usize,
    provider: &ProviderInfo,
    selected: bool,
    enabled: bool,
    daemon_synchronized: bool,
    cx: &mut Context<ConnectionView>,
) -> Div {
    let provider_id = provider.id;
    let availability_label = if enabled {
        "可用"
    } else if daemon_synchronized {
        "未启用"
    } else {
        "等待同步"
    };
    let availability_color = if enabled {
        COLOR_SUCCESS
    } else {
        COLOR_TEXT_TERTIARY
    };
    let action_label = if !enabled && !daemon_synchronized {
        "等待连接"
    } else if !enabled {
        "不可用"
    } else if selected {
        "当前默认"
    } else {
        "设为默认"
    };

    div()
        .h_flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(if selected {
            COLOR_BORDER_HEAVY
        } else {
            COLOR_BORDER
        }))
        .bg(rgb(if selected {
            COLOR_SURFACE_SECONDARY
        } else {
            COLOR_SURFACE
        }))
        .p_4()
        .child(provider_badge(provider.id, ProviderBadgeSize::Settings))
        .child(
            div()
                .v_flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(div().font_medium().child(provider.label))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(availability_color))
                                .child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(rgb(availability_color)),
                                )
                                .child(availability_label),
                        ),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(provider.description),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .line_height(px(18.))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(provider.detail),
                ),
        )
        .child(
            settings_action_button(("provider-setting-select", index), cx)
                .selected(selected)
                .label(action_label)
                .disabled(!enabled)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.choose_default_provider(provider_id, cx);
                })),
        )
}

pub(super) fn settings_page_header(title: &'static str, description: &'static str) -> Div {
    div().v_flex().w_full().gap_6().child(
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_HEADING))
                    .font_semibold()
                    .child(title),
            )
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_SM))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(description),
            ),
    )
}

fn provider_catalog_settings_card(
    view: &ConnectionView,
    daemon_synchronized: bool,
    has_catalog_entries: bool,
    cx: &mut Context<ConnectionView>,
) -> SettingsCard {
    let refreshing = view.provider_catalog_task.is_some();
    let is_online = matches!(view.state, corbit_client::ConnectionState::Online);
    let status = if refreshing {
        "正在刷新"
    } else if view.provider_catalog_error.is_some() && has_catalog_entries {
        "刷新失败，已保留上次结果"
    } else if view.provider_catalog_error.is_some() {
        "目录读取失败"
    } else if daemon_synchronized {
        "已同步"
    } else {
        "等待连接"
    };
    let status_color = if refreshing {
        COLOR_TEXT_SECONDARY
    } else if view.provider_catalog_error.is_some() {
        COLOR_WARNING
    } else if daemon_synchronized {
        COLOR_SUCCESS
    } else {
        COLOR_TEXT_TERTIARY
    };

    settings_card("实时模型目录")
        .child(setting_value_row(
            "同步状态",
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().size(px(6.)).rounded_full().bg(rgb(status_color)))
                .child(status),
        ))
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .line_height(px(18.))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child("目录来自当前 Daemon 的实时能力检测，后台会每分钟自动刷新。"),
        )
        .child(
            div().h_flex().justify_end().child(
                settings_action_button("settings-refresh-provider-catalog", cx)
                    .icon(Icon::new(AppIcon::Refresh))
                    .label("立即刷新")
                    .loading(refreshing)
                    .disabled(!is_online || refreshing)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.refresh_provider_catalog(cx);
                    })),
            ),
        )
}

fn appearance_theme_card(
    index: usize,
    option: ColorScheme,
    selected: bool,
    appearance: AppearancePreferences,
) -> Button {
    let (background, foreground, sidebar) = match option {
        ColorScheme::System => (
            appearance.light_background,
            appearance.light_foreground,
            appearance.dark_background,
        ),
        ColorScheme::Light => (
            appearance.light_background,
            appearance.light_foreground,
            blend_hex(appearance.light_background, appearance.light_foreground, 40),
        ),
        ColorScheme::Dark => (
            appearance.dark_background,
            appearance.dark_foreground,
            blend_hex(appearance.dark_background, 0x00_0000, 280),
        ),
    };

    Button::new(("appearance-theme-card", index))
        .ghost()
        .small()
        .flex_1()
        .min_w(px(0.))
        .h(px(116.))
        .rounded_lg()
        .border_1()
        .border_color(if selected {
            fixed_rgb(appearance.accent_color)
        } else {
            rgb(COLOR_BORDER)
        })
        .bg(rgb(COLOR_SURFACE))
        .p_2()
        .child(
            div()
                .v_flex()
                .w_full()
                .gap_2()
                .child(appearance_theme_preview(background, foreground, sidebar))
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .justify_between()
                        .text_size(font_px(FONT_SIZE_XS))
                        .font_medium()
                        .child(option.card_label())
                        .when(selected, |row| {
                            row.child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(fixed_rgb(appearance.accent_color)),
                            )
                        }),
                ),
        )
}

fn appearance_theme_preview(background: u32, foreground: u32, sidebar: u32) -> Div {
    let subtle = blend_hex(background, foreground, 80);
    let line = blend_hex(background, foreground, 180);
    let sidebar_line = blend_hex(sidebar, foreground, 150);

    div()
        .h_flex()
        .w_full()
        .h(px(72.))
        .overflow_hidden()
        .rounded_lg()
        .border_1()
        .border_color(fixed_rgb(subtle))
        .child(
            div()
                .v_flex()
                .h_full()
                .w(px(42.))
                .flex_none()
                .gap_1()
                .bg(fixed_rgb(sidebar))
                .p_2()
                .child(
                    div()
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(fixed_rgb(blend_hex(sidebar, foreground, 240))),
                )
                .child(
                    div()
                        .h(px(3.))
                        .w(px(20.))
                        .rounded_full()
                        .bg(fixed_rgb(sidebar_line)),
                )
                .child(
                    div()
                        .h(px(3.))
                        .w(px(14.))
                        .rounded_full()
                        .bg(fixed_rgb(sidebar_line)),
                ),
        )
        .child(
            div()
                .v_flex()
                .h_full()
                .flex_1()
                .gap_2()
                .bg(fixed_rgb(background))
                .p_3()
                .child(
                    div()
                        .h(px(5.))
                        .w(px(54.))
                        .rounded_full()
                        .bg(fixed_rgb(foreground)),
                )
                .child(div().h(px(3.)).w_full().rounded_full().bg(fixed_rgb(line)))
                .child(
                    div()
                        .h(px(3.))
                        .w(px(70.))
                        .rounded_full()
                        .bg(fixed_rgb(line)),
                ),
        )
}

fn settings_section(title: &'static str, description: &'static str) -> Div {
    div().v_flex().w_full().gap_3().child(
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_SM))
                    .font_semibold()
                    .child(title),
            )
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                    .child(description),
            ),
    )
}

fn configuration_permission_label(mode: corbit_client::AgentPermissionMode) -> &'static str {
    match mode {
        corbit_client::AgentPermissionMode::ReadOnly => "请求批准",
        corbit_client::AgentPermissionMode::WorkspaceWrite => "帮我批准",
        corbit_client::AgentPermissionMode::FullAccess => "完全访问",
    }
}

fn reasoning_summary_label(summary: corbit_client::AgentReasoningSummary) -> &'static str {
    match summary {
        corbit_client::AgentReasoningSummary::Auto => "自动",
        corbit_client::AgentReasoningSummary::Concise => "简洁",
        corbit_client::AgentReasoningSummary::Detailed => "详细",
        corbit_client::AgentReasoningSummary::None => "关闭",
    }
}

fn personality_label(personality: Option<corbit_client::AgentPersonality>) -> &'static str {
    match personality {
        None => "模型默认",
        Some(corbit_client::AgentPersonality::Friendly) => "友好自然",
        Some(corbit_client::AgentPersonality::Pragmatic) => "简洁务实",
        Some(corbit_client::AgentPersonality::None) => "无预设风格",
    }
}

fn appearance_row_group() -> Div {
    div()
        .v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(COLOR_BORDER_HEAVY))
        .bg(rgb(COLOR_EDITOR))
}

fn appearance_color_value(state: &Entity<ColorPickerState>, color: u32) -> Div {
    div()
        .h_flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(
            div()
                .font_family(mono_font_family())
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(format!("#{color:06x}")),
        )
        .child(
            ColorPicker::new(state)
                .small()
                .featured_colors(featured_theme_colors()),
        )
}

fn featured_theme_colors() -> Vec<gpui::Hsla> {
    [
        0x33_9cff, 0x8b_5cf6, 0x16_a34a, 0xea_580c, 0xdb_2777, 0x6b_7280,
    ]
    .into_iter()
    .map(|color| fixed_rgb(color).into())
    .collect()
}

fn appearance_palette_card(
    title: &'static str,
    background: u32,
    foreground: u32,
    background_state: &Entity<ColorPickerState>,
    foreground_state: &Entity<ColorPickerState>,
) -> Div {
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.))
        .gap_3()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(COLOR_BORDER_HEAVY))
        .bg(rgb(COLOR_EDITOR))
        .p_4()
        .child(
            div()
                .h_flex()
                .justify_between()
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .font_medium()
                        .child(title),
                )
                .child(
                    div()
                        .h_flex()
                        .overflow_hidden()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .child(div().size(px(14.)).bg(fixed_rgb(background)))
                        .child(div().size(px(14.)).bg(fixed_rgb(foreground))),
                ),
        )
        .child(appearance_palette_color_row(
            "背景",
            background,
            background_state,
        ))
        .child(appearance_palette_color_row(
            "前景",
            foreground,
            foreground_state,
        ))
}

fn appearance_palette_color_row(
    label: &'static str,
    color: u32,
    state: &Entity<ColorPickerState>,
) -> Div {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_3()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(COLOR_TEXT_SECONDARY))
        .child(label)
        .child(appearance_color_value(state, color))
}

fn appearance_option_group(options: impl IntoIterator<Item = Button>) -> Div {
    div()
        .h_flex()
        .flex_none()
        .gap_1()
        .rounded(px(8.))
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .p(px(2.))
        .children(options)
}

fn appearance_setting_row(
    label: &'static str,
    description: &'static str,
    value: impl IntoElement,
) -> Div {
    div()
        .h_flex()
        .min_h(px(58.))
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap_6()
        .px_4()
        .py_3()
        .child(
            div()
                .v_flex()
                .min_w(px(0.))
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
        .child(value)
}

fn settings_row_divider() -> Div {
    div().h(px(1.)).w_full().bg(rgb(COLOR_BORDER_LIGHT))
}

fn setting_value_row(label: &'static str, value: Div) -> Div {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_4()
        .text_size(font_px(FONT_SIZE_SM))
        .child(div().text_color(rgb(COLOR_TEXT_SECONDARY)).child(label))
        .child(value)
}

fn setting_text_row(label: &'static str, value: impl Into<SharedString>) -> Div {
    setting_value_row(label, div().min_w(px(0.)).truncate().child(value.into()))
}

fn shortcut_row(label: &'static str, macos: &'static str, portable: &'static str) -> Div {
    setting_value_row(
        label,
        div().h_flex().gap_2().child(shortcut_key(macos)).child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child(format!("Windows / Linux  {portable}")),
        ),
    )
}

fn shortcut_key(label: &'static str) -> Div {
    div()
        .rounded_md()
        .border_1()
        .border_color(rgb(COLOR_BORDER_HEAVY))
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .px_2()
        .py_1()
        .font_family(mono_font_family())
        .text_size(font_px(FONT_SIZE_XS))
        .child(label)
}

fn capability_row(label: &'static str, detail: impl Into<SharedString>, enabled: bool) -> Div {
    setting_value_row(
        label,
        div()
            .h_flex()
            .gap_2()
            .child(div().size(px(6.)).rounded_full().bg(rgb(if enabled {
                COLOR_SUCCESS
            } else {
                COLOR_TEXT_TERTIARY
            })))
            .child(detail.into()),
    )
}

const fn capability_status(synchronized: bool, enabled: bool) -> &'static str {
    if !synchronized {
        "连接后自动检测"
    } else if enabled {
        "Daemon 已启用"
    } else {
        "当前 Daemon 未启用"
    }
}

#[cfg(test)]
mod device_settings_tests {
    use super::*;

    #[test]
    fn device_timestamp_is_compact_and_localized() {
        let formatted = format_device_pairing_time("2026-08-22T14:34:44.140Z");

        assert_eq!(formatted.chars().count(), 16);
        assert!(!formatted.contains('T'));
    }

    #[test]
    fn unknown_device_timestamp_is_preserved() {
        assert_eq!(format_device_pairing_time("刚刚"), "刚刚");
    }
}

#[cfg(test)]
mod third_party_license_tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn third_party_notices_are_complete_and_have_unique_actions() {
        let notices = DESKTOP_THIRD_PARTY_NOTICES
            .iter()
            .chain(DAEMON_THIRD_PARTY_NOTICES)
            .chain(RESOURCE_THIRD_PARTY_NOTICES);
        let mut button_ids = BTreeSet::new();

        for notice in notices {
            assert!(!notice.name.is_empty());
            assert!(!notice.version.is_empty());
            assert!(!notice.license.is_empty());
            assert!(!notice.description.is_empty());
            assert!(notice.source.starts_with("https://"));
            assert!(button_ids.insert(notice.button_id));
        }
    }

    #[test]
    fn claude_agent_sdk_is_not_presented_as_open_source() {
        let notice = DAEMON_THIRD_PARTY_NOTICES
            .iter()
            .find(|notice| notice.name == "Claude Agent SDK")
            .expect("Claude Agent SDK notice should exist");

        assert_eq!(notice.license, "Anthropic 商业条款");
    }
}
