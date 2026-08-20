use super::timeline::TimelineStatus;
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SearchScope {
    All,
    Tasks,
    Workspaces,
    Projects,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActivityFilter {
    All,
    Attention,
    Running,
    Completed,
}

impl ConnectionView {
    fn set_search_scope(&mut self, scope: SearchScope, cx: &mut Context<Self>) {
        self.search_scope = scope;
        cx.notify();
    }

    fn set_activity_filter(&mut self, filter: ActivityFilter, cx: &mut Context<Self>) {
        self.activity_filter = filter;
        cx.notify();
    }

    pub(super) fn activity_attention_count(&self) -> usize {
        let errored_agents = self.snapshot.as_ref().map_or(0, |snapshot| {
            snapshot
                .agents
                .iter()
                .filter(|agent| agent.status == corbit_client::AgentStatus::Error)
                .count()
        });
        self.permissions.len() + errored_agents
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_search_panel(&self, cx: &mut Context<Self>) -> Div {
        let query = Self::input_value(&self.search_input, cx).to_lowercase();
        let show_tasks = matches!(self.search_scope, SearchScope::All | SearchScope::Tasks);
        let show_workspaces = matches!(
            self.search_scope,
            SearchScope::All | SearchScope::Workspaces
        );
        let show_projects = matches!(self.search_scope, SearchScope::All | SearchScope::Projects);
        let mut task_rows = Vec::new();
        let mut workspace_rows = Vec::new();
        let mut project_rows = Vec::new();

        if let Some(snapshot) = &self.snapshot {
            if show_tasks {
                for (index, agent) in snapshot.agents.iter().rev().enumerate() {
                    let workspace = snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == agent.workspace_id);
                    let project = workspace.and_then(|workspace| {
                        snapshot
                            .projects
                            .iter()
                            .find(|project| project.id == workspace.project_id)
                    });
                    if !matches_query(
                        &query,
                        [
                            agent.title.as_str(),
                            agent.provider.as_str(),
                            workspace.map_or("", |workspace| workspace.name.as_str()),
                            workspace.map_or("", |workspace| workspace.working_directory.as_str()),
                            project.map_or("", |project| project.name.as_str()),
                        ],
                    ) {
                        continue;
                    }
                    if query.is_empty() && task_rows.len() >= 6 {
                        continue;
                    }
                    let agent_id = agent.id.clone();
                    let status = agent_status_label(&agent.status);
                    let status_color = agent_status_color(&agent.status);
                    let context = format!(
                        "{} · {} · {status}",
                        Self::provider_label(&agent.provider),
                        workspace.map_or("未知工作区", |workspace| workspace.name.as_str()),
                    );
                    task_rows.push(
                        Button::new(("search-task", index))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_3()
                                    .py_2()
                                    .child(brand_mark(24.))
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
                                                    .font_medium()
                                                    .child(agent.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .truncate()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                    .child(context),
                                            ),
                                    )
                                    .child(div().size(px(6.)).rounded_full().bg(status_color))
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                                    ),
                            )
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.select_agent(&agent_id, cx);
                            })),
                    );
                }
            }

            if show_workspaces {
                for (index, workspace) in snapshot.workspaces.iter().enumerate() {
                    let project = snapshot
                        .projects
                        .iter()
                        .find(|project| project.id == workspace.project_id);
                    if !matches_query(
                        &query,
                        [
                            workspace.name.as_str(),
                            workspace.working_directory.as_str(),
                            project.map_or("", |project| project.name.as_str()),
                        ],
                    ) {
                        continue;
                    }
                    if query.is_empty() && workspace_rows.len() >= 6 {
                        continue;
                    }
                    let workspace_id = workspace.id.clone();
                    let context = format!(
                        "{} · {}",
                        project.map_or("未知项目", |project| project.name.as_str()),
                        workspace.working_directory,
                    );
                    workspace_rows.push(
                        Button::new(("search-workspace", index))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_3()
                                    .py_2()
                                    .child(
                                        Icon::new(AppIcon::Workspace)
                                            .text_color(rgb(COLOR_TEXT_SECONDARY)),
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
                                                    .font_medium()
                                                    .child(workspace.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .truncate()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                    .child(context),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child("浏览文件"),
                                    )
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                                    ),
                            )
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.select_workspace(&workspace_id, cx);
                                view.set_main_section(MainSection::Files, cx);
                            })),
                    );
                }
            }

            if show_projects {
                for (index, project) in snapshot.projects.iter().enumerate() {
                    if !matches_query(&query, [project.name.as_str(), project.root_path.as_str()]) {
                        continue;
                    }
                    if query.is_empty() && project_rows.len() >= 6 {
                        continue;
                    }
                    let project_id = project.id.clone();
                    let workspace_count = snapshot
                        .workspaces
                        .iter()
                        .filter(|workspace| workspace.project_id == project.id)
                        .count();
                    project_rows.push(
                        Button::new(("search-project", index))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_3()
                                    .py_2()
                                    .child(
                                        Icon::new(AppIcon::Project)
                                            .text_color(rgb(COLOR_TEXT_SECONDARY)),
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
                                                    .font_medium()
                                                    .child(project.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .truncate()
                                                    .text_size(font_px(FONT_SIZE_XS))
                                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                                    .child(project.root_path.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(font_px(FONT_SIZE_XS))
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child(format!("{workspace_count} 个工作区")),
                                    )
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                                    ),
                            )
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.select_project(&project_id, cx);
                            })),
                    );
                }
            }
        }

        let result_count = task_rows.len() + workspace_rows.len() + project_rows.len();
        let scope_buttons = [
            (SearchScope::All, "全部"),
            (SearchScope::Tasks, "任务"),
            (SearchScope::Workspaces, "工作区"),
            (SearchScope::Projects, "项目"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (scope, label))| {
            Button::new(("search-scope", index))
                .ghost()
                .small()
                .selected(self.search_scope == scope)
                .label(label)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.set_search_scope(scope, cx);
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
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_HEADING))
                                    .font_semibold()
                                    .child("搜索"),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("快速打开任务、工作区和项目。"),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(COLOR_BORDER_HEAVY))
                            .bg(rgb(COLOR_SURFACE))
                            .px_3()
                            .child(Icon::new(AppIcon::Search).text_color(rgb(COLOR_TEXT_TERTIARY)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(Input::new(&self.search_input).appearance(false)),
                            )
                            .when(!query.is_empty(), |bar| {
                                bar.child(
                                    Button::new("clear-global-search")
                                        .xsmall()
                                        .ghost()
                                        .icon(AppIcon::Close)
                                        .tooltip("清除搜索")
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.search_input.update(cx, |input, cx| {
                                                input.set_value("", window, cx);
                                                input.focus(window, cx);
                                            });
                                        })),
                                )
                            }),
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
                            .child(div().h_flex().gap_1().children(scope_buttons))
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(if query.is_empty() {
                                        "最近与常用".to_owned()
                                    } else {
                                        format!("{result_count} 个结果")
                                    }),
                            ),
                    )
                    .when(!task_rows.is_empty(), |panel| {
                        panel.child(search_result_group("任务", task_rows))
                    })
                    .when(!workspace_rows.is_empty(), |panel| {
                        panel.child(search_result_group("工作区", workspace_rows))
                    })
                    .when(!project_rows.is_empty(), |panel| {
                        panel.child(search_result_group("项目", project_rows))
                    })
                    .when(result_count == 0, |panel| {
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
                                .child(
                                    Icon::new(AppIcon::Search).text_color(rgb(COLOR_TEXT_TERTIARY)),
                                )
                                .child(div().font_medium().child("没有匹配结果"))
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child("尝试更短的关键词，或切换搜索范围。"),
                                ),
                        )
                    }),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_activity_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut permission_rows = Vec::new();
        let mut status_rows = Vec::new();
        let mut timeline_rows = Vec::new();
        let snapshot = self.snapshot.as_ref();

        if matches!(
            self.activity_filter,
            ActivityFilter::All | ActivityFilter::Attention
        ) {
            for (index, permission) in self.permissions.iter().rev().enumerate() {
                let agent = snapshot.and_then(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .find(|agent| agent.id == permission.agent_id)
                });
                let agent_id = permission.agent_id.clone();
                let kind = match permission.permission_kind.as_str() {
                    "command" => "命令执行等待审批",
                    "file-change" => "文件修改等待审批",
                    _ => "任务等待审批",
                };
                let detail = permission
                    .command
                    .as_deref()
                    .or(permission.reason.as_deref())
                    .map_or_else(
                        || format!("Turn {}", permission.turn_id),
                        |detail| compact_summary(detail, "等待你的决定"),
                    );
                permission_rows.push(activity_row(
                    ("activity-permission", index),
                    AppIcon::Approval,
                    rgb(COLOR_WARNING),
                    agent
                        .map_or("未知任务", |agent| agent.title.as_str())
                        .to_owned(),
                    kind.to_owned(),
                    detail,
                    move |view, cx| {
                        view.select_agent(&agent_id, cx);
                        view.set_main_section(MainSection::Permissions, cx);
                    },
                    cx,
                ));
            }
        }

        if let Some(snapshot) = snapshot {
            for (index, agent) in snapshot.agents.iter().rev().enumerate() {
                if !agent_matches_activity_filter(
                    &agent.status,
                    self.activity_filter,
                    self.permissions
                        .iter()
                        .any(|permission| permission.agent_id == agent.id),
                ) {
                    continue;
                }
                let workspace = snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == agent.workspace_id);
                let agent_id = agent.id.clone();
                let label = agent_status_label(&agent.status).to_owned();
                let detail = format!(
                    "{} · {}",
                    Self::provider_label(&agent.provider),
                    workspace.map_or("未知工作区", |workspace| workspace.name.as_str()),
                );
                status_rows.push(activity_row(
                    ("activity-agent", index),
                    AppIcon::Agent,
                    agent_status_color(&agent.status),
                    agent.title.clone(),
                    label,
                    detail,
                    move |view, cx| view.select_agent(&agent_id, cx),
                    cx,
                ));
            }
        }

        for (index, turn) in self.timeline.iter().rev().enumerate() {
            if !timeline_matches_activity_filter(turn.status, self.activity_filter) {
                continue;
            }
            let agent = snapshot.and_then(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.id == turn.agent_id)
            });
            let agent_id = turn.agent_id.clone();
            let (label, color) = timeline_status(turn.status);
            let detail = compact_summary(&turn.prompt, &format!("Turn {}", turn.turn_id));
            timeline_rows.push(activity_row(
                ("activity-turn", index),
                AppIcon::Activity,
                color,
                agent
                    .map_or("未知任务", |agent| agent.title.as_str())
                    .to_owned(),
                label.to_owned(),
                detail,
                move |view, cx| view.select_agent(&agent_id, cx),
                cx,
            ));
        }

        let activity_count = permission_rows.len() + status_rows.len() + timeline_rows.len();
        let filter_buttons = [
            (ActivityFilter::All, "全部"),
            (ActivityFilter::Attention, "需要处理"),
            (ActivityFilter::Running, "运行中"),
            (ActivityFilter::Completed, "已完成"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (filter, label))| {
            Button::new(("activity-filter", index))
                .ghost()
                .small()
                .selected(self.activity_filter == filter)
                .label(label)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.set_activity_filter(filter, cx);
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
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_HEADING))
                                    .font_semibold()
                                    .child("活动"),
                            )
                            .child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("查看当前任务状态以及本次连接收到的实时事件。"),
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
                                    .child(format!("{activity_count} 条")),
                            ),
                    )
                    .when(!permission_rows.is_empty(), |panel| {
                        panel.child(activity_group("待处理", permission_rows))
                    })
                    .when(!status_rows.is_empty(), |panel| {
                        panel.child(activity_group("任务状态", status_rows))
                    })
                    .when(!timeline_rows.is_empty(), |panel| {
                        panel.child(activity_group("本次连接时间线", timeline_rows))
                    })
                    .when(activity_count == 0, |panel| {
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
                                .child(div().font_medium().child("当前没有此类活动"))
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child("运行任务后，状态和实时事件会显示在这里。"),
                                ),
                        )
                    }),
            ),
        )
    }
}

