use super::settings::{settings_card, settings_page_header};
use super::*;

impl ConnectionView {
    pub(super) fn load_plugins(&mut self, cx: &mut Context<Self>) {
        if self.plugin_task.is_some()
            || !matches!(self.state, corbit_client::ConnectionState::Online)
            || !self.feature_enabled("plugins")
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
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let plugins = client.plugins().await;
            let marketplace = client.plugin_marketplace().await;
            let audit = client.plugin_audit(Some(50)).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                match (plugins, marketplace) {
                    (Ok(plugins), Ok(marketplace)) => {
                        view.plugins = plugins;
                        view.plugin_marketplace = marketplace;
                        match audit {
                            Ok(audit) => view.plugin_audit = audit,
                            Err(error) if plugin_audit_is_unsupported(&error) => {
                                view.plugin_audit.clear();
                            }
                            Err(error) => {
                                view.plugin_audit.clear();
                                view.show_warning(format!("读取插件执行记录失败：{error}"), cx);
                            }
                        }
                        view.pending_plugin_uninstall = None;
                        view.pending_plugin_update = None;
                        view.pending_plugin_write = None;
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        view.show_error(format!("读取插件目录失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_plugin_package(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugin_operation_in_flight {
            self.show_warning("插件操作正在执行，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("选择插件目录或 .corbit-plugin 文件".into()),
        });
        let window_handle = window.window_handle();
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let selection = path_prompt.await;
            let path = match selection {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    if let Some(view) = view.upgrade() {
                        let _ = view.update(cx, |view, cx| {
                            view.plugin_operation_in_flight = false;
                            view.plugin_task = None;
                            view.show_error(format!("无法打开插件选择器：{error}"), cx);
                            cx.notify();
                        });
                    }
                    return;
                }
                Err(_) => {
                    if let Some(view) = view.upgrade() {
                        let _ = view.update(cx, |view, cx| {
                            view.plugin_operation_in_flight = false;
                            view.plugin_task = None;
                            view.show_error("插件选择器意外关闭，请重试", cx);
                            cx.notify();
                        });
                    }
                    return;
                }
            };
            let Some(path) = path else {
                if let Some(view) = view.upgrade() {
                    let _ = view.update(cx, |view, cx| {
                        view.plugin_operation_in_flight = false;
                        view.plugin_task = None;
                        cx.notify();
                    });
                }
                return;
            };
            let Some(view_entity) = view.upgrade() else {
                return;
            };
            let result = client
                .inspect_plugin(path.to_string_lossy().into_owned())
                .await;
            match result {
                Ok(inspection) => {
                    let _ = cx.update_window(window_handle, |_, window, app| {
                        view_entity.update(app, |view, cx| {
                            view.plugin_task = None;
                            view.plugin_operation_in_flight = false;
                            view.pending_plugin_inspection = Some(inspection.clone());
                            cx.notify();
                        });
                        open_plugin_inspection_dialog(&view_entity, &inspection, window, app);
                    });
                }
                Err(error) => {
                    let _ = view_entity.update(cx, |view, cx| {
                        view.plugin_task = None;
                        view.plugin_operation_in_flight = false;
                        let message = if matches!(
                            error,
                            corbit_client::ClientError::Rpc(ref body)
                                if body.code == "method_not_found"
                        ) {
                            "当前 Daemon 不支持本地插件预检，请更新 Daemon 后重试".to_owned()
                        } else {
                            format!("插件预检失败：{error}")
                        };
                        view.show_error(message, cx);
                        cx.notify();
                    });
                }
            }
        }));
        cx.notify();
    }

    fn install_inspected_plugin(
        &mut self,
        inspection_id: String,
        is_update: bool,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_operation_in_flight {
            self.show_warning("插件操作正在执行，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        self.pending_plugin_inspection = None;
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client.install_inspected_plugin(inspection_id).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                match result {
                    Ok(plugin) => {
                        let action = if is_update { "更新" } else { "安装" };
                        view.show_success(format!("已{action}插件 {}", plugin.manifest.name), cx);
                        view.load_plugins(cx);
                    }
                    Err(error) => {
                        let action = if is_update { "更新" } else { "安装" };
                        view.show_error(format!("{action}插件失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn set_plugin_enabled(&mut self, plugin_id: String, enabled: bool, cx: &mut Context<Self>) {
        if self.plugin_operation_in_flight {
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client.set_plugin_enabled(plugin_id, enabled).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                match result {
                    Ok(_) => view.load_plugins(cx),
                    Err(error) => view.show_error(format!("更新插件状态失败：{error}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn uninstall_plugin(&mut self, plugin_id: String, cx: &mut Context<Self>) {
        if self.pending_plugin_uninstall.as_deref() != Some(plugin_id.as_str()) {
            self.pending_plugin_uninstall = Some(plugin_id);
            self.show_warning("卸载插件会删除其本地文件，请再次点击确认", cx);
            return;
        }
        if self.plugin_operation_in_flight {
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client.uninstall_plugin(plugin_id).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                match result {
                    Ok(()) => {
                        view.pending_plugin_uninstall = None;
                        view.show_success("插件已卸载", cx);
                        view.load_plugins(cx);
                    }
                    Err(error) => view.show_error(format!("卸载插件失败：{error}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn execute_plugin_command(
        &mut self,
        plugin_id: String,
        command_id: String,
        command_name: String,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_operation_in_flight {
            self.show_warning("插件操作正在执行，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        let requires_write_approval = self.plugins.iter().any(|plugin| {
            plugin.manifest.id == plugin_id
                && plugin_permissions_require_workspace_write(&plugin.manifest.permissions)
        });
        let workspace_id = self.selected_workspace_id.clone();
        let allow_workspace_write = if requires_write_approval {
            if !self.feature_enabled("pluginWorkspaceWrite") {
                self.show_error("当前 Daemon 版本不支持受控插件写入，请更新后重试", cx);
                return;
            }
            let Some(approved_workspace_id) = workspace_id.as_deref() else {
                self.show_error("请先选择允许插件写入的工作区", cx);
                return;
            };
            let write_approval_key =
                plugin_write_approval_key(&plugin_id, &command_id, approved_workspace_id);
            if self.pending_plugin_write.as_deref() != Some(write_approval_key.as_str()) {
                self.pending_plugin_write = Some(write_approval_key);
                self.show_warning(
                    "该插件声明工作区写入权限；仅本次命令有效，再次点击确认运行",
                    cx,
                );
                return;
            }
            self.pending_plugin_write = None;
            true
        } else {
            false
        };
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .execute_plugin_command(plugin_id, command_id, workspace_id, allow_workspace_write)
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                match result {
                    Ok(result) => {
                        let capability_summary =
                            plugin_capability_usage_summary(&result.capability_usage)
                                .map(|summary| format!("（{summary}）"))
                                .unwrap_or_default();
                        view.show_success(
                            format!("{command_name}：{}{capability_summary}", result.message),
                            cx,
                        );
                    }
                    Err(error) => view.show_error(format!("运行插件命令失败：{error}"), cx),
                }
                view.load_plugins(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn install_marketplace_plugin(
        &mut self,
        plugin_id: String,
        version: String,
        is_update: bool,
        permission_escalation: &[corbit_client::PluginPermission],
        cx: &mut Context<Self>,
    ) {
        if is_update
            && !permission_escalation.is_empty()
            && self.pending_plugin_update.as_deref() != Some(plugin_id.as_str())
        {
            let labels = permission_escalation
                .iter()
                .map(plugin_permission_label)
                .collect::<Vec<_>>()
                .join("、");
            self.pending_plugin_update = Some(plugin_id);
            self.show_warning(format!("更新将新增权限：{labels}，请再次点击确认"), cx);
            return;
        }
        if self.plugin_operation_in_flight {
            self.show_warning("插件操作正在执行，请稍候", cx);
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_error("Daemon 尚未连接", cx);
            return;
        };
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .install_marketplace_plugin(plugin_id, Some(version))
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                view.pending_plugin_update = None;
                match result {
                    Ok(plugin) => {
                        let message = if is_update {
                            format!("已更新插件 {}", plugin.manifest.name)
                        } else {
                            format!("已安装已验证插件 {}", plugin.manifest.name)
                        };
                        view.show_success(message, cx);
                        view.load_plugins(cx);
                    }
                    Err(error) => {
                        let operation = if is_update { "更新" } else { "安装" };
                        view.show_error(format!("{operation}市场插件失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_plugin_settings(&self, is_online: bool, cx: &mut Context<Self>) -> Div {
        let supported = self.feature_enabled("plugins");
        let plugin_cards = self
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                installed_plugin_card(
                    index,
                    plugin,
                    is_online,
                    self.plugin_operation_in_flight,
                    self.pending_plugin_uninstall.as_deref(),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let marketplace_cards = self
            .plugin_marketplace
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                plugin_marketplace_row(
                    index,
                    entry,
                    supported,
                    is_online,
                    self.plugin_operation_in_flight,
                    self.pending_plugin_update.as_deref(),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let audit_rows = self
            .plugin_audit
            .iter()
            .map(|entry| {
                let plugin_name = self
                    .plugins
                    .iter()
                    .find(|plugin| plugin.manifest.id == entry.plugin_id)
                    .map_or(entry.plugin_id.as_str(), |plugin| {
                        plugin.manifest.name.as_str()
                    });
                plugin_audit_row(entry, plugin_name)
            })
            .collect::<Vec<_>>();

        settings_page_header("插件", "管理内置插件，并从本机插件目录安装第三方插件。")
            .when(!supported, |page| {
                page.child(
                    settings_card("插件功能不可用").child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("当前 Daemon 版本未提供插件管理能力，请更新并重新连接。"),
                    ),
                )
            })
            .child(
                settings_card("本地插件")
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .line_height(px(19.))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(
                                "插件由 Daemon 管理。第三方插件需要声明权限，并运行在独立进程中。",
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                Button::new("plugin-install-local")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(AppIcon::Add))
                                    .label("安装本地插件")
                                    .loading(self.plugin_operation_in_flight)
                                    .disabled(
                                        !supported || !is_online || self.plugin_operation_in_flight,
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.select_plugin_package(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("plugin-refresh")
                                    .outline()
                                    .small()
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .label("刷新")
                                    .disabled(
                                        !supported || !is_online || self.plugin_operation_in_flight,
                                    )
                                    .on_click(cx.listener(|view, _, _, cx| view.load_plugins(cx))),
                            ),
                    )
                    .when(self.plugins.is_empty(), |card| {
                        card.child(
                            div()
                                .py_2()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child("暂无已安装插件。"),
                        )
                    })
                    .children(plugin_cards),
            )
            .child(
                settings_card("最近执行记录")
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .line_height(px(19.))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("仅显示最近的脱敏记录，不保存命令参数、文件路径或文件内容。"),
                    )
                    .when(self.plugin_audit.is_empty(), |card| {
                        card.child(
                            div()
                                .py_2()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child("暂无插件执行记录。"),
                        )
                    })
                    .children(audit_rows),
            )
            .child(
                settings_card("插件市场")
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("展示 Daemon 内置插件和通过签名索引校验的第三方插件。"),
                    )
                    .when(self.plugin_marketplace.is_empty(), |card| {
                        card.child(
                            div()
                                .py_2()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child("连接 Daemon 后加载插件目录。"),
                        )
                    })
                    .children(marketplace_cards),
            )
    }
}

#[allow(clippy::too_many_lines)]
fn open_plugin_inspection_dialog(
    view: &Entity<ConnectionView>,
    inspection: &corbit_client::PluginInspection,
    window: &mut Window,
    cx: &mut App,
) {
    let inspection_id = inspection.inspection_id.clone();
    let manifest = inspection.manifest.clone();
    let source_kind = match inspection.source_kind {
        corbit_client::PluginSourceKind::Directory => "本地目录",
        corbit_client::PluginSourceKind::Archive => ".corbit-plugin 压缩包",
    };
    let source_kind = source_kind.to_owned();
    let is_update = matches!(
        inspection.operation,
        corbit_client::PluginInspectionOperation::Update
    );
    let operation_label = if is_update { "更新" } else { "安装" };
    let installed_version = inspection.installed_version.clone();
    let permission_escalation = inspection.permission_escalation.clone();
    let unavailable_permissions = inspection.unavailable_permissions.clone();
    let fingerprint = inspection
        .source_fingerprint
        .chars()
        .take(16)
        .collect::<String>();
    let confirm_view = view.clone();
    let confirm_id = inspection_id.clone();
    let cancel_view = view.clone();
    let footer_cancel_view = view.clone();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let manifest = manifest.clone();
        let source_kind = source_kind.clone();
        let fingerprint = fingerprint.clone();
        let installed_version = installed_version.clone();
        let permission_escalation = permission_escalation.clone();
        let unavailable_permissions = unavailable_permissions.clone();
        let permission_badges = manifest
            .permissions
            .iter()
            .map(|permission| {
                div()
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                    .px_2()
                    .py_1()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(plugin_permission_label(permission))
            })
            .collect::<Vec<_>>();
        let command_rows = manifest
            .commands
            .iter()
            .map(|command| {
                div()
                    .v_flex()
                    .gap_1()
                    .child(div().font_medium().child(command.name.clone()))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(
                                command
                                    .description
                                    .clone()
                                    .unwrap_or_else(|| command.id.clone()),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        let permission_content = if permission_badges.is_empty() {
            div()
                .text_size(font_px(FONT_SIZE_SM))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child("无额外权限")
        } else {
            div().h_flex().flex_wrap().gap_1().children(permission_badges)
        };
        let escalation_labels = permission_escalation
            .iter()
            .map(plugin_permission_label)
            .collect::<Vec<_>>();
        let escalation_content = escalation_labels.join("、");
        let unavailable_labels = unavailable_permission_labels(&unavailable_permissions);
        let unavailable_content = unavailable_labels.join("、");
        let command_content: AnyElement = if command_rows.is_empty() {
            div()
                .text_size(font_px(FONT_SIZE_SM))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child("未声明命令")
                .into_any_element()
        } else {
            div()
                .v_flex()
                .gap_2()
                .max_h(px(240.))
                .overflow_y_scrollbar()
                .children(command_rows)
                .into_any_element()
        };
        let confirm_view = confirm_view.clone();
        let confirm_id = confirm_id.clone();
        let cancel_view = cancel_view.clone();
        let footer_cancel_view = footer_cancel_view.clone();
        dialog
            .title(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(format!("确认{operation_label}本地插件"))
                    .child(
                        Button::new("plugin-inspection-close")
                            .ghost()
                            .small()
                            .icon(Icon::new(AppIcon::Close))
                            .tooltip("关闭")
                            .on_click(move |_, window, cx| {
                                cancel_view.update(cx, |view, cx| {
                                    view.pending_plugin_inspection = None;
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
            .w(px(620.))
            .max_w(px(700.))
            .bg(rgb(COLOR_EDITOR))
            .close_button(false)
            .overlay_closable(false)
            .keyboard(false)
            .footer(move |_, _, _, _| {
                let confirm_view = confirm_view.clone();
                let confirm_id = confirm_id.clone();
                let cancel_view = footer_cancel_view.clone();
                vec![
                    Button::new("plugin-inspection-cancel")
                        .ghost()
                        .label("取消")
                        .on_click(move |_, window, cx| {
                            cancel_view.update(cx, |view, cx| {
                                view.pending_plugin_inspection = None;
                                cx.notify();
                            });
                            window.close_dialog(cx);
                        }),
                    Button::new("plugin-inspection-confirm")
                        .primary()
                        .label(format!("确认{operation_label}"))
                        .on_click(move |_, window, cx| {
                            confirm_view.update(cx, |view, cx| {
                                view.install_inspected_plugin(confirm_id.clone(), is_update, cx);
                            });
                            window.close_dialog(cx);
                        }),
                ]
            })
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_HEADING))
                            .font_medium()
                            .child(manifest.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(manifest.description.clone()),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(format!(
                                "ID：{} · 发布者：{} · 版本：v{}",
                                manifest.id, manifest.publisher, manifest.version
                            )),
                    )
                    .when_some(installed_version, |content, version| {
                        content.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(format!(
                                    "当前已安装版本：v{version} → 目标版本：v{}",
                                    manifest.version
                                )),
                        )
                    })
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(format!(
                                "来源：{source_kind} · 运行时：独立进程 · 指纹：{fingerprint}…"
                            )),
                    )
                    .child(div().font_medium().child("请求权限"))
                    .child(permission_content)
                    .when(!unavailable_content.is_empty(), |content| {
                        content.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_WARNING))
                                .child(format!("当前 Daemon 未提供这些权限对应的能力宿主：{unavailable_content}")),
                        )
                    })
                    .when(!escalation_content.is_empty(), |content| {
                        content.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_WARNING))
                                .child(format!(
                                    "本次{operation_label}新增权限：{escalation_content}"
                                )),
                        )
                    })
                    .child(div().font_medium().child("可用命令"))
                    .child(command_content)
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child("第三方插件在独立进程运行，但不是完整的操作系统沙箱。请只安装信任来源的插件。"),
                    ),
            )
    });
}

fn plugin_audit_is_unsupported(error: &corbit_client::ClientError) -> bool {
    matches!(
        error,
        corbit_client::ClientError::Rpc(body)
            if matches!(body.code.as_str(), "method_not_found" | "root_credential_required")
    )
}

fn plugin_audit_row(entry: &corbit_client::PluginAuditEntry, plugin_name: &str) -> Div {
    let (status_label, status_color) = match entry.status {
        corbit_client::PluginAuditStatus::Succeeded => ("成功", COLOR_SUCCESS),
        corbit_client::PluginAuditStatus::Failed => ("失败", COLOR_ERROR),
    };
    let plugin_label = if plugin_name == entry.plugin_id {
        entry.plugin_id.clone()
    } else {
        format!("{plugin_name} · {}", entry.plugin_id)
    };
    let capability_summary = plugin_capability_usage_summary(&entry.capability_usage);

    div()
        .v_flex()
        .gap_1()
        .rounded_lg()
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .px_3()
        .py_2()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(font_px(FONT_SIZE_SM))
                        .font_medium()
                        .child(plugin_label),
                )
                .child(
                    div()
                        .flex_none()
                        .rounded_full()
                        .px_2()
                        .py_1()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(status_color))
                        .child(status_label),
                ),
        )
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(format!(
                    "命令 {} · {}",
                    entry.command_id,
                    plugin_audit_completed_at(&entry.completed_at)
                )),
        )
        .when_some(capability_summary, |row, summary| {
            row.child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                    .child(summary),
            )
        })
        .when_some(entry.error_code.clone(), |row, error_code| {
            row.child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_ERROR))
                    .child(format!("错误码：{error_code}")),
            )
        })
}

fn plugin_audit_completed_at(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value).map_or_else(
        |_| value.to_string(),
        |date_time| {
            date_time
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        },
    )
}

#[allow(clippy::too_many_lines)]
fn installed_plugin_card(
    index: usize,
    plugin: &corbit_client::PluginRecord,
    is_online: bool,
    operation_in_flight: bool,
    pending_uninstall: Option<&str>,
    cx: &mut Context<ConnectionView>,
) -> Div {
    let toggle_view = cx.entity();
    let uninstall_view = toggle_view.clone();
    let plugin_id = plugin.manifest.id.clone();
    let uninstall_id = plugin_id.clone();
    let enabled = plugin.enabled;
    let action_disabled = !is_online || operation_in_flight;
    let unavailable_summary = plugin
        .unavailable_permissions
        .iter()
        .map(plugin_permission_label)
        .collect::<Vec<_>>()
        .join("、");
    let permission_badges = plugin
        .manifest
        .permissions
        .iter()
        .map(|permission| {
            div()
                .flex_none()
                .rounded_full()
                .bg(rgb(COLOR_SURFACE_SECONDARY))
                .px_2()
                .py_1()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(plugin_permission_label(permission))
        })
        .collect::<Vec<_>>();
    let command_buttons = plugin
        .manifest
        .commands
        .iter()
        .enumerate()
        .map(|(command_index, command)| {
            let command_view = cx.entity();
            let plugin_id = plugin.manifest.id.clone();
            let command_id = command.id.clone();
            let command_name = command.name.clone();
            Button::new(("plugin-command", index * 64 + command_index))
                .outline()
                .small()
                .icon(Icon::new(AppIcon::Play))
                .label(command.name.clone())
                .disabled(action_disabled || !enabled)
                .on_click(cx.listener(move |_, _, _, cx| {
                    let plugin_id = plugin_id.clone();
                    let command_id = command_id.clone();
                    let command_name = command_name.clone();
                    command_view.update(cx, |view, cx| {
                        view.execute_plugin_command(plugin_id, command_id, command_name, cx);
                    });
                }))
        })
        .collect::<Vec<_>>();

    div()
        .h_flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
        .p_4()
        .child(
            Icon::new(AppIcon::Tool)
                .size(px(22.))
                .text_color(rgb(COLOR_TEXT_SECONDARY)),
        )
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().font_medium().child(plugin.manifest.name.clone()))
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(plugin.manifest.description.clone()),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(format!(
                            "{} · v{} · {}",
                            plugin.manifest.publisher,
                            plugin.manifest.version,
                            if plugin.source == "builtin" {
                                "内置"
                            } else {
                                "本地安装"
                            }
                        )),
                )
                .when(!plugin.manifest.permissions.is_empty(), |details| {
                    details.child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .gap_1()
                            .children(permission_badges),
                    )
                })
                .when(!unavailable_summary.is_empty(), |details| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child(format!("能力宿主未提供：{unavailable_summary}")),
                    )
                }),
        )
        .children(command_buttons)
        .child(
            Button::new(("plugin-toggle", index))
                .outline()
                .small()
                .label(if enabled { "禁用" } else { "启用" })
                .disabled(action_disabled)
                .on_click(cx.listener(move |_, _, _, cx| {
                    let plugin_id = plugin_id.clone();
                    toggle_view.update(cx, |view, cx| {
                        view.set_plugin_enabled(plugin_id, !enabled, cx);
                    });
                })),
        )
        .when(plugin.source != "builtin", |card| {
            card.child(
                Button::new(("plugin-uninstall", index))
                    .danger()
                    .small()
                    .label(if pending_uninstall == Some(uninstall_id.as_str()) {
                        "确认卸载"
                    } else {
                        "卸载"
                    })
                    .disabled(action_disabled)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let plugin_id = uninstall_id.clone();
                        uninstall_view.update(cx, |view, cx| {
                            view.uninstall_plugin(plugin_id, cx);
                        });
                    })),
            )
        })
}

