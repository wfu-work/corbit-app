use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScheduledFilter {
    All,
    Active,
    Paused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScheduledCadence {
    Interval,
    Daily,
    Workdays,
    Weekly,
}

type ScheduledState = (
    Vec<corbit_client::ScheduledTask>,
    Vec<corbit_client::ScheduledRun>,
);

async fn fetch_scheduled_state(
    client: &corbit_client::DaemonRuntimeClient,
) -> Result<ScheduledState, corbit_client::ClientError> {
    let tasks = client.scheduled_tasks().await?;
    let runs = client.scheduled_runs(None, Some(200)).await?;
    Ok((tasks, runs))
}

fn scheduled_time_label(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "尚未运行".into();
    };
    chrono::DateTime::parse_from_rfc3339(value).map_or_else(
        |_| value.to_owned(),
        |timestamp| {
            timestamp
                .with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        },
    )
}

fn weekday_label(weekday: u8) -> &'static str {
    match weekday {
        0 => "周日",
        1 => "周一",
        2 => "周二",
        3 => "周三",
        4 => "周四",
        5 => "周五",
        6 => "周六",
        _ => "周一",
    }
}

fn schedule_label(schedule: &corbit_client::ScheduledTaskSchedule) -> String {
    match schedule {
        corbit_client::ScheduledTaskSchedule::Interval { every_minutes } => {
            format!("每隔 {every_minutes} 分钟")
        }
        corbit_client::ScheduledTaskSchedule::Daily { hour, minute } => {
            format!("每天 {hour:02}:{minute:02}")
        }
        corbit_client::ScheduledTaskSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } if weekdays == &[1, 2, 3, 4, 5] => {
            format!("工作日 {hour:02}:{minute:02}")
        }
        corbit_client::ScheduledTaskSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } => {
            let days = weekdays
                .iter()
                .map(|weekday| weekday_label(*weekday))
                .collect::<Vec<_>>()
                .join("、");
            format!("{days} {hour:02}:{minute:02}")
        }
    }
}

fn parse_clock(value: &str) -> Result<(u8, u8), &'static str> {
    let Some((hour, minute)) = value.trim().split_once(':') else {
        return Err("时间格式应为 HH:MM");
    };
    let hour = hour.parse::<u8>().map_err(|_| "小时必须是 0–23")?;
    let minute = minute.parse::<u8>().map_err(|_| "分钟必须是 0–59")?;
    if hour > 23 || minute > 59 {
        return Err("时间必须在 00:00–23:59 之间");
    }
    Ok((hour, minute))
}