fn search_result_group(title: &'static str, rows: Vec<Button>) -> Div {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .px_2()
                .text_size(font_px(FONT_SIZE_SM))
                .font_medium()
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(title),
        )
        .children(rows)
}

fn activity_group(title: &'static str, rows: Vec<Button>) -> Div {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .px_2()
                .text_size(font_px(FONT_SIZE_SM))
                .font_medium()
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(title),
        )
        .children(rows)
}

#[allow(clippy::too_many_arguments)]
fn activity_row(
    id: (&'static str, usize),
    icon: AppIcon,
    color: gpui::Rgba,
    title: String,
    status: String,
    detail: String,
    on_open: impl Fn(&mut ConnectionView, &mut Context<ConnectionView>) + 'static,
    cx: &mut Context<ConnectionView>,
) -> Button {
    Button::new(id)
        .ghost()
        .w_full()
        .justify_start()
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
                        .rounded_lg()
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .child(Icon::new(icon).text_color(color)),
                )
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .items_start()
                        .child(
                            div()
                                .h_flex()
                                .w_full()
                                .min_w(px(0.))
                                .gap_2()
                                .child(div().min_w(px(0.)).truncate().font_medium().child(title))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .text_color(color)
                                        .child(status),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_size(font_px(FONT_SIZE_XS))
                                .text_color(rgb(COLOR_TEXT_TERTIARY))
                                .child(detail),
                        ),
                )
                .child(Icon::new(AppIcon::ChevronRight).text_color(rgb(COLOR_TEXT_TERTIARY))),
        )
        .on_click(cx.listener(move |view, _, _, cx| on_open(view, cx)))
}

