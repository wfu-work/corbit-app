use super::settings::settings_page_header;
use super::*;

impl ConnectionView {
    pub(super) fn load_codex_official_plugins(
        &mut self,
        force_refetch: bool,
        cx: &mut Context<Self>,
    ) {
        if self.codex_official_plugin_task.is_some()
            || !matches!(self.state, corbit_client::ConnectionState::Online)
            || !self.feature_enabled("officialPlugins")
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
        self.codex_official_plugin_operation_in_flight = true;
        self.codex_official_plugin_error = None;
        self.codex_official_plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client.codex_official_plugins(force_refetch).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.codex_official_plugin_task = None;
                view.codex_official_plugin_operation_in_flight = false;
                match result {
                    Ok(catalog) => {
                        view.codex_official_plugin_catalog = Some(catalog);
                        view.codex_official_plugin_error = None;
                        view.pending_codex_official_plugin_install = None;
                        view.pending_codex_official_plugin_uninstall = None;
                        if force_refetch {
                            view.codex_official_apps_needing_auth.clear();
                        }
                    }
                    Err(error) => {
                        view.codex_official_plugin_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

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
                        view.pending_plugin_uninstall = None;
                        view.pending_plugin_update = None;
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

    fn install_codex_official_plugin(
        &mut self,
        plugin_id: String,
        marketplace_name: String,
        plugin_name: String,
        requires_confirmation: bool,
        cx: &mut Context<Self>,
    ) {
        if requires_confirmation
            && self.pending_codex_official_plugin_install.as_deref() != Some(plugin_id.as_str())
        {
            self.pending_codex_official_plugin_install = Some(plugin_id);
            self.show_warning(
                "该 Codex 插件要求安装前确认，可能会连接外部服务；请再次点击安装",
                cx,
            );
            return;
        }
        if self.codex_official_plugin_operation_in_flight {
            self.show_warning("Codex 官方插件操作正在执行，请稍候", cx);
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
        self.codex_official_plugin_operation_in_flight = true;
        self.codex_official_plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .install_codex_official_plugin(marketplace_name, plugin_name)
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.codex_official_plugin_task = None;
                view.codex_official_plugin_operation_in_flight = false;
                view.pending_codex_official_plugin_install = None;
                match result {
                    Ok(result) => {
                        view.codex_official_apps_needing_auth = result.apps_needing_auth;
                        if view.codex_official_apps_needing_auth.is_empty() {
                            view.show_success("Codex 官方插件已安装", cx);
                        } else {
                            view.show_success("Codex 官方插件已安装，请继续连接所需账号", cx);
                        }
                        view.load_codex_official_plugins(false, cx);
                    }
                    Err(error) => {
                        view.show_error(format!("安装 Codex 官方插件失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn uninstall_codex_official_plugin(&mut self, plugin_id: String, cx: &mut Context<Self>) {
        if self.pending_codex_official_plugin_uninstall.as_deref() != Some(plugin_id.as_str()) {
            self.pending_codex_official_plugin_uninstall = Some(plugin_id);
            self.show_warning("卸载后 Codex 将不再加载此官方插件，请再次点击确认", cx);
            return;
        }
        if self.codex_official_plugin_operation_in_flight {
            self.show_warning("Codex 官方插件操作正在执行，请稍候", cx);
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
        self.codex_official_plugin_operation_in_flight = true;
        self.codex_official_plugin_task = Some(cx.spawn(async move |view, cx| {
            let result = client.uninstall_codex_official_plugin(plugin_id).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.codex_official_plugin_task = None;
                view.codex_official_plugin_operation_in_flight = false;
                view.pending_codex_official_plugin_uninstall = None;
                match result {
                    Ok(()) => {
                        view.codex_official_apps_needing_auth.clear();
                        view.show_success("Codex 官方插件已卸载", cx);
                        view.load_codex_official_plugins(false, cx);
                    }
                    Err(error) => {
                        view.show_error(format!("卸载 Codex 官方插件失败：{error}"), cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_plugin_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择包含 .codex-plugin/plugin.json 的插件目录".into()),
        });
        let window_handle = window.window_handle();
        self.plugin_operation_in_flight = true;
        self.plugin_task = Some(cx.spawn(async move |view, cx| {
            let path = match path_prompt.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    if let Some(view) = view.upgrade() {
                        let _ = view.update(cx, |view, cx| {
                            view.plugin_operation_in_flight = false;
                            view.plugin_task = None;
                            view.show_error(format!("无法打开插件目录选择器：{error}"), cx);
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
                            view.show_error("插件目录选择器意外关闭，请重试", cx);
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
            match client
                .inspect_plugin(path.to_string_lossy().into_owned())
                .await
            {
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
                        view.show_error(format!("Codex 插件预检失败：{error}"), cx);
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
                        view.show_success(
                            format!("已{action}插件 {}", plugin_display_name(&plugin.manifest)),
                            cx,
                        );
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
            self.show_warning("卸载插件会删除其本地副本和插件数据，请再次点击确认", cx);
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

    fn install_marketplace_plugin(
        &mut self,
        plugin_id: String,
        version: Option<String>,
        is_update: bool,
        cx: &mut Context<Self>,
    ) {
        if is_update && self.pending_plugin_update.as_deref() != Some(plugin_id.as_str()) {
            self.pending_plugin_update = Some(plugin_id);
            self.show_warning(
                "市场插件将从声明的来源重新获取并替换当前版本，请再次点击确认",
                cx,
            );
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
            let result = client.install_marketplace_plugin(plugin_id, version).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.plugin_task = None;
                view.plugin_operation_in_flight = false;
                view.pending_plugin_update = None;
                match result {
                    Ok(plugin) => {
                        let action = if is_update { "更新" } else { "安装" };
                        view.show_success(
                            format!("已{action}插件 {}", plugin_display_name(&plugin.manifest)),
                            cx,
                        );
                        view.load_plugins(cx);
                    }
                    Err(error) => {
                        let action = if is_update { "更新" } else { "安装" };
                        view.show_error(format!("{action}市场插件失败：{error}"), cx);
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
        let official_supported = self.feature_enabled("officialPlugins");
        let official_query = self
            .codex_official_plugin_search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let featured_ids = self
            .codex_official_plugin_catalog
            .as_ref()
            .map(|catalog| {
                catalog
                    .featured_plugin_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let matching_official_plugins =
            self.codex_official_plugin_catalog
                .as_ref()
                .map_or(0, |catalog| {
                    catalog
                        .marketplaces
                        .iter()
                        .flat_map(|marketplace| marketplace.plugins.iter())
                        .filter(|plugin| official_plugin_matches(plugin, &official_query))
                        .count()
                });
        let mut official_installed_rows = Vec::new();
        let mut official_featured_rows = Vec::new();
        let mut official_marketplace_sections = Vec::new();
        let mut official_row_index = 0_usize;
        if let Some(catalog) = self.codex_official_plugin_catalog.as_ref() {
            for marketplace in &catalog.marketplaces {
                let mut marketplace_rows = Vec::new();
                for plugin in &marketplace.plugins {
                    if !official_plugin_matches(plugin, &official_query) {
                        continue;
                    }
                    let row = official_plugin_row(
                        official_row_index,
                        &marketplace.name,
                        plugin,
                        is_online,
                        self.codex_official_plugin_operation_in_flight,
                        self.pending_codex_official_plugin_install.as_deref(),
                        self.pending_codex_official_plugin_uninstall.as_deref(),
                        cx,
                    );
                    official_row_index += 1;
                    if plugin.installed {
                        official_installed_rows.push(row);
                    } else if featured_ids.contains(plugin.id.as_str()) {
                        official_featured_rows.push(row);
                    } else {
                        marketplace_rows.push(row);
                    }
                }
                if !marketplace_rows.is_empty() {
                    official_marketplace_sections.push(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .pt_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_semibold()
                                    .child(marketplace.display_name.clone()),
                            )
                            .children(marketplace_rows),
                    );
                }
            }
        }
        let official_auth_rows = self
            .codex_official_apps_needing_auth
            .iter()
            .enumerate()
            .map(|(index, app)| official_auth_app_row(index, app, cx))
            .collect::<Vec<_>>();
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

        settings_page_header(
            "插件",
            "浏览 Codex 托管插件，或安装可跨 Codex、Claude 和 ACP 使用的本地 Codex 插件。",
        )
        .child(
            settings_card("Codex 官方插件（实验性）")
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .line_height(px(19.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(
                            "目录、安装和账号授权由本机 Codex App Server 管理；连接器仅保证在 Codex Provider 中完整可用。",
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(settings_input(&self.codex_official_plugin_search)),
                        )
                        .child(
                            settings_action_button("codex-official-plugin-refresh", cx)
                                .icon(Icon::new(AppIcon::Refresh))
                                .label("刷新")
                                .loading(self.codex_official_plugin_operation_in_flight)
                                .disabled(
                                    !official_supported
                                        || !is_online
                                        || self.codex_official_plugin_operation_in_flight,
                                )
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.load_codex_official_plugins(true, cx);
                                })),
                        ),
                )
                .when(!official_supported, |card| {
                    card.child(
                        div()
                            .rounded_lg()
                            .bg(rgb(COLOR_SURFACE_SECONDARY))
                            .p_3()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(
                                "当前 Daemon 或 Codex 版本未提供官方插件目录；本地插件仍可正常管理。",
                            ),
                    )
                })
                .when_some(self.codex_official_plugin_error.clone(), |card, error| {
                    card.child(
                        div()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(COLOR_WARNING))
                            .p_3()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_WARNING))
                            .child(format!("Codex 官方目录暂时不可用：{error}")),
                    )
                })
                .when(!official_auth_rows.is_empty(), |card| {
                    card.child(
                        div()
                            .v_flex()
                            .gap_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(COLOR_BORDER))
                            .bg(rgb(COLOR_SURFACE_SECONDARY))
                            .p_3()
                            .child(div().font_semibold().child("连接账号以完成设置"))
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(
                                        "授权页面由 Codex 提供，Corbit 不读取或保存 Gmail、Slack 等服务的 OAuth 凭据。",
                                    ),
                            )
                            .children(official_auth_rows),
                    )
                })
                .when(
                    official_supported
                        && self.codex_official_plugin_catalog.is_none()
                        && self.codex_official_plugin_error.is_none(),
                    |card| {
                        card.child(
                            div()
                                .py_2()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(if self.codex_official_plugin_operation_in_flight {
                                    "正在读取 Codex 官方目录…"
                                } else {
                                    "尚未读取 Codex 官方目录。"
                                }),
                        )
                    },
                )
                .when(!official_installed_rows.is_empty(), |card| {
                    card.child(official_plugin_section("已安装", official_installed_rows))
                })
                .when(!official_featured_rows.is_empty(), |card| {
                    card.child(official_plugin_section("精选", official_featured_rows))
                })
                .children(official_marketplace_sections)
                .when(
                    self.codex_official_plugin_catalog.is_some()
                        && matching_official_plugins == 0,
                    |card| {
                        card.child(
                            div()
                                .py_3()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(if official_query.is_empty() {
                                    "Codex 当前未返回可展示的官方插件。"
                                } else {
                                    "没有匹配的 Codex 官方插件。"
                                }),
                        )
                    },
                )
                .when_some(
                    self.codex_official_plugin_catalog.as_ref().and_then(|catalog| {
                        (!catalog.marketplace_load_errors.is_empty())
                            .then(|| catalog.marketplace_load_errors.join("；"))
                    }),
                    |card, errors| {
                        card.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_WARNING))
                                .child(format!("部分市场加载失败：{errors}")),
                        )
                    },
                ),
        )
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
            settings_card("Corbit 本地插件")
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .line_height(px(19.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(
                            "只接受包含 .codex-plugin/plugin.json 的目录；旧 Corbit 清单和压缩包不再支持。",
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .child(
                            settings_primary_action_button("plugin-install-local", cx)
                                .icon(Icon::new(AppIcon::Add))
                                .label("安装 Codex 插件目录")
                                .loading(self.plugin_operation_in_flight)
                                .disabled(
                                    !supported || !is_online || self.plugin_operation_in_flight,
                                )
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.select_plugin_directory(window, cx);
                                })),
                        )
                        .child(
                            settings_action_button("plugin-refresh", cx)
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
            settings_card("Corbit 插件市场")
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .line_height(px(19.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(
                            "读取官方 marketplace.json 结构，支持本地、Git 子目录和 NPM 来源。",
                        ),
                )
                .when(self.plugin_marketplace.is_empty(), |card| {
                    card.child(
                        div()
                            .py_2()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child("未配置插件市场，或市场当前没有条目。"),
                    )
                })
                .children(marketplace_cards),
        )
    }
}

fn official_plugin_section(title: &'static str, rows: Vec<Div>) -> Div {
    div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .pt_2()
                .text_size(font_px(FONT_SIZE_SM))
                .font_semibold()
                .child(title),
        )
        .children(rows)
}

fn official_plugin_matches(
    plugin: &corbit_client::CodexOfficialPluginSummary,
    query: &str,
) -> bool {
    if query.is_empty() {
        return true;
    }
    let interface = plugin.interface.as_ref();
    [
        Some(plugin.id.as_str()),
        Some(plugin.name.as_str()),
        interface.and_then(|value| value.display_name.as_deref()),
        interface.and_then(|value| value.short_description.as_deref()),
        interface.and_then(|value| value.long_description.as_deref()),
        interface.and_then(|value| value.developer_name.as_deref()),
        interface.and_then(|value| value.category.as_deref()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(query))
        || plugin
            .keywords
            .iter()
            .any(|keyword| keyword.to_lowercase().contains(query))
}

fn official_plugin_display_name(plugin: &corbit_client::CodexOfficialPluginSummary) -> String {
    plugin
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.clone())
        .unwrap_or_else(|| plugin.name.clone())
}

fn official_plugin_description(plugin: &corbit_client::CodexOfficialPluginSummary) -> String {
    plugin
        .interface
        .as_ref()
        .and_then(|interface| {
            interface
                .short_description
                .clone()
                .or_else(|| interface.long_description.clone())
        })
        .unwrap_or_else(|| "Codex 官方托管插件".into())
}

fn official_plugin_logo(plugin: &corbit_client::CodexOfficialPluginSummary) -> AnyElement {
    let interface = plugin.interface.as_ref();
    let logo_url = if is_dark_mode() {
        interface.and_then(|value| {
            value
                .logo_url_dark
                .as_ref()
                .or(value.logo_url.as_ref())
                .cloned()
        })
    } else {
        interface.and_then(|value| value.logo_url.clone())
    };
    logo_url.map_or_else(
        || {
            div()
                .flex()
                .size(px(38.))
                .flex_none()
                .items_center()
                .justify_center()
                .rounded_lg()
                .border_1()
                .border_color(rgb(COLOR_BORDER))
                .bg(rgb(COLOR_SURFACE_SECONDARY))
                .child(
                    Icon::new(AppIcon::Tool)
                        .size(px(19.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY)),
                )
                .into_any_element()
        },
        |url| {
            div()
                .size(px(38.))
                .flex_none()
                .rounded_lg()
                .border_1()
                .border_color(rgb(COLOR_BORDER))
                .overflow_hidden()
                .child(img(SharedString::from(url)).size_full())
                .into_any_element()
        },
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn official_plugin_row(
    index: usize,
    marketplace_name: &str,
    plugin: &corbit_client::CodexOfficialPluginSummary,
    is_online: bool,
    operation_in_flight: bool,
    pending_install: Option<&str>,
    pending_uninstall: Option<&str>,
    cx: &mut Context<ConnectionView>,
) -> Div {
    let install_view = cx.entity();
    let uninstall_view = install_view.clone();
    let plugin_id = plugin.id.clone();
    let install_plugin_id = plugin_id.clone();
    let uninstall_plugin_id = plugin_id.clone();
    let marketplace_name = marketplace_name.to_owned();
    let plugin_name = plugin.name.clone();
    let requires_confirmation = plugin.must_show_installation_interstitial.unwrap_or(false);
    let admin_disabled = plugin.availability == "DISABLED_BY_ADMIN";
    let installed_by_default = plugin.install_policy == "INSTALLED_BY_DEFAULT";
    let install_allowed = is_online
        && !operation_in_flight
        && !admin_disabled
        && plugin.install_policy == "AVAILABLE";
    let uninstall_allowed = is_online
        && !operation_in_flight
        && plugin.installed
        && !admin_disabled
        && !installed_by_default;
    let install_confirmation_pending = pending_install == Some(plugin.id.as_str());
    let uninstall_confirmation_pending = pending_uninstall == Some(plugin.id.as_str());
    let metadata = plugin.interface.as_ref().map_or_else(
        || plugin.source_type.clone(),
        |interface| {
            [
                interface.category.clone(),
                interface.developer_name.clone(),
                plugin.version.as_ref().map(|version| format!("v{version}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
        },
    );

    div()
        .h_flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
        .p_3()
        .child(official_plugin_logo(plugin))
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .font_medium()
                                .child(official_plugin_display_name(plugin)),
                        )
                        .when(admin_disabled, |title| {
                            title.child(official_plugin_status_badge("管理员已禁用", COLOR_WARNING))
                        })
                        .when(plugin.installed && !admin_disabled, |title| {
                            title.child(official_plugin_status_badge(
                                if plugin.enabled {
                                    "已安装"
                                } else {
                                    "已停用"
                                },
                                if plugin.enabled {
                                    COLOR_SUCCESS
                                } else {
                                    COLOR_TEXT_TERTIARY
                                },
                            ))
                        }),
                )
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_SM))
                        .line_height(px(18.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(official_plugin_description(plugin)),
                )
                .when(!metadata.is_empty(), |details| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(metadata),
                    )
                }),
        )
        .when(!plugin.installed, |row| {
            row.child(
                settings_primary_action_button(("codex-official-install", index), cx)
                    .icon(Icon::new(AppIcon::Add))
                    .label(if admin_disabled {
                        "不可安装"
                    } else if plugin.install_policy == "NOT_AVAILABLE" {
                        "不可用"
                    } else if install_confirmation_pending {
                        "确认安装"
                    } else {
                        "安装"
                    })
                    .disabled(!install_allowed)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let plugin_id = install_plugin_id.clone();
                        let marketplace_name = marketplace_name.clone();
                        let plugin_name = plugin_name.clone();
                        install_view.update(cx, |view, cx| {
                            view.install_codex_official_plugin(
                                plugin_id,
                                marketplace_name,
                                plugin_name,
                                requires_confirmation,
                                cx,
                            );
                        });
                    })),
            )
        })
        .when(plugin.installed && uninstall_allowed, |row| {
            row.child(
                settings_danger_action_button(("codex-official-uninstall", index), cx)
                    .label(if uninstall_confirmation_pending {
                        "确认卸载"
                    } else {
                        "卸载"
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let plugin_id = uninstall_plugin_id.clone();
                        uninstall_view.update(cx, |view, cx| {
                            view.uninstall_codex_official_plugin(plugin_id, cx);
                        });
                    })),
            )
        })
        .when(
            plugin.installed && !uninstall_allowed && installed_by_default,
            |row| {
                row.child(official_plugin_status_badge(
                    "默认安装",
                    COLOR_TEXT_TERTIARY,
                ))
            },
        )
}

fn official_plugin_status_badge(label: &'static str, color: u32) -> Div {
    div()
        .flex_none()
        .rounded_full()
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .px_2()
        .py_1()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(color))
        .child(label)
}

fn official_auth_app_row(
    index: usize,
    app: &corbit_client::CodexOfficialPluginApp,
    cx: &mut Context<ConnectionView>,
) -> Div {
    let install_url = app.install_url.clone();
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            Icon::new(AppIcon::User)
                .size(px(16.))
                .text_color(rgb(COLOR_TEXT_SECONDARY)),
        )
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .child(app.name.clone())
                .when_some(app.description.clone(), |details, description| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(description),
                    )
                }),
        )
        .child(
            settings_primary_action_button(("codex-official-auth", index), cx)
                .icon(Icon::new(AppIcon::ExternalLink))
                .label(if install_url.is_some() {
                    "连接账号"
                } else {
                    "等待 Codex"
                })
                .disabled(install_url.is_none())
                .when_some(install_url, |button, url| {
                    button.on_click(move |_, _, cx| cx.open_url(&url))
                }),
        )
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
    let components = inspection.components.clone();
    let compatibility = inspection.provider_compatibility.clone();
    let is_update = matches!(
        inspection.operation,
        corbit_client::PluginInspectionOperation::Update
    );
    let operation_label = if is_update { "更新" } else { "安装" };
    let installed_version = inspection.installed_version.clone();
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
        let components = components.clone();
        let compatibility = compatibility.clone();
        let fingerprint = fingerprint.clone();
        let installed_version = installed_version.clone();
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
                    .child(format!("确认{operation_label} Codex 插件"))
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
                            .child(plugin_display_name(&manifest)),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child(plugin_description(&manifest)),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(format!(
                                "ID：{} · 作者：{} · 版本：{}",
                                manifest.name,
                                plugin_author(&manifest),
                                plugin_version(&manifest)
                            )),
                    )
                    .when_some(installed_version, |content, version| {
                        content.child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(format!(
                                    "当前版本：v{version} → 目标版本：{}",
                                    plugin_version(&manifest)
                                )),
                        )
                    })
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child(format!("目录来源 · 指纹：{fingerprint}…")),
                    )
                    .child(component_summary(&components))
                    .child(div().font_medium().child("Provider 兼容性"))
                    .child(provider_compatibility_rows(&compatibility))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child(
                                "Skills 会作为说明注入，MCP Server 可能启动本地进程或访问远程服务；请只安装信任来源的插件。",
                            ),
                    ),
            )
    });
}

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
    let plugin_id = plugin.plugin_id.clone();
    let uninstall_id = plugin_id.clone();
    let enabled = plugin.enabled;
    let action_disabled = !is_online || operation_in_flight;

    div()
        .v_flex()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(COLOR_BORDER))
        .bg(rgb(COLOR_SURFACE))
        .p_4()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_3()
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
                        .child(
                            div()
                                .font_medium()
                                .child(plugin_display_name(&plugin.manifest)),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(plugin_description(&plugin.manifest)),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(format!(
                                    "{} · {} · {}",
                                    plugin.manifest.name,
                                    plugin_version(&plugin.manifest),
                                    plugin_source_label(&plugin.source)
                                )),
                        ),
                )
                .child(
                    settings_action_button(("plugin-toggle", index), cx)
                        .label(if enabled { "禁用" } else { "启用" })
                        .disabled(action_disabled)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let plugin_id = plugin_id.clone();
                            toggle_view.update(cx, |view, cx| {
                                view.set_plugin_enabled(plugin_id, !enabled, cx);
                            });
                        })),
                )
                .child(
                    settings_danger_action_button(("plugin-uninstall", index), cx)
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
                ),
        )
        .child(component_summary(&plugin.components))
        .child(provider_compatibility_rows(&plugin.provider_compatibility))
}

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
    let plugin_id = entry.plugin_id.clone();
    let version = plugin_source_version(&entry.source);
    let is_update = entry.installed;
    let update_confirmation_pending = pending_update == Some(entry.plugin_id.as_str());
    let can_install = supported
        && is_online
        && !operation_in_flight
        && entry.compatible
        && !matches!(
            entry.policy.installation,
            corbit_client::PluginInstallationPolicy::NotAvailable
        );

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
                .child(entry.name.clone())
                .child(
                    div()
                        .text_size(font_px(FONT_SIZE_XS))
                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                        .child(format!(
                            "{} · {}{}",
                            entry.category,
                            plugin_source_label(&entry.source),
                            entry
                                .installed_version
                                .as_deref()
                                .map_or_else(String::new, |version| format!(
                                    " · 已安装 v{version}"
                                ))
                        )),
                )
                .when_some(entry.incompatibility_reason.clone(), |details, reason| {
                    details.child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_WARNING))
                            .child(reason),
                    )
                }),
        )
        .when(can_install, |row| {
            row.child(
                settings_primary_action_button(("plugin-marketplace-install", index), cx)
                    .icon(Icon::new(if is_update {
                        AppIcon::Refresh
                    } else {
                        AppIcon::Add
                    }))
                    .label(if update_confirmation_pending {
                        "确认更新"
                    } else if is_update {
                        "更新"
                    } else {
                        "安装"
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let plugin_id = plugin_id.clone();
                        let version = version.clone();
                        install_view.update(cx, |view, cx| {
                            view.install_marketplace_plugin(plugin_id, version, is_update, cx);
                        });
                    })),
            )
        })
}