impl ConnectionView {
    fn scheduled_supported(&self) -> bool {
        self.server_info
            .as_ref()
            .and_then(|info| info.features.get("scheduledTasks"))
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn load_scheduled_tasks(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, corbit_client::ConnectionState::Online)
            || !self.scheduled_supported()
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
        self.scheduled_request_id = self.scheduled_request_id.wrapping_add(1);
        let request_id = self.scheduled_request_id;
        self.scheduled_refresh_task = Some(cx.spawn(async move |view, cx| {
            let result = fetch_scheduled_state(&client).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.scheduled_request_id != request_id {
                    return;
                }
                match result {
                    Ok((tasks, runs)) => {
                        view.scheduled_tasks = tasks;
                        view.scheduled_runs = runs;
                        view.scheduled_delete_confirmation = None;
                        cx.notify();
                    }
                    Err(error) => {
                        view.show_error(format!("读取已安排任务失败：{error}"), cx);
                    }
                }
            });
        }));
    }

    pub(super) fn schedule_scheduled_refresh(&mut self, cx: &mut Context<Self>) {
        self.scheduled_refresh_task = Some(cx.spawn(async move |view, cx| {
            Timer::after(Duration::from_millis(200)).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| view.load_scheduled_tasks(cx));
        }));
    }

    fn set_scheduled_filter(&mut self, filter: ScheduledFilter, cx: &mut Context<Self>) {
        self.scheduled_filter = filter;
        cx.notify();
    }

    fn choose_scheduled_agent(&mut self, agent_id: String, cx: &mut Context<Self>) {
        self.scheduled_agent_id = Some(agent_id);
        cx.notify();
    }

    fn choose_scheduled_cadence(&mut self, cadence: ScheduledCadence, cx: &mut Context<Self>) {
        self.scheduled_cadence = cadence;
        cx.notify();
    }

    fn choose_scheduled_weekday(&mut self, weekday: u8, cx: &mut Context<Self>) {
        self.scheduled_weekday = weekday;
        cx.notify();
    }

    fn choose_scheduled_permission(
        &mut self,
        permission: corbit_client::AgentPermissionMode,
        cx: &mut Context<Self>,
    ) {
        self.scheduled_permission_mode = permission;
        cx.notify();
    }

    fn open_scheduled_editor(
        &mut self,
        task_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = task_id
            .as_ref()
            .and_then(|task_id| self.scheduled_tasks.iter().find(|task| &task.id == task_id))
            .cloned();
        let eligible_agent = |agent: &&corbit_client::AgentResource| {
            agent.status != corbit_client::AgentStatus::Stopped && agent.provider != "acp"
        };
        self.scheduled_agent_id = task.as_ref().map(|task| task.agent_id.clone()).or_else(|| {
            self.snapshot
                .as_ref()
                .and_then(|snapshot| {
                    self.selected_agent_id
                        .as_ref()
                        .and_then(|selected| {
                            snapshot
                                .agents
                                .iter()
                                .find(|agent| &agent.id == selected)
                                .filter(eligible_agent)
                        })
                        .or_else(|| snapshot.agents.iter().find(eligible_agent))
                })
                .map(|agent| agent.id.clone())
        });
        self.scheduled_permission_mode = task
            .as_ref()
            .and_then(|task| task.prompt_options.permission_mode)
            .unwrap_or(corbit_client::AgentPermissionMode::ReadOnly);
        self.scheduled_cadence = task
            .as_ref()
            .map_or(ScheduledCadence::Daily, |task| match &task.schedule {
                corbit_client::ScheduledTaskSchedule::Interval { .. } => ScheduledCadence::Interval,
                corbit_client::ScheduledTaskSchedule::Daily { .. } => ScheduledCadence::Daily,
                corbit_client::ScheduledTaskSchedule::Weekly { weekdays, .. }
                    if weekdays == &[1, 2, 3, 4, 5] =>
                {
                    ScheduledCadence::Workdays
                }
                corbit_client::ScheduledTaskSchedule::Weekly { .. } => ScheduledCadence::Weekly,
            });
        if let Some(corbit_client::ScheduledTaskSchedule::Weekly { weekdays, .. }) =
            task.as_ref().map(|task| &task.schedule)
            && let Some(weekday) = weekdays.first()
        {
            self.scheduled_weekday = *weekday;
        }
        let title = task
            .as_ref()
            .map_or_else(String::new, |task| task.title.clone());
        let prompt = task
            .as_ref()
            .map_or_else(String::new, |task| task.prompt.clone());
        let interval = task.as_ref().and_then(|task| match task.schedule {
            corbit_client::ScheduledTaskSchedule::Interval { every_minutes } => {
                Some(every_minutes.to_string())
            }
            _ => None,
        });
        let time = task.as_ref().and_then(|task| match task.schedule {
            corbit_client::ScheduledTaskSchedule::Daily { hour, minute }
            | corbit_client::ScheduledTaskSchedule::Weekly { hour, minute, .. } => {
                Some(format!("{hour:02}:{minute:02}"))
            }
            corbit_client::ScheduledTaskSchedule::Interval { .. } => None,
        });
        self.scheduled_title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.scheduled_prompt
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        self.scheduled_interval.update(cx, |input, cx| {
            input.set_value(interval.unwrap_or_else(|| "60".into()), window, cx);
        });
        self.scheduled_time.update(cx, |input, cx| {
            input.set_value(time.unwrap_or_else(|| "09:00".into()), window, cx);
        });
        self.scheduled_editing_task_id = task_id;
        self.scheduled_editor_open = true;
        self.scheduled_delete_confirmation = None;
        cx.notify();
    }

    fn open_scheduled_template(
        &mut self,
        title: &'static str,
        prompt: &'static str,
        cadence: ScheduledCadence,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_scheduled_editor(None, window, cx);
        self.scheduled_title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.scheduled_prompt
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        self.scheduled_cadence = cadence;
        cx.notify();
    }

    fn close_scheduled_editor(&mut self, cx: &mut Context<Self>) {
        self.scheduled_editor_open = false;
        self.scheduled_editing_task_id = None;
        cx.notify();
    }

    fn scheduled_form_schedule(
        &self,
        cx: &App,
    ) -> Result<corbit_client::ScheduledTaskSchedule, &'static str> {
        if self.scheduled_cadence == ScheduledCadence::Interval {
            let every_minutes = Self::input_value(&self.scheduled_interval, cx)
                .parse::<u32>()
                .map_err(|_| "间隔分钟数必须是整数")?;
            if !(1..=10_080).contains(&every_minutes) {
                return Err("间隔分钟数必须在 1–10080 之间");
            }
            return Ok(corbit_client::ScheduledTaskSchedule::Interval { every_minutes });
        }
        let (hour, minute) = parse_clock(&Self::input_value(&self.scheduled_time, cx))?;
        Ok(match self.scheduled_cadence {
            ScheduledCadence::Daily => corbit_client::ScheduledTaskSchedule::Daily { hour, minute },
            ScheduledCadence::Workdays => corbit_client::ScheduledTaskSchedule::Weekly {
                weekdays: vec![1, 2, 3, 4, 5],
                hour,
                minute,
            },
            ScheduledCadence::Weekly => corbit_client::ScheduledTaskSchedule::Weekly {
                weekdays: vec![self.scheduled_weekday],
                hour,
                minute,
            },
            ScheduledCadence::Interval => unreachable!(),
        })
    }

    fn submit_scheduled_task(&mut self, cx: &mut Context<Self>) {
        if self.scheduled_operation_in_flight {
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
        let title = Self::input_value(&self.scheduled_title, cx);
        let prompt = Self::input_value(&self.scheduled_prompt, cx);
        let Some(agent_id) = self.scheduled_agent_id.clone() else {
            self.show_validation_error("请选择运行此计划的 Agent", cx);
            return;
        };
        if title.is_empty() || prompt.is_empty() {
            self.show_validation_error("请输入计划名称和每次运行的任务说明", cx);
            return;
        }
        let schedule = match self.scheduled_form_schedule(cx) {
            Ok(schedule) => schedule,
            Err(error) => {
                self.show_validation_error(error, cx);
                return;
            }
        };
        let editing_id = self.scheduled_editing_task_id.clone();
        let mut prompt_options = editing_id
            .as_ref()
            .and_then(|task_id| self.scheduled_tasks.iter().find(|task| &task.id == task_id))
            .map(|task| task.prompt_options.clone())
            .unwrap_or_default();
        prompt_options.permission_mode = Some(self.scheduled_permission_mode);
        self.scheduled_operation_in_flight = true;
        self.scheduled_delete_confirmation = None;
        self.scheduled_task = Some(cx.spawn(async move |view, cx| {
            let result = async {
                if let Some(task_id) = editing_id {
                    client
                        .update_scheduled_task(corbit_client::ScheduledTaskUpdateInput {
                            task_id,
                            title: Some(title),
                            agent_id: Some(agent_id),
                            prompt: Some(prompt),
                            schedule: Some(schedule),
                            time_zone: None,
                            prompt_options: Some(prompt_options),
                        })
                        .await?;
                } else {
                    client
                        .create_scheduled_task(corbit_client::ScheduledTaskCreateInput {
                            title,
                            agent_id,
                            prompt,
                            schedule,
                            time_zone: None,
                            prompt_options: Some(prompt_options),
                        })
                        .await?;
                }
                fetch_scheduled_state(&client).await
            }
            .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.scheduled_operation_in_flight = false;
                match result {
                    Ok((tasks, runs)) => {
                        view.scheduled_tasks = tasks;
                        view.scheduled_runs = runs;
                        view.scheduled_editor_open = false;
                        view.scheduled_editing_task_id = None;
                        view.show_success("已保存计划任务", cx);
                    }
                    Err(error) => view.show_error(format!("保存计划任务失败：{error}"), cx),
                }
            });
        }));
        cx.notify();
    }

    fn set_scheduled_paused(&mut self, task_id: String, paused: bool, cx: &mut Context<Self>) {
        self.run_scheduled_action(
            move |client| async move {
                client.set_scheduled_task_paused(task_id, paused).await?;
                fetch_scheduled_state(&client).await
            },
            if paused {
                "计划已暂停"
            } else {
                "计划已恢复"
            },
            cx,
        );
    }

    fn run_scheduled_now(&mut self, task_id: String, cx: &mut Context<Self>) {
        if self.scheduled_operation_in_flight {
            self.show_warning("另一个计划任务操作正在进行", cx);
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
        self.scheduled_operation_in_flight = true;
        self.scheduled_task = Some(cx.spawn(async move |view, cx| {
            let result = async {
                let run = client.run_scheduled_task_now(task_id).await?;
                let state = fetch_scheduled_state(&client).await?;
                Ok::<_, corbit_client::ClientError>((run, state))
            }
            .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.scheduled_operation_in_flight = false;
                match result {
                    Ok((run, (tasks, runs))) => {
                        view.scheduled_tasks = tasks;
                        view.scheduled_runs = runs;
                        view.scheduled_delete_confirmation = None;
                        match run.status {
                            corbit_client::ScheduledRunStatus::Running => {
                                view.show_success("计划任务已开始运行", cx);
                            }
                            corbit_client::ScheduledRunStatus::Completed => {
                                view.show_success("计划任务已完成", cx);
                            }
                            corbit_client::ScheduledRunStatus::Failed => view.show_error(
                                format!(
                                    "立即运行失败：{}",
                                    run.error.as_deref().unwrap_or("未提供错误详情")
                                ),
                                cx,
                            ),
                            corbit_client::ScheduledRunStatus::Skipped => view.show_warning(
                                format!(
                                    "本次运行已跳过：{}",
                                    run.error.as_deref().unwrap_or("已有运行正在执行")
                                ),
                                cx,
                            ),
                        }
                    }
                    Err(error) => view.show_error(format!("立即运行失败：{error}"), cx),
                }
            });
        }));
        cx.notify();
    }

    fn request_scheduled_delete(&mut self, task_id: String, cx: &mut Context<Self>) {
        if self.scheduled_delete_confirmation.as_ref() != Some(&task_id) {
            self.scheduled_delete_confirmation = Some(task_id);
            self.show_warning("删除计划不可撤销；请再次点击确认删除", cx);
            return;
        }
        self.run_scheduled_action(
            move |client| async move {
                client.delete_scheduled_task(task_id).await?;
                fetch_scheduled_state(&client).await
            },
            "计划已删除",
            cx,
        );
    }

    fn run_scheduled_action<F, Fut>(
        &mut self,
        operation: F,
        success_message: &'static str,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(corbit_client::DaemonRuntimeClient) -> Fut + 'static,
        Fut: Future<Output = Result<ScheduledState, corbit_client::ClientError>> + 'static,
    {
        if self.scheduled_operation_in_flight {
            self.show_warning("另一个计划任务操作正在进行", cx);
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
        self.scheduled_operation_in_flight = true;
        self.scheduled_task = Some(cx.spawn(async move |view, cx| {
            let result = operation(client).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.scheduled_operation_in_flight = false;
                match result {
                    Ok((tasks, runs)) => {
                        view.scheduled_tasks = tasks;
                        view.scheduled_runs = runs;
                        view.scheduled_delete_confirmation = None;
                        view.show_success(success_message, cx);
                    }
                    Err(error) => view.show_error(format!("计划任务操作失败：{error}"), cx),
                }
            });
        }));
        cx.notify();
    }

    fn toggle_scheduled_runs(&mut self, task_id: String, cx: &mut Context<Self>) {
        if !self.scheduled_expanded_runs.insert(task_id.clone()) {
            self.scheduled_expanded_runs.remove(&task_id);
        }
        cx.notify();
    }

    fn render_scheduled_editor(&self, is_online: bool, cx: &mut Context<Self>) -> AnyElement {
        let agents = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .filter(|agent| {
                        agent.status != corbit_client::AgentStatus::Stopped
                            && agent.provider != "acp"
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected_agent_label = agents
            .iter()
            .find(|agent| self.scheduled_agent_id.as_ref() == Some(&agent.id))
            .map_or_else(|| "选择 Agent".to_owned(), |agent| agent.title.clone());
        let selected_agent_id = self.scheduled_agent_id.clone();
        let agents_empty = agents.is_empty();
        let agent_view = cx.entity();
        let agent_menu = settings_select_button("scheduled-agent", cx)
            .label(selected_agent_label)
            .disabled(agents_empty || self.scheduled_operation_in_flight)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(260.));
                for agent in agents.clone() {
                    let item_view = agent_view.clone();
                    let agent_id = agent.id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(agent.title)
                            .checked(selected_agent_id.as_ref() == Some(&agent_id))
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_scheduled_agent(agent_id.clone(), cx);
                                });
                            }),
                    );
                }
                menu
            });
        let cadence_view = cx.entity();
        let cadence = self.scheduled_cadence;
        let cadence_menu = settings_select_button("scheduled-cadence", cx)
            .label(match cadence {
                ScheduledCadence::Interval => "每隔 N 分钟",
                ScheduledCadence::Daily => "每天",
                ScheduledCadence::Workdays => "工作日",
                ScheduledCadence::Weekly => "每周",
            })
            .disabled(self.scheduled_operation_in_flight)
            .dropdown_menu(move |menu, _, _| {
                [
                    (ScheduledCadence::Interval, "每隔 N 分钟"),
                    (ScheduledCadence::Daily, "每天"),
                    (ScheduledCadence::Workdays, "工作日"),
                    (ScheduledCadence::Weekly, "每周"),
                ]
                .into_iter()
                .fold(menu.min_w(px(190.)), |menu, (candidate, label)| {
                    let item_view = cadence_view.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(cadence == candidate)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_scheduled_cadence(candidate, cx);
                                });
                            }),
                    )
                })
            });
        let weekday_view = cx.entity();
        let weekday = self.scheduled_weekday;
        let weekday_menu = settings_select_button("scheduled-weekday", cx)
            .label(weekday_label(weekday))
            .disabled(self.scheduled_operation_in_flight)
            .dropdown_menu(move |menu, _, _| {
                (0..=6).fold(menu.min_w(px(140.)), |menu, candidate| {
                    let item_view = weekday_view.clone();
                    menu.item(
                        PopupMenuItem::new(weekday_label(candidate))
                            .checked(weekday == candidate)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_scheduled_weekday(candidate, cx);
                                });
                            }),
                    )
                })
            });
        let permission_view = cx.entity();
        let permission = self.scheduled_permission_mode;
        let permission_menu = settings_select_button("scheduled-permission", cx)
            .label(match permission {
                corbit_client::AgentPermissionMode::ReadOnly => "只读",
                corbit_client::AgentPermissionMode::WorkspaceWrite => "工作区写入",
                corbit_client::AgentPermissionMode::FullAccess => "完全访问",
            })
            .disabled(self.scheduled_operation_in_flight)
            .dropdown_menu(move |menu, _, _| {
                [
                    (corbit_client::AgentPermissionMode::ReadOnly, "只读"),
                    (
                        corbit_client::AgentPermissionMode::WorkspaceWrite,
                        "工作区写入",
                    ),
                    (corbit_client::AgentPermissionMode::FullAccess, "完全访问"),
                ]
                .into_iter()
                .fold(menu.min_w(px(180.)), |menu, (candidate, label)| {
                    let item_view = permission_view.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(permission == candidate)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_scheduled_permission(candidate, cx);
                                });
                            }),
                    )
                })
            });

        settings_card(if self.scheduled_editing_task_id.is_some() {
            "编辑计划"
        } else {
            "新建计划"
        })
        .child(settings_input(&self.scheduled_title).disabled(self.scheduled_operation_in_flight))
        .child(settings_input(&self.scheduled_prompt).disabled(self.scheduled_operation_in_flight))
        .child(
            div()
                .h_flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(agent_menu)
                .child(cadence_menu)
                .when(self.scheduled_cadence == ScheduledCadence::Weekly, |row| {
                    row.child(weekday_menu)
                })
                .child(div().w(px(120.)).child(
                    if self.scheduled_cadence == ScheduledCadence::Interval {
                        settings_input(&self.scheduled_interval)
                    } else {
                        settings_input(&self.scheduled_time)
                    },
                ))
                .child(permission_menu),
        )
        .child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(
                    if permission == corbit_client::AgentPermissionMode::FullAccess {
                        rgb(COLOR_WARNING)
                    } else {
                        rgb(COLOR_TEXT_TERTIARY)
                    },
                )
                .child(
                    if permission == corbit_client::AgentPermissionMode::FullAccess {
                        "完全访问会在无人值守时运行命令并修改文件，请仅在确有需要时使用。"
                    } else {
                        "计划由 Daemon 在后台运行；默认使用只读权限，不会隐式提升访问级别。"
                    },
                ),
        )
        .child(
            div()
                .h_flex()
                .justify_end()
                .gap_2()
                .child(
                    settings_quiet_action_button("scheduled-cancel")
                        .label("取消")
                        .disabled(self.scheduled_operation_in_flight)
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.close_scheduled_editor(cx);
                        })),
                )
                .child(
                    settings_primary_action_button("scheduled-save", cx)
                        .label(if self.scheduled_editing_task_id.is_some() {
                            "保存修改"
                        } else {
                            "创建计划"
                        })
                        .loading(self.scheduled_operation_in_flight)
                        .disabled(
                            !is_online
                                || !self.scheduled_supported()
                                || self.scheduled_agent_id.is_none(),
                        )
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.submit_scheduled_task(cx);
                        })),
                ),
        )
        .into_any_element()
    }

    pub(super) fn render_scheduled_panel(
        &self,
        is_online: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.scheduled_search.read(cx).value().trim().to_lowercase();
        let visible_tasks = self
            .scheduled_tasks
            .iter()
            .filter(|task| match self.scheduled_filter {
                ScheduledFilter::All => true,
                ScheduledFilter::Active => {
                    task.status == corbit_client::ScheduledTaskStatus::Active
                }
                ScheduledFilter::Paused => {
                    task.status == corbit_client::ScheduledTaskStatus::Paused
                }
            })
            .filter(|task| {
                if query.is_empty() {
                    return true;
                }
                let agent_title = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        snapshot
                            .agents
                            .iter()
                            .find(|agent| agent.id == task.agent_id)
                    })
                    .map_or("", |agent| agent.title.as_str());
                task.title.to_lowercase().contains(&query)
                    || task.prompt.to_lowercase().contains(&query)
                    || agent_title.to_lowercase().contains(&query)
            })
            .cloned()
            .collect::<Vec<_>>();
        let active_count = self
            .scheduled_tasks
            .iter()
            .filter(|task| task.status == corbit_client::ScheduledTaskStatus::Active)
            .count();
        let paused_count = self.scheduled_tasks.len().saturating_sub(active_count);
        let filter_buttons = [
            (ScheduledFilter::All, "全部"),
            (ScheduledFilter::Active, "运行中"),
            (ScheduledFilter::Paused, "已暂停"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (filter, label))| {
            Button::new(("scheduled-filter", index))
                .ghost()
                .small()
                .selected(self.scheduled_filter == filter)
                .label(label)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.set_scheduled_filter(filter, cx);
                }))
        })
        .collect::<Vec<_>>();
        let task_rows = visible_tasks
            .into_iter()
            .enumerate()
            .map(|(index, task)| {
                let agent_title = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| {
                        snapshot
                            .agents
                            .iter()
                            .find(|agent| agent.id == task.agent_id)
                    })
                    .map_or("未知 Agent", |agent| agent.title.as_str())
                    .to_owned();
                let paused = task.status == corbit_client::ScheduledTaskStatus::Paused;
                let task_id_for_toggle = task.id.clone();
                let task_id_for_run = task.id.clone();
                let task_id_for_edit = task.id.clone();
                let task_id_for_delete = task.id.clone();
                let task_id_for_expand = task.id.clone();
                let confirming = self.scheduled_delete_confirmation.as_ref() == Some(&task.id);
                let expanded = self.scheduled_expanded_runs.contains(&task.id);
                let runs = self
                    .scheduled_runs
                    .iter()
                    .filter(|run| run.task_id == task.id)
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>();
                let run_rows = runs.iter().enumerate().map(|(run_index, run)| {
                    let (label, color) = match run.status {
                        corbit_client::ScheduledRunStatus::Running => ("运行中", COLOR_WARNING),
                        corbit_client::ScheduledRunStatus::Completed => ("已完成", COLOR_SUCCESS),
                        corbit_client::ScheduledRunStatus::Failed => ("失败", COLOR_ERROR),
                        corbit_client::ScheduledRunStatus::Skipped => {
                            ("已跳过", COLOR_TEXT_TERTIARY)
                        }
                    };
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .gap_3()
                        .py_2()
                        .border_t_1()
                        .border_color(rgb(COLOR_BORDER_LIGHT))
                        .child(
                            div()
                                .w(px(58.))
                                .text_size(font_px(FONT_SIZE_XS))
                                .font_medium()
                                .text_color(rgb(color))
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(run.error.clone().unwrap_or_else(|| {
                                    if run.trigger == corbit_client::ScheduledRunTrigger::Manual {
                                        "手动触发".into()
                                    } else {
                                        "按计划触发".into()
                                    }
                                })),
                        )
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(scheduled_time_label(
                                    run.completed_at.as_deref().or(run.started_at.as_deref()),
                                )),
                        )
                        .id(("scheduled-run-row", index * 10 + run_index))
                });
                div()
                    .v_flex()
                    .w_full()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(COLOR_BORDER))
                    .bg(rgb(COLOR_SURFACE_UNDER))
                    .p_4()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .h_flex()
                                    .size(px(34.))
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                                    .child(Icon::new(AppIcon::Scheduled).size(px(17.))),
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
                                            .gap_2()
                                            .child(div().font_medium().child(task.title.clone()))
                                            .child(
                                                div()
                                                    .rounded_full()
                                                    .px_2()
                                                    .py_0p5()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .bg(rgb(COLOR_SURFACE_SECONDARY))
                                                    .text_color(if paused {
                                                        rgb(COLOR_TEXT_TERTIARY)
                                                    } else {
                                                        rgb(COLOR_SUCCESS)
                                                    })
                                                    .child(if paused {
                                                        "已暂停"
                                                    } else {
                                                        "运行中"
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                                            .child(task.prompt.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child(format!(
                                                "{} · {} · {} · 下次 {}",
                                                agent_title,
                                                schedule_label(&task.schedule),
                                                task.time_zone,
                                                scheduled_time_label(task.next_run_at.as_deref())
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                settings_action_button(("scheduled-pause", index), cx)
                                    .label(if paused { "恢复" } else { "暂停" })
                                    .disabled(!is_online || self.scheduled_operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.set_scheduled_paused(
                                            task_id_for_toggle.clone(),
                                            !paused,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                settings_action_button(("scheduled-run", index), cx)
                                    .icon(Icon::new(AppIcon::Play).size(px(14.)))
                                    .label("立即运行")
                                    .disabled(!is_online || self.scheduled_operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.run_scheduled_now(task_id_for_run.clone(), cx);
                                    })),
                            )
                            .child(
                                settings_action_button(("scheduled-edit", index), cx)
                                    .label("编辑")
                                    .disabled(self.scheduled_operation_in_flight)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.open_scheduled_editor(
                                            Some(task_id_for_edit.clone()),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                settings_danger_action_button(("scheduled-delete", index), cx)
                                    .label(if confirming { "确认删除" } else { "删除" })
                                    .disabled(!is_online || self.scheduled_operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.request_scheduled_delete(
                                            task_id_for_delete.clone(),
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                settings_quiet_action_button(("scheduled-history", index))
                                    .label(if expanded {
                                        "收起运行记录"
                                    } else {
                                        "查看运行记录"
                                    })
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.toggle_scheduled_runs(task_id_for_expand.clone(), cx);
                                    })),
                            ),
                    )
                    .when(expanded, |card| {
                        card.child(
                            div()
                                .v_flex()
                                .when(runs.is_empty(), |history| {
                                    history.child(
                                        div()
                                            .pt_2()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child("暂无运行记录"),
                                    )
                                })
                                .children(run_rows),
                        )
                    })
            })
            .collect::<Vec<_>>();

        let supported = self.scheduled_supported();
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
                            .gap_3()
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_HEADING))
                                            .font_semibold()
                                            .child("已安排"),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                                            .child(format!(
                                                "{active_count} 个运行中 · {paused_count} 个已暂停"
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("scheduled-new")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(AppIcon::Add))
                                    .label("新建计划")
                                    .disabled(
                                        !is_online
                                            || !supported
                                            || self.scheduled_operation_in_flight,
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_scheduled_editor(None, window, cx);
                                    })),
                            ),
                    )
                    .when(!supported, |panel| {
                        panel.child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_WARNING))
                                .bg(rgb(COLOR_SURFACE_UNDER))
                                .p_4()
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child("当前 Daemon 版本不支持计划任务，请升级后重试。"),
                        )
                    })
                    .when(self.scheduled_editor_open, |panel| {
                        panel.child(self.render_scheduled_editor(is_online, cx))
                    })
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .border_b_1()
                            .border_color(rgb(COLOR_BORDER_LIGHT))
                            .pb_3()
                            .child(div().h_flex().gap_1().children(filter_buttons))
                            .child(
                                div()
                                    .w(px(260.))
                                    .child(settings_input(&self.scheduled_search)),
                            ),
                    )
                    .when(self.scheduled_tasks.is_empty() && supported, |panel| {
                        panel.child(
                            div()
                                .v_flex()
                                .gap_3()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(COLOR_BORDER))
                                .bg(rgb(COLOR_SURFACE_UNDER))
                                .p_5()
                                .child(div().font_medium().child("让 Corbit 按时继续工作"))
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child("先选择一个已有 Agent，再从常用模板开始。"),
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .child(
                                            settings_action_button("scheduled-template-daily", cx)
                                                .label("每日简报")
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    view.open_scheduled_template(
                                                        "每日项目简报",
                                                        "总结过去一天的重要代码变更、风险和下一步。",
                                                        ScheduledCadence::Daily,
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            settings_action_button("scheduled-template-weekly", cx)
                                                .label("每周回顾")
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    view.open_scheduled_template(
                                                        "每周项目回顾",
                                                        "回顾本周的代码变更、未完成事项和需要关注的风险。",
                                                        ScheduledCadence::Weekly,
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            settings_action_button("scheduled-template-monitor", cx)
                                                .label("跟进监控")
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    view.open_scheduled_template(
                                                        "定期跟进",
                                                        "检查当前工作的最新状态；仅在有重要变化时报告。",
                                                        ScheduledCadence::Interval,
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        ),
                                ),
                        )
                    })
                    .when(
                        !self.scheduled_tasks.is_empty() && task_rows.is_empty(),
                        |panel| {
                            panel.child(
                                div()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(rgb(COLOR_BORDER))
                                    .p_5()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("当前搜索或筛选下没有计划任务。"),
                            )
                        },
                    )
                    .children(task_rows),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_labels_cover_supported_cadences() {
        assert_eq!(
            schedule_label(&corbit_client::ScheduledTaskSchedule::Interval { every_minutes: 30 }),
            "每隔 30 分钟"
        );
        assert_eq!(
            schedule_label(&corbit_client::ScheduledTaskSchedule::Weekly {
                weekdays: vec![1, 2, 3, 4, 5],
                hour: 9,
                minute: 5,
            }),
            "工作日 09:05"
        );
        assert_eq!(parse_clock("23:59"), Ok((23, 59)));
        assert!(parse_clock("24:00").is_err());
    }
}