fn matches_query<'a>(query: &str, values: impl IntoIterator<Item = &'a str>) -> bool {
    query.is_empty()
        || values
            .into_iter()
            .any(|value| value.to_lowercase().contains(query))
}

fn compact_summary(value: &str, fallback: &str) -> String {
    let value = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(fallback);
    let mut result = value.chars().take(96).collect::<String>();
    if value.chars().count() > 96 {
        result.push('…');
    }
    result
}

fn agent_status_label(status: &corbit_client::AgentStatus) -> &'static str {
    match status {
        corbit_client::AgentStatus::Initializing => "正在初始化",
        corbit_client::AgentStatus::Idle => "可启动",
        corbit_client::AgentStatus::Running => "运行中",
        corbit_client::AgentStatus::Error => "需要处理",
        corbit_client::AgentStatus::Stopped => "已停止",
    }
}

fn agent_status_color(status: &corbit_client::AgentStatus) -> gpui::Rgba {
    match status {
        corbit_client::AgentStatus::Running => rgb(COLOR_SUCCESS),
        corbit_client::AgentStatus::Error => rgb(COLOR_ERROR),
        corbit_client::AgentStatus::Initializing => rgb(COLOR_WARNING),
        corbit_client::AgentStatus::Idle | corbit_client::AgentStatus::Stopped => {
            rgb(COLOR_TEXT_TERTIARY)
        }
    }
}