fn component_summary(components: &corbit_client::PluginComponents) -> Div {
    let mut parts = vec![format!("Skills {}", components.skill_count)];
    parts.push(format!("MCP {}", components.mcp_server_names.len()));
    if components.has_hooks {
        parts.push("Hooks".into());
    }
    if components.has_apps {
        parts.push("Apps".into());
    }
    div()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(COLOR_TEXT_TERTIARY))
        .child(parts.join(" · "))
}

fn provider_compatibility_rows(compatibility: &corbit_client::PluginProviderCompatibility) -> Div {
    div()
        .h_flex()
        .flex_wrap()
        .gap_2()
        .child(compatibility_badge("Codex", &compatibility.codex))
        .child(compatibility_badge("Claude", &compatibility.claude))
        .child(compatibility_badge("ACP", &compatibility.acp))
}

fn compatibility_badge(
    provider: &'static str,
    compatibility: &corbit_client::PluginProviderCompatibilityEntry,
) -> Div {
    let (label, color) = match compatibility.status {
        corbit_client::PluginProviderCompatibilityStatus::Full => ("完整", COLOR_SUCCESS),
        corbit_client::PluginProviderCompatibilityStatus::Partial => ("部分", COLOR_WARNING),
        corbit_client::PluginProviderCompatibilityStatus::Unsupported => ("不支持", COLOR_ERROR),
    };
    div()
        .flex_none()
        .rounded_full()
        .bg(rgb(COLOR_SURFACE_SECONDARY))
        .px_2()
        .py_1()
        .text_size(font_px(FONT_SIZE_XS))
        .text_color(rgb(color))
        .child(format!("{provider} {label}"))
}