#[allow(clippy::too_many_lines)]
fn plugin_marketplace_row(
    index: usize,
    entry: &corbit_client::PluginMarketplaceEntry,
    supported: bool,
    is_online: bool,
    operation_in_flight: bool,
    pending_update: Option<&str>,
    cx: &mut Context<ConnectionView>,
) -> Div {
    let install_view = cx.entity();
    let plugin_id = entry.manifest.id.clone();
    let version = entry.manifest.version.clone();
    let is_update = entry.update_available;
    let permission_escalation = entry.permission_escalation.clone();
    let permission_summary = entry
        .manifest
        .permissions
        .iter()
        .map(plugin_permission_label)
        .collect::<Vec<_>>()
        .join("、");
    let escalation_summary = entry
        .permission_escalation
        .iter()
        .map(plugin_permission_label)
        .collect::<Vec<_>>()
        .join("、");
    let unavailable_summary = entry
        .unavailable_permissions
        .iter()
        .map(plugin_permission_label)
        .collect::<Vec<_>>()
        .join("、");
    let update_confirmation_pending =
        pending_update == Some(entry.manifest.id.as_str()) && !permission_escalation.is_empty();
    let availability_label = if entry.update_available {
        format!(
            "{} → {}",
            entry.installed_version.as_deref().unwrap_or("未知版本"),
            entry.manifest.version
        )
    } else if let Some(version) = entry.installed_version.as_deref() {
        format!("已安装 {version}")
    } else if entry.verified {
        format!("已验证 {}", entry.manifest.version)
    } else {
        "不可用".to_owned()
    };
    let can_install = supported
        && is_online
        && !operation_in_flight
        && entry.verified
        && (!entry.installed || entry.update_available)
        && entry.package_url.is_some();
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .py_2()
        .child(
            Icon::new(AppIcon::Tool)
                .size(px(16.))
                .text_color(rgb(COLOR_TEXT_TERTIARY)),
        )
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(entry.manifest.name.clone())
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(format!("目标版本 v{}", entry.manifest.version)),
                )
                .when(!permission_summary.is_empty(), |details| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(format!("权限：{permission_summary}")),
                    )
                })
                .when(!escalation_summary.is_empty(), |details| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child(format!("更新新增权限：{escalation_summary}")),
                    )
                })
                .when(!unavailable_summary.is_empty(), |details| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child(format!("当前 Daemon 未提供能力宿主：{unavailable_summary}")),
                    )
                }),
        )
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child(availability_label),
        )
        .when(can_install, |row| {
            row.child(
                Button::new(("plugin-marketplace-install", index))
                    .primary()
                    .small()
                    .icon(Icon::new(if entry.update_available {
                        AppIcon::Refresh
                    } else {
                        AppIcon::Add
                    }))
                    .label(if update_confirmation_pending {
                        "确认更新"
                    } else if entry.update_available {
                        "更新"
                    } else {
                        "安装"
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let plugin_id = plugin_id.clone();
                        let version = version.clone();
                        let permission_escalation = permission_escalation.clone();
                        install_view.update(cx, |view, cx| {
                            view.install_marketplace_plugin(
                                plugin_id,
                                version,
                                is_update,
                                &permission_escalation,
                                cx,
                            );
                        });
                    })),
            )
        })
}