fn agent_matches_activity_filter(
    status: &corbit_client::AgentStatus,
    filter: ActivityFilter,
    has_permission: bool,
) -> bool {
    match filter {
        ActivityFilter::All => true,
        ActivityFilter::Attention => *status == corbit_client::AgentStatus::Error || has_permission,
        ActivityFilter::Running => matches!(
            status,
            corbit_client::AgentStatus::Initializing | corbit_client::AgentStatus::Running
        ),
        ActivityFilter::Completed => matches!(
            status,
            corbit_client::AgentStatus::Idle | corbit_client::AgentStatus::Stopped
        ),
    }
}

fn timeline_matches_activity_filter(status: TimelineStatus, filter: ActivityFilter) -> bool {
    match filter {
        ActivityFilter::All => true,
        ActivityFilter::Attention => {
            matches!(status, TimelineStatus::Interrupted | TimelineStatus::Failed)
        }
        ActivityFilter::Running => status == TimelineStatus::InProgress,
        ActivityFilter::Completed => status == TimelineStatus::Completed,
    }
}

fn timeline_status(status: TimelineStatus) -> (&'static str, gpui::Rgba) {
    match status {
        TimelineStatus::InProgress => ("生成中", rgb(COLOR_SUCCESS)),
        TimelineStatus::Completed => ("已完成", rgb(COLOR_TEXT_TERTIARY)),
        TimelineStatus::Interrupted => ("已中断", rgb(COLOR_WARNING)),
        TimelineStatus::Failed => ("失败", rgb(COLOR_ERROR)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_matching_is_case_insensitive_and_cross_field() {
        assert!(matches_query("codex", ["My Task", "CODEX"]));
        assert!(matches_query(
            "workspace",
            ["Workspace Alpha", "/tmp/project"]
        ));
        assert!(!matches_query("claude", ["My Task", "codex"]));
    }

    #[test]
    fn summaries_use_first_non_empty_line_and_are_bounded() {
        assert_eq!(
            compact_summary("\n  first line\nsecond", "fallback"),
            "first line"
        );
        let long = "a".repeat(120);
        let summary = compact_summary(&long, "fallback");
        assert_eq!(summary.chars().count(), 97);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn activity_filters_keep_attention_and_completion_distinct() {
        assert!(agent_matches_activity_filter(
            &corbit_client::AgentStatus::Error,
            ActivityFilter::Attention,
            false,
        ));
        assert!(agent_matches_activity_filter(
            &corbit_client::AgentStatus::Running,
            ActivityFilter::Attention,
            true,
        ));
        assert!(!agent_matches_activity_filter(
            &corbit_client::AgentStatus::Running,
            ActivityFilter::Completed,
            false,
        ));
        assert!(timeline_matches_activity_filter(
            TimelineStatus::Completed,
            ActivityFilter::Completed,
        ));
        assert!(timeline_matches_activity_filter(
            TimelineStatus::Failed,
            ActivityFilter::Attention,
        ));
    }
}