fn plugin_display_name(manifest: &corbit_client::PluginManifest) -> String {
    manifest
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.clone())
        .unwrap_or_else(|| manifest.name.clone())
}

fn plugin_description(manifest: &corbit_client::PluginManifest) -> String {
    manifest
        .description
        .clone()
        .unwrap_or_else(|| "未提供描述".into())
}

fn plugin_version(manifest: &corbit_client::PluginManifest) -> String {
    manifest
        .version
        .as_ref()
        .map_or_else(|| "未声明".into(), |version| format!("v{version}"))
}

fn plugin_author(manifest: &corbit_client::PluginManifest) -> String {
    manifest
        .author
        .as_ref()
        .map_or_else(|| "未声明".into(), |author| author.name.clone())
}

fn plugin_source_label(source: &corbit_client::PluginSource) -> &'static str {
    match source {
        corbit_client::PluginSource::Local { .. } => "本地目录",
        corbit_client::PluginSource::Git { .. } => "Git",
        corbit_client::PluginSource::Npm { .. } => "NPM",
    }
}

fn plugin_source_version(source: &corbit_client::PluginSource) -> Option<String> {
    match source {
        corbit_client::PluginSource::Npm { version, .. } => version.clone(),
        corbit_client::PluginSource::Local { .. } | corbit_client::PluginSource::Git { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        official_plugin_display_name, official_plugin_matches, plugin_display_name,
        plugin_source_label, plugin_source_version,
    };
    use corbit_client::{
        CodexOfficialPluginInterface, CodexOfficialPluginSummary, PluginInterface, PluginManifest,
        PluginSource,
    };

    #[test]
    fn plugin_labels_follow_codex_manifest_and_source_metadata() {
        let manifest = PluginManifest {
            name: "example.plugin".into(),
            version: Some("1.0.0".into()),
            description: Some("Example".into()),
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: Vec::new(),
            skills: Some("./skills".into()),
            mcp_servers: None,
            apps: None,
            hooks: None,
            interface: Some(PluginInterface {
                display_name: Some("Example Plugin".into()),
                short_description: None,
                long_description: None,
                developer_name: None,
                category: None,
                capabilities: Vec::new(),
                website_url: None,
                privacy_policy_url: None,
                terms_of_service_url: None,
                default_prompt: Vec::new(),
                brand_color: None,
                composer_icon: None,
                logo: None,
                screenshots: Vec::new(),
            }),
        };
        let source = PluginSource::Npm {
            package: "@example/plugin".into(),
            version: Some("1.0.0".into()),
            registry: None,
        };

        assert_eq!(plugin_display_name(&manifest), "Example Plugin");
        assert_eq!(plugin_source_label(&source), "NPM");
        assert_eq!(plugin_source_version(&source).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn official_plugin_search_covers_catalog_and_interface_metadata() {
        let plugin = CodexOfficialPluginSummary {
            id: "plugin_gmail".into(),
            name: "gmail".into(),
            installed: false,
            enabled: false,
            source_type: "remote".into(),
            availability: "AVAILABLE".into(),
            install_policy: "AVAILABLE".into(),
            auth_policy: "ON_INSTALL".into(),
            version: Some("0.1.3".into()),
            local_version: None,
            keywords: vec!["email".into()],
            must_show_installation_interstitial: Some(true),
            interface: Some(CodexOfficialPluginInterface {
                display_name: Some("Gmail".into()),
                short_description: Some("Read and manage Gmail".into()),
                developer_name: Some("OpenAI".into()),
                category: Some("Communication".into()),
                ..CodexOfficialPluginInterface::default()
            }),
        };

        assert_eq!(official_plugin_display_name(&plugin), "Gmail");
        assert!(official_plugin_matches(&plugin, "gmail"));
        assert!(official_plugin_matches(&plugin, "manage"));
        assert!(official_plugin_matches(&plugin, "communication"));
        assert!(official_plugin_matches(&plugin, "email"));
        assert!(!official_plugin_matches(&plugin, "calendar"));
    }
}