const fn plugin_permission_label(permission: &corbit_client::PluginPermission) -> &'static str {
    match permission {
        corbit_client::PluginPermission::WorkspaceRead => "工作区只读",
        corbit_client::PluginPermission::WorkspaceWrite => "工作区写入",
        corbit_client::PluginPermission::Network => "网络访问",
        corbit_client::PluginPermission::Process => "进程执行",
        corbit_client::PluginPermission::Secrets => "敏感信息",
    }
}

fn unavailable_permission_labels(
    permissions: &[corbit_client::PluginPermission],
) -> Vec<&'static str> {
    permissions.iter().map(plugin_permission_label).collect()
}

fn plugin_permissions_require_workspace_write(
    permissions: &[corbit_client::PluginPermission],
) -> bool {
    permissions.contains(&corbit_client::PluginPermission::WorkspaceWrite)
}

fn plugin_write_approval_key(plugin_id: &str, command_id: &str, workspace_id: &str) -> String {
    format!("{plugin_id}:{command_id}:{workspace_id}")
}

fn plugin_capability_label(capability: &str) -> &str {
    match capability {
        "workspace.list" => "目录列表",
        "workspace.read" => "文件读取",
        "workspace.write" => "文件写入",
        "workspace.git.status" => "Git 状态",
        "workspace.git.diff" => "Git 差异",
        _ => capability,
    }
}

