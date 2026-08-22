use super::*;

impl ConnectionView {
    pub(super) fn resource_section_label(&self) -> &'static str {
        match self.resource_section {
            ResourceSection::General => "常规",
            ResourceSection::Appearance => "外观",
            ResourceSection::Providers => "模型提供商",
            ResourceSection::Plugins => "插件",
            ResourceSection::Shortcuts => "快捷键",
            ResourceSection::Projects => "项目",
            ResourceSection::Workspaces => "工作区",
            ResourceSection::Agents => "Agent",
            ResourceSection::Devices => "设备",
            ResourceSection::About => "关于 Corbit",
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

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_general_settings(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
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

        settings_page_header("常规", "连接状态、Daemon 身份和当前运行能力。")
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
                            .child(Input::new(&self.connection_endpoint).small()),
                    ))
                    .child(setting_value_row(
                        "访问 Token",
                        div().w(px(330.)).max_w_full().child(
                            Input::new(&self.connection_token)
                                .small()
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
                                Button::new("settings-save-connection")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("保存并重新连接")
                                    .loading(is_connecting)
                                    .disabled(is_connecting)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.save_connection_settings(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-detect-local-daemon")
                                    .outline()
                                    .small()
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
                                        Button::new("settings-delete-connection-credential")
                                            .danger()
                                            .small()
                                            .label("移除已存 Token")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.delete_saved_connection_credential(cx);
                                            })),
                                    )
                                },
                            )
                            .child(
                                Button::new("settings-reconnect")
                                    .outline()
                                    .small()
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
                                Button::new("settings-refresh-daemon-diagnostics")
                                    .outline()
                                    .small()
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("重新检测")
                                    .loading(daemon_status.phase.is_busy())
                                    .disabled(daemon_action_busy)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.refresh_daemon_diagnostics(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-restart-local-daemon")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("安全重启")
                                    .loading(daemon_status.phase == local_daemon::DaemonPhase::Restarting)
                                    .disabled(!daemon_can_restart)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.restart_local_daemon(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-open-daemon-logs")
                                    .outline()
                                    .small()
                                    .icon(Icon::new(AppIcon::FolderOpen))
                                    .label("打开日志")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.open_daemon_logs(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-copy-daemon-diagnostics")
                                    .ghost()
                                    .small()
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
                        Switch::new("appearance-translucent-sidebar")
                            .small()
                            .checked(self.appearance.translucent_sidebar)
                            .on_click(cx.listener(|view, checked, window, cx| {
                                view.set_translucent_sidebar(*checked, window, cx);
                            })),
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
                            .child(Input::new(&self.appearance_theme_code).small().w(px(250.)))
                            .child(
                                Button::new("appearance-theme-import")
                                    .outline()
                                    .small()
                                    .label("导入")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.import_appearance_theme(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("appearance-theme-copy")
                                    .outline()
                                    .small()
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
                        Button::new("appearance-theme-reset")
                            .outline()
                            .small()
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
            "模型提供商",
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
                    "可选择的模型提供商",
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
        let device_rows = self
            .devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let device_id = device.id.clone();
                let armed = self.pending_revoke_device_id.as_ref() == Some(&device.id);
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .child(
                        div()
                            .flex_none()
                            .size(px(30.))
                            .rounded_lg()
                            .bg(rgb(COLOR_SURFACE_SECONDARY))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(AppIcon::User).size(px(15.))),
                    )
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .child(div().font_medium().child(device.name.clone()))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(format!(
                                        "{} · 配对于 {}",
                                        device.client_id, device.created_at
                                    )),
                            ),
                    )
                    .child(
                        Button::new(("revoke-device", index))
                            .danger()
                            .small()
                            .label(if armed { "确认撤销" } else { "撤销" })
                            .disabled(self.device_operation_in_flight)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.request_device_revoke(&device_id, cx);
                            })),
                    )
            })
            .collect::<Vec<_>>();

        settings_page_header("设备", "配对手机或远程客户端，并管理已签发的设备凭证。")
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
            .when(supported, |page| {
                page.child(
                    settings_card("配对新设备")
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .line_height(px(19.))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(
                                    "填写手机能够访问的地址。127.0.0.1 只指向手机自身，跨设备配对请使用局域网 IP、HTTPS 或 Relay 地址。",
                                ),
                        )
                        .child(Input::new(&self.pairing_endpoint).small())
                        .child(Input::new(&self.pairing_host_name).small())
                        .child(
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    Button::new("create-pairing-offer")
                                        .primary()
                                        .small()
                                        .icon(Icon::new(AppIcon::Add))
                                        .label("生成一次性配对链接")
                                        .loading(self.device_operation_in_flight)
                                        .disabled(!is_online || self.device_operation_in_flight)
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.create_pairing_offer(cx);
                                        })),
                                )
                                .child(
                                    Button::new("refresh-devices")
                                        .outline()
                                        .small()
                                        .icon(Icon::new(AppIcon::Refresh))
                                        .label("刷新")
                                        .disabled(!is_online || self.device_operation_in_flight)
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.load_devices(cx);
                                        })),
                                ),
                        )
                        .children(self.pairing_offer.as_ref().map(|offer| {
                            let pairing_uri = offer.pairing_uri.clone();
                            div()
                                .v_flex()
                                .gap_2()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_BORDER_HEAVY))
                                .bg(rgb(COLOR_SURFACE_SECONDARY))
                                .p_3()
                                .child(
                                    div()
                                        .h_flex()
                                        .justify_between()
                                        .child(div().font_medium().child("一次性配对链接"))
                                        .child(
                                            div()
                                                .text_size(font_px(FONT_SIZE_XS))
                                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                .child(format!("有效至 {}", offer.expires_at)),
                                        ),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(mono_font_family())
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .child(offer.pairing_uri.clone()),
                                )
                                .child(
                                    Button::new("copy-pairing-uri")
                                        .outline()
                                        .small()
                                        .icon(Icon::new(AppIcon::Copy))
                                        .label("复制配对链接")
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.copy_pairing_uri(pairing_uri.clone(), cx);
                                        })),
                                )
                        })),
                )
                .child(
                    settings_card("已配对设备")
                        .when(device_rows.is_empty(), |card| {
                            card.child(
                                div()
                                    .py_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("尚无已配对设备。"),
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

    pub(super) fn render_about_settings() -> Div {
        settings_page_header("关于 Corbit", "原生桌面客户端与本地 Agent 工作空间。")
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
                settings_card("构建信息")
                    .child(setting_text_row("渠道", build_info::channel_label()))
                    .child(settings_row_divider())
                    .child(setting_text_row("目标", build_info::TARGET)),
            )
            .child(
                settings_card("工作方式")
                    .child(setting_text_row("界面", "GPUI 原生桌面应用"))
                    .child(setting_text_row(
                        "连接",
                        "本机 Daemon + 版本化 WebSocket 协议",
                    ))
                    .child(setting_text_row("状态", "Daemon 权威快照与事件游标恢复"))
                    .child(setting_text_row(
                        "凭证",
                        "Bearer Token，不写入普通配置或日志",
                    )),
            )
    }
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
            Button::new(("provider-setting-select", index))
                .outline()
                .small()
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

pub(super) fn settings_card(title: &'static str) -> Div {
    div()
        .v_flex()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
        .p_4()
        .child(div().font_medium().child(title))
}

fn provider_catalog_settings_card(
    view: &ConnectionView,
    daemon_synchronized: bool,
    has_catalog_entries: bool,
    cx: &mut Context<ConnectionView>,
) -> Div {
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
            Button::new("settings-refresh-provider-catalog")
                .outline()
                .small()
                .icon(Icon::new(AppIcon::Refresh))
                .label("立即刷新")
                .loading(refreshing)
                .disabled(!is_online || refreshing)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.refresh_provider_catalog(cx);
                })),
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

fn appearance_row_group() -> Div {
    div()
        .v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
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
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
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