fn plugin_capability_usage_summary(
    usage: &[corbit_client::PluginCapabilityUsage],
) -> Option<String> {
    if usage.is_empty() {
        return None;
    }
    let mut parts = usage
        .iter()
        .take(3)
        .map(|item| {
            let failure = if item.failure_count == 0 {
                String::new()
            } else {
                format!("，失败 {} 次", item.failure_count)
            };
            format!(
                "{} {} 次{failure}",
                plugin_capability_label(&item.capability),
                item.request_count
            )
        })
        .collect::<Vec<_>>();
    if usage.len() > 3 {
        parts.push(format!("另 {} 项", usage.len() - 3));
    }
    Some(format!("能力调用：{}", parts.join("；")))
}

#[cfg(test)]
mod tests {
    use super::{
        plugin_capability_usage_summary, plugin_permission_label,
        plugin_permissions_require_workspace_write, plugin_write_approval_key,
        unavailable_permission_labels,
    };
    use corbit_client::{PluginCapabilityUsage, PluginPermission};

    #[test]
    fn plugin_permissions_have_explicit_user_facing_labels() {
        assert_eq!(
            [
                PluginPermission::WorkspaceRead,
                PluginPermission::WorkspaceWrite,
                PluginPermission::Network,
                PluginPermission::Process,
                PluginPermission::Secrets,
            ]
            .iter()
            .map(plugin_permission_label)
            .collect::<Vec<_>>(),
            [
                "工作区只读",
                "工作区写入",
                "网络访问",
                "进程执行",
                "敏感信息"
            ]
        );
    }

    #[test]
    fn plugin_unavailable_permissions_follow_the_daemon_response() {
        assert_eq!(
            unavailable_permission_labels(&[PluginPermission::Network, PluginPermission::Secrets,]),
            vec!["网络访问", "敏感信息"]
        );
    }

    #[test]
    fn plugin_workspace_write_requires_an_explicit_command_approval() {
        assert!(plugin_permissions_require_workspace_write(&[
            PluginPermission::WorkspaceRead,
            PluginPermission::WorkspaceWrite,
        ]));
        assert!(!plugin_permissions_require_workspace_write(&[
            PluginPermission::WorkspaceRead,
            PluginPermission::Network,
        ]));
    }

    #[test]
    fn plugin_workspace_write_approval_is_bound_to_the_selected_workspace() {
        let main = plugin_write_approval_key("com.example.writer", "writer.run", "workspace_main");
        let docs = plugin_write_approval_key("com.example.writer", "writer.run", "workspace_docs");

        assert_ne!(main, docs);
        assert_eq!(
            main,
            plugin_write_approval_key("com.example.writer", "writer.run", "workspace_main")
        );
    }

    #[test]
    fn capability_summary_exposes_counts_without_request_parameters() {
        let summary = plugin_capability_usage_summary(&[
            PluginCapabilityUsage {
                capability: "workspace.read".into(),
                request_count: 2,
                success_count: 1,
                failure_count: 1,
            },
            PluginCapabilityUsage {
                capability: "workspace.git.status".into(),
                request_count: 1,
                success_count: 1,
                failure_count: 0,
            },
        ]);

        assert_eq!(
            summary.as_deref(),
            Some("能力调用：文件读取 2 次，失败 1 次；Git 状态 1 次")
        );
    }
}
