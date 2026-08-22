use super::*;

fn sidebar_menu_text(text: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(0.))
        .truncate()
        .text_size(font_px(SIDEBAR_FONT_SIZE))
        .child(text.into())
}

fn sidebar_row_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(rgb(COLOR_TEXT).into())
        .hover(sidebar_row_hover_rgb().into())
        .active(sidebar_row_active_rgb().into())
}

fn toggle_sidebar_project_state(collapsed_projects: &mut BTreeSet<String>, project_id: &str) {
    if !collapsed_projects.insert(project_id.to_owned()) {
        collapsed_projects.remove(project_id);
    }
}

#[cfg(target_os = "macos")]
fn file_manager_reveal_label() -> &'static str {
    "在 Finder 中显示"
}

#[cfg(target_os = "windows")]
fn file_manager_reveal_label() -> &'static str {
    "在资源管理器中显示"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn file_manager_reveal_label() -> &'static str {
    "在文件管理器中显示"
}

fn reveal_working_directory(working_directory: &str) -> Result<(), String> {
    let path = PathBuf::from(working_directory);
    if !path.is_dir() {
        return Err(format!("工作目录不存在或不是文件夹：{}", path.display()));
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(&path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        command.arg("/select,").arg(&path);
        command
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(&path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开文件管理器：{error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DeleteTarget {
    Agent(String),
    Project(String),
    Workspace(String),
}

#[derive(Clone, Debug)]
pub(super) struct RetryMutation {
    signature: String,
    client_mutation_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectCreateRequest {
    name: String,
    root_path: String,
    source_folders: Vec<String>,
}

struct ProjectCreationDialogState {
    name: Entity<InputState>,
    source_folders: Vec<PathBuf>,
    folder_picker_in_flight: bool,
    error: Option<String>,
}

fn project_source_folder_rows(
    dialog_state: &Entity<ProjectCreationDialogState>,
    folders: &[PathBuf],
) -> Vec<Div> {
    folders
        .iter()
        .enumerate()
        .map(|(index, folder)| {
            let remove_state = dialog_state.clone();
            div()
                .h_flex()
                .h(px(42.))
                .items_center()
                .gap_2()
                .px_3()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(COLOR_BORDER))
                .bg(sidebar_row_hover_rgb())
                .child(
                    Icon::new(AppIcon::Folder)
                        .size(px(16.))
                        .text_color(rgb(COLOR_TEXT_SECONDARY)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(font_px(FONT_SIZE_SM))
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child(folder.to_string_lossy().into_owned()),
                )
                .child(
                    Button::new(("new-project-remove-source", index))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(AppIcon::Close).size(px(13.)))
                        .tooltip("移除此源文件夹")
                        .on_click(move |_, _, cx| {
                            remove_state.update(cx, |state, cx| {
                                if index < state.source_folders.len() {
                                    state.source_folders.remove(index);
                                }
                                state.error = None;
                                cx.notify();
                            });
                        }),
                )
        })
        .collect()
}

fn validated_project_source_folders(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    let mut seen = BTreeSet::new();
    let mut folders = Vec::new();

    for path in paths {
        if !path.is_absolute() {
            return Err("请选择绝对路径下的项目文件夹".into());
        }

        let root_path = path.to_string_lossy().into_owned();
        if !seen.insert(root_path.clone()) {
            continue;
        }
        folders.push(path);
    }

    if folders.is_empty() {
        Err("请至少添加一个源文件夹".into())
    } else {
        Ok(folders)
    }
}

fn project_create_request(
    name: &str,
    source_folders: &[PathBuf],
) -> Result<ProjectCreateRequest, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".into());
    }

    let source_folders = validated_project_source_folders(source_folders.iter().cloned())?
        .into_iter()
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let root_path = source_folders
        .first()
        .cloned()
        .ok_or_else(|| "请至少添加一个源文件夹".to_owned())?;

    Ok(ProjectCreateRequest {
        name: name.to_owned(),
        root_path,
        source_folders,
    })
}

fn source_folder_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.to_owned(), str::to_owned)
}

fn project_name_autofill(current_name: &str, source_folders: &[PathBuf]) -> Option<String> {
    if !current_name.trim().is_empty() {
        return None;
    }

    source_folders.first().map(|folder| {
        let path = folder.to_string_lossy();
        source_folder_name(path.as_ref())
    })
}

struct ProjectCreationResult {
    project_id: String,
    first_workspace_id: Option<String>,
    snapshot: corbit_client::AuthoritativeSnapshot,
    created_sources: usize,
    source_count: usize,
    failure: Option<(String, String)>,
}

async fn create_project_resources(
    client: corbit_client::DaemonRuntimeClient,
    project: ProjectCreateRequest,
) -> Result<ProjectCreationResult, String> {
    let source_count = project.source_folders.len();
    let (project_acknowledgement, mut snapshot) = client
        .mutate_and_snapshot(
            "project.create",
            json!({
                "name": project.name,
                "rootPath": project.root_path,
                "clientMutationId": format!("mutation_{}", uuid::Uuid::new_v4()),
            }),
        )
        .await
        .map_err(|error| error.to_string())?;
    let project_id = project_acknowledgement.resource_id;
    let mut created_sources = 0usize;
    let mut first_workspace_id = None;
    let mut failure = None;

    for source_folder in project.source_folders {
        let workspace_name = source_folder_name(&source_folder);
        match client
            .mutate_and_snapshot(
                "workspace.create",
                json!({
                    "projectId": project_id,
                    "name": workspace_name,
                    "workingDirectory": source_folder,
                    "clientMutationId": format!("mutation_{}", uuid::Uuid::new_v4()),
                }),
            )
            .await
        {
            Ok((acknowledgement, latest_snapshot)) => {
                created_sources += 1;
                if first_workspace_id.is_none() {
                    first_workspace_id = Some(acknowledgement.resource_id);
                }
                snapshot = latest_snapshot;
            }
            Err(error) => {
                failure = Some((workspace_name, error.to_string()));
                break;
            }
        }
    }

    Ok(ProjectCreationResult {
        project_id,
        first_workspace_id,
        snapshot,
        created_sources,
        source_count,
        failure,
    })
}

impl ConnectionView {
    pub(super) fn reconcile_selection(&mut self) {
        let previous_workspace_id = self.selected_workspace_id.clone();
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        self.project_providers.retain(|project_id, _| {
            snapshot
                .projects
                .iter()
                .any(|project| project.id == *project_id)
        });
        let selected_project_id = self
            .selected_project_id
            .as_ref()
            .filter(|id| snapshot.projects.iter().any(|project| &project.id == *id))
            .cloned()
            .or_else(|| snapshot.projects.first().map(|project| project.id.clone()));
        self.selected_project_id.clone_from(&selected_project_id);

        self.selected_workspace_id = selected_project_id.and_then(|project_id| {
            self.selected_workspace_id
                .as_ref()
                .filter(|id| {
                    snapshot
                        .workspaces
                        .iter()
                        .any(|workspace| &workspace.id == *id && workspace.project_id == project_id)
                })
                .cloned()
                .or_else(|| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.project_id == project_id)
                        .map(|workspace| workspace.id.clone())
                })
        });
        self.selected_agent_id =
            self.selected_workspace_id
                .as_ref()
                .and_then(|workspace_id| {
                    self.selected_agent_id
                        .as_ref()
                        .filter(|id| {
                            snapshot.agents.iter().any(|agent| {
                                &agent.id == *id && &agent.workspace_id == workspace_id
                            })
                        })
                        .cloned()
                        .or_else(|| {
                            snapshot
                                .agents
                                .iter()
                                .find(|agent| &agent.workspace_id == workspace_id)
                                .map(|agent| agent.id.clone())
                        })
                });
        if self.main_section == MainSection::Conversation && self.selected_agent_id.is_none() {
            self.main_section = MainSection::NewTask;
        }
        if self.selected_workspace_id != previous_workspace_id {
            self.clear_workspace_files();
            self.clear_workspace_git();
        }
        self.reconcile_composer_catalog();
    }

    pub(super) fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let previous_workspace_id = self.selected_workspace_id.clone();
        self.selected_project_id = Some(project_id.to_owned());
        self.selected_workspace_id = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.project_id == project_id)
                .map(|workspace| workspace.id.clone())
        });
        self.selected_agent_id = self
            .selected_workspace_id
            .as_ref()
            .and_then(|workspace_id| {
                self.snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .find(|agent| &agent.workspace_id == workspace_id)
                        .map(|agent| agent.id.clone())
                })
            });
        if self.selected_workspace_id != previous_workspace_id {
            self.clear_workspace_files();
            self.clear_workspace_git();
        }
        self.main_section = if self.selected_agent_id.is_some() {
            MainSection::Conversation
        } else {
            MainSection::NewTask
        };
        self.delete_confirmation = None;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn open_new_task_for_project(
        &mut self,
        project_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project_name, workspace_id)) = self.snapshot.as_ref().and_then(|snapshot| {
            let project = snapshot
                .projects
                .iter()
                .find(|project| project.id == project_id)?;
            let workspace_id = snapshot
                .workspaces
                .iter()
                .find(|workspace| {
                    workspace.project_id == project.id
                        && workspace.status == corbit_client::WorkspaceStatus::Active
                })
                .map(|workspace| workspace.id.clone());
            Some((project.name.clone(), workspace_id))
        }) else {
            self.show_validation_error("该项目已不存在，请刷新后重试", cx);
            return;
        };

        let workspace_changed = self.selected_workspace_id != workspace_id;
        self.selected_project_id = Some(project_id.to_owned());
        self.selected_workspace_id = workspace_id;
        self.selected_agent_id = None;
        if workspace_changed {
            self.clear_workspace_files();
            self.clear_workspace_git();
        }
        self.main_section = MainSection::NewTask;
        self.delete_confirmation = None;
        self.detail = format!("已选择项目“{project_name}”，请描述要完成的任务");
        self.new_task_prompt
            .update(cx, |input, cx| input.focus(window, cx));
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    pub(super) fn select_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let workspace_changed = self.selected_workspace_id.as_deref() != Some(workspace_id);
        if let Some(project_id) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
                .map(|workspace| workspace.project_id.clone())
        }) {
            self.selected_project_id = Some(project_id);
        }
        self.selected_workspace_id = Some(workspace_id.to_owned());
        self.selected_agent_id = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.workspace_id == workspace_id)
                .map(|agent| agent.id.clone())
        });
        if workspace_changed {
            self.clear_workspace_files();
            self.clear_workspace_git();
        }
        self.main_section = if self.selected_agent_id.is_some() {
            MainSection::Conversation
        } else {
            MainSection::NewTask
        };
        self.delete_confirmation = None;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    pub(super) fn select_agent(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let selection = self.snapshot.as_ref().and_then(|snapshot| {
            let agent = snapshot.agents.iter().find(|agent| agent.id == agent_id)?;
            let project_id = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.id == agent.workspace_id)
                .map(|workspace| workspace.project_id.clone());
            Some((agent.workspace_id.clone(), project_id))
        });
        if let Some((workspace_id, project_id)) = selection {
            if self.selected_workspace_id.as_ref() != Some(&workspace_id) {
                self.clear_workspace_files();
                self.clear_workspace_git();
            }
            self.selected_workspace_id = Some(workspace_id);
            if let Some(project_id) = project_id {
                self.selected_project_id = Some(project_id);
            }
        }
        self.selected_agent_id = Some(agent_id.to_owned());
        self.main_section = MainSection::Conversation;
        self.delete_confirmation = None;
        self.reset_timeline_list_to_selected();
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn select_project_in_settings(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.select_project(project_id, cx);
        self.resource_section = ResourceSection::Projects;
        self.main_section = MainSection::Resources;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn select_workspace_in_settings(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.select_workspace(workspace_id, cx);
        self.resource_section = ResourceSection::Workspaces;
        self.main_section = MainSection::Resources;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    pub(super) fn select_agent_in_settings(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        self.select_agent(agent_id, cx);
        self.resource_section = ResourceSection::Agents;
        self.main_section = MainSection::Resources;
        self.schedule_ui_state_save(cx);
        cx.notify();
    }

    fn toggle_sidebar_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        toggle_sidebar_project_state(&mut self.collapsed_sidebar_projects, project_id);
        cx.notify();
    }

    fn project_popup_menu(
        menu: PopupMenu,
        view: Entity<Self>,
        project_id: String,
        project_name: String,
        can_mutate: bool,
    ) -> PopupMenu {
        let new_task_view = view.clone();
        let new_task_project_id = project_id.clone();
        let settings_view = view.clone();
        let settings_project_id = project_id.clone();
        let rename_view = view.clone();
        let rename_project_id = project_id.clone();
        let rename_project_name = project_name.clone();
        let delete_view = view;
        let delete_project_id = project_id;
        let delete_project_name = project_name;

        menu.min_w(px(188.))
            .item(
                PopupMenuItem::new("新建任务")
                    .icon(Icon::new(AppIcon::Add))
                    .disabled(!can_mutate)
                    .on_click(move |_, window, cx| {
                        new_task_view.update(cx, |view, cx| {
                            view.open_new_task_for_project(&new_task_project_id, window, cx);
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("项目设置")
                    .icon(Icon::new(AppIcon::Settings))
                    .on_click(move |_, _, cx| {
                        settings_view.update(cx, |view, cx| {
                            view.select_project_in_settings(&settings_project_id, cx);
                        });
                    }),
            )
            .item(PopupMenuItem::separator())
            .item(
                PopupMenuItem::new("重命名项目")
                    .icon(Icon::new(AppIcon::Rename))
                    .disabled(!can_mutate)
                    .on_click(move |_, window, cx| {
                        Self::open_project_rename_dialog(
                            rename_view.clone(),
                            rename_project_id.clone(),
                            rename_project_name.clone(),
                            window,
                            cx,
                        );
                    }),
            )
            .item(
                PopupMenuItem::new("删除项目")
                    .icon(Icon::new(AppIcon::Delete).text_color(rgb(COLOR_ERROR)))
                    .disabled(!can_mutate)
                    .on_click(move |_, window, cx| {
                        Self::open_project_delete_dialog(
                            delete_view.clone(),
                            delete_project_id.clone(),
                            delete_project_name.clone(),
                            window,
                            cx,
                        );
                    }),
            )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_sidebar(
        &self,
        connection_status: &'static str,
        is_connecting: bool,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let sidebar_view = cx.entity();
        let project_groups = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .projects
                    .iter()
                    .enumerate()
                    .map(|(index, project)| {
                        let is_expanded = !self.collapsed_sidebar_projects.contains(&project.id);
                        let row_project_id = project.id.clone();
                        let project_name = project.name.clone();
                        let group_name: SharedString = format!("sidebar-project-{index}").into();
                        let menu_view = sidebar_view.clone();
                        let menu_project_id = project.id.clone();
                        let menu_project_name = project.name.clone();
                        let context_menu_view = sidebar_view.clone();
                        let context_menu_project_id = project.id.clone();
                        let context_menu_project_name = project.name.clone();
                        let can_mutate = is_online && !self.operation_in_flight;
                        let agent_rows = snapshot
                            .agents
                            .iter()
                            .enumerate()
                            .filter_map(|(agent_index, agent)| {
                                snapshot
                                    .workspaces
                                    .iter()
                                    .find(|workspace| {
                                        workspace.project_id == project.id
                                            && workspace.id == agent.workspace_id
                                    })
                                    .map(|workspace| (agent_index, agent, workspace))
                            })
                            .map(|(agent_index, agent, workspace)| {
                                let agent_id = agent.id.clone();
                                let agent_title = agent.title.clone();
                                let menu_agent_id = agent.id.clone();
                                let menu_agent_title = agent.title.clone();
                                let menu_workspace_id = workspace.id.clone();
                                let menu_project_id = project.id.clone();
                                let working_directory = workspace.working_directory.clone();
                                let menu_view = sidebar_view.clone();

                                Button::new(("sidebar-agent", agent_index))
                                    .custom(sidebar_row_variant(cx))
                                    .small()
                                    .rounded(px(8.))
                                    .h(navigation_row_height())
                                    .w_full()
                                    .pl_8()
                                    .pr_3()
                                    .justify_start()
                                    .text_color(rgb(COLOR_TEXT))
                                    .selected(self.selected_agent_id.as_ref() == Some(&agent.id))
                                    .child(sidebar_menu_text(agent_title.clone()))
                                    .tooltip(agent_title)
                                    .disabled(self.operation_in_flight)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.select_agent(&agent_id, cx);
                                    }))
                                    .context_menu(move |menu, _, _| {
                                        let rename_view = menu_view.clone();
                                        let rename_agent_id = menu_agent_id.clone();
                                        let rename_agent_title = menu_agent_title.clone();
                                        let reveal_view = menu_view.clone();
                                        let reveal_directory = working_directory.clone();
                                        let copy_directory_view = menu_view.clone();
                                        let copy_directory = working_directory.clone();
                                        let copy_id_view = menu_view.clone();
                                        let copy_agent_id = menu_agent_id.clone();
                                        let new_window_view = menu_view.clone();
                                        let new_window_agent_id = menu_agent_id.clone();
                                        let new_window_workspace_id = menu_workspace_id.clone();
                                        let new_window_project_id = menu_project_id.clone();

                                        menu.item(
                                            PopupMenuItem::new("重命名任务")
                                                .disabled(!can_mutate)
                                                .on_click(move |_, window, cx| {
                                                    Self::open_agent_rename_dialog(
                                                        rename_view.clone(),
                                                        rename_agent_id.clone(),
                                                        rename_agent_title.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                        )
                                        .item(PopupMenuItem::separator())
                                        .item(
                                            PopupMenuItem::new(file_manager_reveal_label())
                                                .on_click(move |_, _, cx| {
                                                    let result =
                                                        reveal_working_directory(&reveal_directory);
                                                    reveal_view.update(
                                                        cx,
                                                        |view, cx| match result {
                                                            Ok(()) => view.show_success(
                                                                "已在文件管理器中显示工作目录",
                                                                cx,
                                                            ),
                                                            Err(error) => {
                                                                view.show_error(error, cx);
                                                            }
                                                        },
                                                    );
                                                }),
                                        )
                                        .item(PopupMenuItem::new("复制工作目录").on_click(
                                            move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    copy_directory.clone(),
                                                ));
                                                copy_directory_view.update(cx, |view, cx| {
                                                    view.show_success("已复制工作目录", cx);
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::new("复制会话 ID").on_click(
                                            move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    copy_agent_id.clone(),
                                                ));
                                                copy_id_view.update(cx, |view, cx| {
                                                    view.show_success("已复制会话 ID", cx);
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::separator())
                                        .item(
                                            PopupMenuItem::new("在新窗口中打开").on_click(
                                                move |_, _, cx| {
                                                    Self::open_agent_in_new_window(
                                                        &new_window_view,
                                                        new_window_project_id.clone(),
                                                        new_window_workspace_id.clone(),
                                                        new_window_agent_id.clone(),
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
                                    })
                            })
                            .collect::<Vec<_>>();

                        div()
                            .v_flex()
                            .w_full()
                            .child(
                                div()
                                    .relative()
                                    .group(group_name.clone())
                                    .w_full()
                                    .child(
                                        Button::new(("sidebar-project", index))
                                            .custom(sidebar_row_variant(cx))
                                            .small()
                                            .rounded(px(8.))
                                            .h(navigation_row_height())
                                            .w_full()
                                            .pr_8()
                                            .justify_start()
                                            .text_color(rgb(COLOR_TEXT))
                                            .icon(
                                                Icon::new(if is_expanded {
                                                    AppIcon::FolderOpen
                                                } else {
                                                    AppIcon::Project
                                                })
                                                .size(px(16.)),
                                            )
                                            .child(sidebar_menu_text(project_name))
                                            .tooltip(if is_expanded {
                                                "折叠项目任务"
                                            } else {
                                                "展开项目任务"
                                            })
                                            .disabled(self.operation_in_flight)
                                            .on_click(cx.listener(move |view, _, _, cx| {
                                                view.toggle_sidebar_project(&row_project_id, cx);
                                            }))
                                            .context_menu(move |menu, _, _| {
                                                Self::project_popup_menu(
                                                    menu,
                                                    context_menu_view.clone(),
                                                    context_menu_project_id.clone(),
                                                    context_menu_project_name.clone(),
                                                    can_mutate,
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(3.))
                                            .right_1()
                                            .invisible()
                                            .group_hover(group_name, gpui::Styled::visible)
                                            .child(
                                                Button::new(("sidebar-project-more", index))
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(Icon::new(AppIcon::More))
                                                    .disabled(self.operation_in_flight)
                                                    .dropdown_menu_with_anchor(
                                                        Corner::TopRight,
                                                        move |menu, _, _| {
                                                            Self::project_popup_menu(
                                                                menu,
                                                                menu_view.clone(),
                                                                menu_project_id.clone(),
                                                                menu_project_name.clone(),
                                                                can_mutate,
                                                            )
                                                        },
                                                    ),
                                            ),
                                    ),
                            )
                            .when(is_expanded, |group| group.children(agent_rows))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let online_color = if is_online {
            rgb(COLOR_SUCCESS)
        } else {
            rgb(COLOR_TEXT_TERTIARY)
        };

        div()
            .v_flex()
            .w_full()
            .h_full()
            .border_r_1()
            .border_color(sidebar_border_rgb())
            .bg(sidebar_rgb())
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .h(px(TOOLBAR_HEIGHT))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .pl(px(TITLEBAR_LEFT_PADDING))
                    .pr_2()
                    .when(build_info::is_development(), |toolbar| {
                        toolbar.child(
                            div()
                                .flex_none()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(rgb(COLOR_BORDER_HEAVY))
                                .bg(rgb(COLOR_SURFACE_SECONDARY))
                                .px_1p5()
                                .py_0p5()
                                .text_size(font_px(FONT_SIZE_XS))
                                .font_semibold()
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child("DEV"),
                        )
                    })
                    .child(
                        Button::new("sidebar-collapse")
                            .ghost()
                            .small()
                            .icon(Icon::new(AppIcon::PanelLeftClose))
                            .tooltip("折叠侧栏")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.toggle_sidebar(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap_1()
                    .pl_1()
                    .pr_2()
                    .pb_2()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .mb_3()
                            .child(
                                Button::new("sidebar-new-task")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::NewTask)
                                    .icon(Icon::new(AppIcon::Add))
                                    .child(sidebar_menu_text("新建任务"))
                                    .tooltip("新建任务 · ⌘N")
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::NewTask, cx);
                                    })),
                            )
                            .child(
                                Button::new("sidebar-search")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::Search)
                                    .icon(Icon::new(AppIcon::Search))
                                    .child(sidebar_menu_text("搜索"))
                                    .tooltip("搜索 · ⌘K")
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.set_main_section(MainSection::Search, cx);
                                        view.search_input
                                            .update(cx, |input, cx| input.focus(window, cx));
                                    })),
                            )
                            .child(
                                Button::new("sidebar-tasks")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::Tasks)
                                    .icon(Icon::new(AppIcon::Tasks))
                                    .child(sidebar_menu_text("任务"))
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::Tasks, cx);
                                    })),
                            )
                            .child(
                                Button::new("sidebar-activity")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::Activity)
                                    .icon(Icon::new(AppIcon::Activity))
                                    .child(sidebar_menu_text(
                                        if self.activity_attention_count() == 0 {
                                            "活动".to_owned()
                                        } else {
                                            format!("活动  {}", self.activity_attention_count())
                                        },
                                    ))
                                    .tooltip("活动 · ⇧⌘A")
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::Activity, cx);
                                    })),
                            )
                            .child(
                                Button::new("sidebar-permissions")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::Permissions)
                                    .icon(Icon::new(AppIcon::Approval))
                                    .child(sidebar_menu_text(if self.permissions.is_empty() {
                                        "审批".to_owned()
                                    } else {
                                        format!("审批  {}", self.permissions.len())
                                    }))
                                    .disabled(!is_online)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::Permissions, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .pl_2()
                                    .pr_1()
                                    .pt_1()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(font_px(FONT_SIZE_SM))
                                            .font_medium()
                                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                                            .child("项目"),
                                    )
                                    .child(
                                        Button::new("sidebar-add-project")
                                            .ghost()
                                            .small()
                                            .icon(Icon::new(AppIcon::Add))
                                            .tooltip(if is_online {
                                                "新建项目"
                                            } else {
                                                "连接 Daemon 后新建项目"
                                            })
                                            .disabled(
                                                !is_online
                                                    || self.snapshot.is_none()
                                                    || self.operation_in_flight,
                                            )
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.add_project_from_sidebar(window, cx);
                                            })),
                                    ),
                            )
                            .when(project_groups.is_empty(), |group| {
                                group.child(
                                    div()
                                        .px_2()
                                        .py_2()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                                        .child(if is_online {
                                            "暂无项目"
                                        } else {
                                            "连接后显示项目"
                                        }),
                                )
                            })
                            .children(project_groups),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .flex_none()
                    .border_t_1()
                    .border_color(sidebar_border_rgb())
                    .pl_1()
                    .pr_2()
                    .py_2()
                    .gap_1()
                    .child(
                        div()
                            .h_flex()
                            .h(navigation_row_height())
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("sidebar-manage")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .flex_1()
                                    .justify_start()
                                    .selected(self.main_section == MainSection::Resources)
                                    .icon(Icon::new(AppIcon::Settings))
                                    .child(sidebar_menu_text("设置"))
                                    .tooltip("设置 · ⌘,")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.set_main_section(MainSection::Resources, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1p5()
                                    .pr_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .child(div().size(px(7.)).rounded_full().bg(online_color))
                                    .child(connection_status),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .h(px(28.))
                            .items_center()
                            .gap_2()
                            .px_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(font_px(FONT_SIZE_XS))
                                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                                    .child(self.detail.clone()),
                            )
                            .child(
                                Button::new("sidebar-connect")
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(AppIcon::Refresh))
                                    .tooltip("重新连接 Daemon")
                                    .loading(is_connecting)
                                    .disabled(is_online || is_connecting)
                                    .on_click(cx.listener(|view, _, _, cx| view.connect(cx))),
                            ),
                    ),
            )
    }

    fn add_project_from_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, corbit_client::ConnectionState::Online) || self.snapshot.is_none()
        {
            self.show_validation_error("请等待 Daemon 连接并完成状态同步", cx);
            return;
        }
        if self.operation_in_flight {
            self.show_warning("另一个资源操作正在执行，请稍后再试", cx);
            return;
        }

        if !connection::is_loopback_endpoint(&self.daemon_endpoint) {
            self.resource_section = ResourceSection::Projects;
            self.set_main_section(MainSection::Resources, cx);
            self.show_info(
                "当前连接的是远程 Daemon，请填写远程主机上的项目名称和绝对根目录",
                cx,
            );
            return;
        }

        Self::open_project_creation_dialog(cx.entity(), window, cx);
    }

    #[allow(clippy::too_many_lines)]
    fn open_project_creation_dialog(
        view: Entity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("项目名称"));
        let dialog_state = cx.new(|_| ProjectCreationDialogState {
            name,
            source_folders: Vec::new(),
            folder_picker_in_flight: false,
            error: None,
        });

        window.open_dialog(cx, move |dialog, _window, cx| {
            let (name_input, source_folders, folder_picker_in_flight, error) = {
                let state = dialog_state.read(cx);
                (
                    state.name.clone(),
                    state.source_folders.clone(),
                    state.folder_picker_in_flight,
                    state.error.clone(),
                )
            };

            let folder_rows = project_source_folder_rows(&dialog_state, &source_folders);
            let picker_state = dialog_state.clone();
            let picker_height = if source_folders.is_empty() {
                px(152.)
            } else {
                px(84.)
            };
            let picker_variant = ButtonCustomVariant::new(cx)
                .color(sidebar_row_hover_rgb().into())
                .foreground(rgb(COLOR_TEXT_SECONDARY).into())
                .border(rgb(COLOR_BORDER).into())
                .hover(sidebar_row_active_rgb().into())
                .active(sidebar_row_active_rgb().into());

            let footer_state = dialog_state.clone();
            let footer_view = view.clone();
            dialog
                .title(
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child("创建项目")
                        .child(
                            Button::new("new-project-close")
                                .ghost()
                                .small()
                                .icon(Icon::new(AppIcon::Close))
                                .tooltip("关闭")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                )
                .w(px(700.))
                .max_w(px(760.))
                .bg(rgb(COLOR_EDITOR))
                .close_button(false)
                .overlay_closable(false)
                .keyboard(false)
                .footer(move |_, _, _, cx| {
                    let can_create = {
                        let state = footer_state.read(cx);
                        !state.name.read(cx).value().trim().is_empty()
                            && !state.source_folders.is_empty()
                            && !state.folder_picker_in_flight
                            && !footer_view.read(cx).operation_in_flight
                    };

                    let create_state = footer_state.clone();
                    let create_view = footer_view.clone();
                    vec![
                        Button::new("new-project-cancel")
                            .ghost()
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        Button::new("new-project-create")
                            .primary()
                            .label("创建项目")
                            .disabled(!can_create)
                            .on_click(move |_, window, cx| {
                                let request = {
                                    let state = create_state.read(cx);
                                    project_create_request(
                                        state.name.read(cx).value().as_ref(),
                                        &state.source_folders,
                                    )
                                };

                                let request = match request {
                                    Ok(request) => request,
                                    Err(message) => {
                                        create_state.update(cx, |state, cx| {
                                            state.error = Some(message);
                                            cx.notify();
                                        });
                                        return;
                                    }
                                };

                                let started = create_view.update(cx, |view, cx| {
                                    view.create_project_with_sources(request, cx)
                                });
                                if started {
                                    window.close_dialog(cx);
                                }
                            }),
                    ]
                })
                .child(
                    div()
                        .v_flex()
                        .gap_4()
                        .child(
                            Input::new(&name_input).large().prefix(
                                Icon::new(AppIcon::Folder)
                                    .size(px(18.))
                                    .text_color(rgb(COLOR_TEXT_SECONDARY)),
                            ),
                        )
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_BASE))
                                        .font_medium()
                                        .child("源文件夹"),
                                )
                                .when(!folder_rows.is_empty(), |section| {
                                    section.child(
                                        div()
                                            .v_flex()
                                            .gap_2()
                                            .max_h(px(180.))
                                            .overflow_y_scrollbar()
                                            .children(folder_rows),
                                    )
                                })
                                .child(
                                    Button::new("new-project-add-source-folders")
                                        .custom(picker_variant)
                                        .w_full()
                                        .h(picker_height)
                                        .rounded(px(8.))
                                        .loading(folder_picker_in_flight)
                                        .disabled(folder_picker_in_flight)
                                        .child(
                                            div()
                                                .v_flex()
                                                .items_center()
                                                .justify_center()
                                                .gap_2()
                                                .child(
                                                    Icon::new(AppIcon::FolderOpen)
                                                        .size(px(22.))
                                                        .text_color(rgb(COLOR_TEXT_SECONDARY)),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(font_px(FONT_SIZE_BASE))
                                                        .font_medium()
                                                        .child("添加 Corbit 可读取和编辑的文件夹"),
                                                ),
                                        )
                                        .on_click(move |_, window, cx| {
                                            Self::select_project_source_folders(
                                                picker_state.clone(),
                                                window,
                                                cx,
                                            );
                                        }),
                                ),
                        )
                        .when_some(error, |content, message| {
                            content.child(
                                div()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .text_color(rgb(COLOR_ERROR))
                                    .child(message),
                            )
                        }),
                )
        });
    }

    fn select_project_source_folders(
        dialog_state: Entity<ProjectCreationDialogState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if dialog_state.read(cx).folder_picker_in_flight {
            return;
        }

        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("选择源文件夹".into()),
        });
        let window_handle = window.window_handle();
        dialog_state.update(cx, |state, cx| {
            state.folder_picker_in_flight = true;
            state.error = None;
            cx.notify();
        });

        cx.spawn(async move |cx| {
            let selection = path_prompt.await;
            let name_autofill = dialog_state.update(cx, |state, cx| {
                state.folder_picker_in_flight = false;
                let selected_folders = match selection {
                    Ok(Ok(Some(paths))) => match validated_project_source_folders(paths) {
                        Ok(paths) => {
                            let mut seen = state
                                .source_folders
                                .iter()
                                .map(|path| path.to_string_lossy().into_owned())
                                .collect::<BTreeSet<_>>();
                            for path in paths {
                                if seen.insert(path.to_string_lossy().into_owned()) {
                                    state.source_folders.push(path);
                                }
                            }
                            true
                        }
                        Err(message) => {
                            state.error = Some(message);
                            false
                        }
                    },
                    Ok(Ok(None)) => false,
                    Ok(Err(error)) => {
                        state.error = Some(format!("无法打开目录选择器：{error}"));
                        false
                    }
                    Err(_) => {
                        state.error = Some("目录选择器意外关闭，请重试".into());
                        false
                    }
                };
                let name_autofill = selected_folders.then(|| {
                    let name = state.name.read(cx).value();
                    project_name_autofill(name.as_ref(), &state.source_folders)
                        .map(|name| (state.name.clone(), name))
                });
                cx.notify();
                name_autofill.flatten()
            });
            if let Ok(Some((name_input, name))) = name_autofill {
                let _ = cx.update_window(window_handle, move |_, window, cx| {
                    name_input.update(cx, |input, cx| input.set_value(name, window, cx));
                });
            }
        })
        .detach();
    }

    fn open_project_rename_dialog(
        view: Entity<Self>,
        project_id: String,
        project_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("输入项目名称")
                .default_value(project_name.clone())
        });

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let ok_view = view.clone();
            let ok_project_id = project_id.clone();
            let ok_project_name = project_name.clone();
            let ok_name_input = name_input.clone();

            dialog
                .title("重命名项目")
                .w(px(480.))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("保存")
                        .cancel_text("取消"),
                )
                .on_ok(move |_, window, cx| {
                    let name = ok_name_input.read(cx).value().trim().to_owned();
                    if name.is_empty() {
                        ok_name_input.update(cx, |input, cx| input.focus(window, cx));
                        push_app_notification(FeedbackKind::Error, "项目名称不能为空", window, cx);
                        return false;
                    }
                    if ok_view.read(cx).operation_in_flight {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "另一个资源操作正在执行，请稍后再试",
                            window,
                            cx,
                        );
                        return false;
                    }

                    let project_exists =
                        ok_view.read(cx).snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot
                                .projects
                                .iter()
                                .any(|project| project.id == ok_project_id)
                        });
                    if !project_exists {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "该项目已不存在，无法重命名",
                            window,
                            cx,
                        );
                        return true;
                    }
                    if name == ok_project_name {
                        push_app_notification(FeedbackKind::Info, "项目名称未发生变化", window, cx);
                        return true;
                    }

                    ok_view.update(cx, |view, cx| {
                        view.rename_project_to(&ok_project_id, &name, cx);
                    });
                    true
                })
                .child(
                    div()
                        .v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(format!("为“{project_name}”设置新的显示名称。")),
                        )
                        .child(Input::new(&name_input).small()),
                )
        });
    }

    fn open_agent_rename_dialog(
        view: Entity<Self>,
        agent_id: String,
        agent_title: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let title_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("输入任务名称")
                .default_value(agent_title.clone())
        });

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let ok_view = view.clone();
            let ok_agent_id = agent_id.clone();
            let ok_agent_title = agent_title.clone();
            let ok_title_input = title_input.clone();

            dialog
                .title("重命名任务")
                .w(px(480.))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("保存")
                        .cancel_text("取消"),
                )
                .on_ok(move |_, window, cx| {
                    let title = ok_title_input.read(cx).value().trim().to_owned();
                    if title.is_empty() {
                        ok_title_input.update(cx, |input, cx| input.focus(window, cx));
                        push_app_notification(FeedbackKind::Error, "任务名称不能为空", window, cx);
                        return false;
                    }
                    if ok_view.read(cx).operation_in_flight {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "另一个资源操作正在执行，请稍后再试",
                            window,
                            cx,
                        );
                        return false;
                    }

                    let agent_exists = ok_view.read(cx).snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.agents.iter().any(|agent| agent.id == ok_agent_id)
                    });
                    if !agent_exists {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "该任务已不存在，无法重命名",
                            window,
                            cx,
                        );
                        return true;
                    }
                    if title == ok_agent_title {
                        push_app_notification(FeedbackKind::Info, "任务名称未发生变化", window, cx);
                        return true;
                    }

                    ok_view.update(cx, |view, cx| {
                        view.rename_agent_to(&ok_agent_id, &title, cx);
                    });
                    true
                })
                .child(
                    div()
                        .v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child(format!("为“{agent_title}”设置新的显示名称。")),
                        )
                        .child(Input::new(&title_input).small()),
                )
        });
    }

    fn open_agent_in_new_window(
        view: &Entity<Self>,
        project_id: String,
        workspace_id: String,
        agent_id: String,
        cx: &mut App,
    ) {
        let (appearance, mut preferences) = {
            let current_view = view.read(cx);
            (current_view.appearance, current_view.ui_preferences(cx))
        };
        preferences.main_section = MainSection::Conversation;
        preferences.settings_return_section = MainSection::Conversation;
        preferences.selected_project_id = Some(project_id);
        preferences.selected_workspace_id = Some(workspace_id);
        preferences.selected_agent_id = Some(agent_id);
        preferences.window = None;

        let result = cx.open_window(
            corbit_window_options(appearance, None),
            move |window, cx| {
                let connection_view =
                    cx.new(|cx| ConnectionView::new(window, cx, appearance, preferences));
                cx.new(|cx| Root::new(connection_view, window, cx))
            },
        );
        view.update(cx, |view, cx| match result {
            Ok(_) => view.show_success("已在新窗口中打开任务", cx),
            Err(error) => view.show_error(format!("无法打开新窗口：{error}"), cx),
        });
    }

    fn open_project_delete_dialog(
        view: Entity<Self>,
        project_id: String,
        project_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let status = view.read(cx).project_workspace_status(&project_id);
        let Some((project_exists, workspace_count)) = status else {
            push_app_notification(FeedbackKind::Warning, "项目状态尚未同步", window, cx);
            return;
        };
        if !project_exists {
            push_app_notification(
                FeedbackKind::Warning,
                "该项目已不存在，无法删除",
                window,
                cx,
            );
            return;
        }
        if workspace_count > 0 {
            let message =
                format!("无法删除“{project_name}”：请先删除项目下的 {workspace_count} 个工作区");
            view.update(cx, |view, cx| {
                message.clone_into(&mut view.detail);
                cx.notify();
            });
            push_app_notification(FeedbackKind::Warning, message, window, cx);
            return;
        }

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let ok_view = view.clone();
            let ok_project_id = project_id.clone();
            let ok_project_name = project_name.clone();

            dialog
                .title("删除项目？")
                .w(px(480.))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除项目")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消"),
                )
                .on_ok(move |_, window, cx| {
                    if ok_view.read(cx).operation_in_flight {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "另一个资源操作正在执行，请稍后再试",
                            window,
                            cx,
                        );
                        return false;
                    }

                    let Some((project_exists, workspace_count)) = ok_view
                        .read(cx)
                        .project_workspace_status(&ok_project_id)
                    else {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "项目状态尚未同步",
                            window,
                            cx,
                        );
                        return false;
                    };
                    if !project_exists {
                        push_app_notification(
                            FeedbackKind::Warning,
                            "该项目已不存在，无需再次删除",
                            window,
                            cx,
                        );
                        return true;
                    }
                    if workspace_count > 0 {
                        push_app_notification(
                            FeedbackKind::Warning,
                            format!(
                                "无法删除“{ok_project_name}”：请先删除项目下的 {workspace_count} 个工作区"
                            ),
                            window,
                            cx,
                        );
                        return false;
                    }

                    ok_view.update(cx, |view, cx| {
                        view.delete_project(&ok_project_id, cx);
                    });
                    true
                })
                .child(
                    div()
                        .v_flex()
                        .gap_2()
                        .child(format!(
                            "将从 Corbit 中删除项目“{project_name}”。此操作无法撤销。"
                        ))
                        .child(
                            div()
                                .text_size(font_px(FONT_SIZE_SM))
                                .text_color(rgb(COLOR_TEXT_SECONDARY))
                                .child("磁盘上的项目文件夹和代码不会被删除。"),
                        ),
                )
        });
    }

    fn project_workspace_status(&self, project_id: &str) -> Option<(bool, usize)> {
        let snapshot = self.snapshot.as_ref()?;
        Some((
            snapshot
                .projects
                .iter()
                .any(|project| project.id == project_id),
            snapshot
                .workspaces
                .iter()
                .filter(|workspace| workspace.project_id == project_id)
                .count(),
        ))
    }

    fn create_project_with_sources(
        &mut self,
        project: ProjectCreateRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.operation_in_flight {
            self.show_warning("另一个资源操作正在执行，请稍后再试", cx);
            return false;
        }
        if project.name.trim().is_empty() || project.source_folders.is_empty() {
            self.show_validation_error("请输入项目名称并至少添加一个源文件夹", cx);
            return false;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return false;
        };
        if !matches!(self.state, corbit_client::ConnectionState::Online) || self.snapshot.is_none()
        {
            self.show_validation_error("请等待 Daemon 完成权威状态同步", cx);
            return false;
        }

        let project_name = project.name.clone();
        self.operation_in_flight = true;
        self.retry_mutation = None;
        self.delete_confirmation = None;
        self.detail = format!("正在创建项目“{project_name}”…");
        self.mutation_task = Some(cx.spawn(async move |view, cx| {
            let creation = match create_project_resources(client, project).await {
                Ok(creation) => creation,
                Err(error) => {
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    let _ = view.update(cx, |view, cx| {
                        view.operation_in_flight = false;
                        view.show_error(format!("项目“{project_name}”创建失败：{error}"), cx);
                    });
                    return;
                }
            };

            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.operation_in_flight = false;
                let revision = creation.snapshot.revision;
                view.snapshot = Some(creation.snapshot);
                view.selected_project_id = Some(creation.project_id);
                view.selected_workspace_id = creation.first_workspace_id;
                view.selected_agent_id = None;
                view.clear_workspace_files();
                view.clear_workspace_git();
                view.reconcile_selection();
                view.main_section = MainSection::NewTask;
                view.schedule_ui_state_save(cx);
                match creation.failure {
                    Some((folder_name, error)) => view.show_warning(
                        format!(
                            "项目“{project_name}”已创建，已添加 {}/{} 个源文件夹；“{folder_name}”添加失败：{error}",
                            creation.created_sources, creation.source_count
                        ),
                        cx,
                    ),
                    None => view.show_success(
                        format!(
                            "项目“{project_name}”已创建 · {} 个源文件夹 · 权威修订 {revision}",
                            creation.created_sources
                        ),
                        cx,
                    ),
                }
            });
        }));
        cx.notify();
        true
    }

    fn create_project(&mut self, cx: &mut Context<Self>) {
        let name = Self::input_value(&self.project_name, cx);
        let root_path = Self::input_value(&self.project_root_path, cx);
        if name.is_empty() || root_path.is_empty() {
            self.show_validation_error("请输入项目名称和绝对根目录", cx);
            return;
        }
        self.run_mutation(
            "project.create",
            json!({ "name": name, "rootPath": root_path }),
            "项目已创建",
            cx,
        );
    }

    fn rename_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let name = Self::input_value(&self.project_new_name, cx);
        self.rename_project_to(project_id, &name, cx);
    }

    fn rename_project_to(&mut self, project_id: &str, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            self.show_validation_error("请输入项目的新名称", cx);
            return;
        }
        if self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .projects
                .iter()
                .any(|project| project.id == project_id && project.name == name)
        }) {
            self.show_info("项目名称未发生变化", cx);
            return;
        }
        self.run_mutation(
            "project.update",
            json!({ "projectId": project_id, "name": name }),
            "项目名称已更新",
            cx,
        );
    }

    fn create_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project_id.clone() else {
            self.show_validation_error("请先选择一个项目", cx);
            return;
        };
        let name = Self::input_value(&self.workspace_name, cx);
        let working_directory = Self::input_value(&self.workspace_directory, cx);
        if name.is_empty() || working_directory.is_empty() {
            self.show_validation_error("请输入工作区名称和绝对工作目录", cx);
            return;
        }
        self.run_mutation(
            "workspace.create",
            json!({
                "projectId": project_id,
                "name": name,
                "workingDirectory": working_directory,
            }),
            "工作区已创建",
            cx,
        );
    }

    fn rename_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let name = Self::input_value(&self.workspace_new_name, cx);
        if name.is_empty() {
            self.show_validation_error("请输入工作区的新名称", cx);
            return;
        }
        self.run_mutation(
            "workspace.update",
            json!({ "workspaceId": workspace_id, "name": name }),
            "工作区名称已更新",
            cx,
        );
    }

    fn set_workspace_archived(
        &mut self,
        workspace_id: &str,
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        let status = if archived { "archived" } else { "active" };
        let message = if archived {
            "工作区已归档"
        } else {
            "工作区已恢复"
        };
        self.run_mutation(
            "workspace.update",
            json!({ "workspaceId": workspace_id, "status": status }),
            message,
            cx,
        );
    }

    fn create_agent(&mut self, cx: &mut Context<Self>) {
        let Some(workspace_id) = self.selected_workspace_id.clone() else {
            self.show_validation_error("请先选择一个工作区", cx);
            return;
        };
        let provider = self.selected_provider.clone();
        let title = Self::input_value(&self.agent_title, cx);
        if title.is_empty() {
            self.show_validation_error("请输入 Agent 标题", cx);
            return;
        }
        if !self
            .provider_options()
            .iter()
            .any(|(candidate, _, _)| *candidate == provider)
        {
            self.show_validation_error("所选模型提供商当前不可用", cx);
            return;
        }
        self.run_mutation(
            "agent.create",
            json!({ "workspaceId": workspace_id, "provider": provider, "title": title }),
            "Agent 已创建",
            cx,
        );
    }

    fn rename_agent(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let title = Self::input_value(&self.agent_new_title, cx);
        self.rename_agent_to(agent_id, &title, cx);
    }

    fn rename_agent_to(&mut self, agent_id: &str, title: &str, cx: &mut Context<Self>) {
        let title = title.trim();
        if title.is_empty() {
            self.show_validation_error("请输入 Agent 的新标题", cx);
            return;
        }
        if self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .agents
                .iter()
                .any(|agent| agent.id == agent_id && agent.title == title)
        }) {
            self.show_info("任务名称未发生变化", cx);
            return;
        }
        self.run_mutation(
            "agent.update",
            json!({ "agentId": agent_id, "title": title }),
            "任务名称已更新",
            cx,
        );
    }

    pub(super) fn start_agent(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        self.run_mutation(
            "agent.start",
            json!({ "agentId": agent_id }),
            "Agent 已启动",
            cx,
        );
    }

    pub(super) fn stop_agent(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        self.run_mutation(
            "agent.stop",
            json!({ "agentId": agent_id }),
            "Agent 已停止",
            cx,
        );
    }

    pub(super) fn request_agent_delete(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let target = DeleteTarget::Agent(agent_id.to_owned());
        if self.delete_confirmation.as_ref() != Some(&target) {
            self.delete_confirmation = Some(target);
            self.show_warning("删除 Agent 不可撤销；请再次点击“确认删除 Agent”", cx);
            return;
        }
        self.run_mutation(
            "agent.delete",
            json!({ "agentId": agent_id }),
            "Agent 已删除",
            cx,
        );
    }

    fn request_project_delete(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let target = DeleteTarget::Project(project_id.to_owned());
        if self.delete_confirmation.as_ref() != Some(&target) {
            self.delete_confirmation = Some(target);
            self.show_warning("删除项目不可撤销；请再次点击“确认删除项目”", cx);
            return;
        }
        self.delete_project(project_id, cx);
    }

    fn delete_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.run_mutation(
            "project.delete",
            json!({ "projectId": project_id }),
            "项目已删除",
            cx,
        );
    }

    fn request_workspace_delete(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let target = DeleteTarget::Workspace(workspace_id.to_owned());
        if self.delete_confirmation.as_ref() != Some(&target) {
            self.delete_confirmation = Some(target);
            self.show_warning("删除工作区不可撤销；请再次点击“确认删除工作区”", cx);
            return;
        }
        self.run_mutation(
            "workspace.delete",
            json!({ "workspaceId": workspace_id }),
            "工作区已删除",
            cx,
        );
    }

    fn run_mutation(
        &mut self,
        method: &'static str,
        payload: Value,
        success_message: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.operation_in_flight {
            self.show_warning("另一个资源操作正在执行，请稍后再试", cx);
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
        if !matches!(self.state, corbit_client::ConnectionState::Online) || self.snapshot.is_none()
        {
            self.show_validation_error("请等待 Daemon 完成权威状态同步", cx);
            return;
        }
        let Value::Object(mut params) = payload else {
            self.show_validation_error("内部错误：资源操作参数无效", cx);
            return;
        };
        let signature = format!("{method}:{}", Value::Object(params.clone()));
        let client_mutation_id = self
            .retry_mutation
            .as_ref()
            .filter(|pending| pending.signature == signature)
            .map_or_else(
                || format!("mutation_{}", uuid::Uuid::new_v4()),
                |pending| pending.client_mutation_id.clone(),
            );
        self.retry_mutation = Some(RetryMutation {
            signature,
            client_mutation_id: client_mutation_id.clone(),
        });
        params.insert("clientMutationId".into(), Value::String(client_mutation_id));

        self.operation_in_flight = true;
        self.delete_confirmation = None;
        self.detail = format!("正在执行 {method}…");
        self.mutation_task = Some(cx.spawn(async move |view, cx| {
            let result = client
                .mutate_and_snapshot(method, Value::Object(params))
                .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.operation_in_flight = false;
                match result {
                    Ok((acknowledgement, snapshot)) => {
                        let previous_workspace_id = view.selected_workspace_id.clone();
                        view.retry_mutation = None;
                        view.snapshot = Some(snapshot);
                        match method {
                            "project.create" => {
                                view.selected_project_id =
                                    Some(acknowledgement.resource_id.clone());
                                view.selected_workspace_id = None;
                            }
                            "workspace.create" => {
                                view.selected_workspace_id =
                                    Some(acknowledgement.resource_id.clone());
                                view.selected_agent_id = None;
                            }
                            "agent.create" => {
                                view.selected_agent_id = Some(acknowledgement.resource_id.clone());
                            }
                            _ => {}
                        }
                        view.reconcile_selection();
                        if view.selected_workspace_id != previous_workspace_id {
                            view.clear_workspace_files();
                            view.clear_workspace_git();
                        }
                        view.show_success(
                            format!("{success_message} · 权威修订 {}", acknowledgement.revision),
                            cx,
                        );
                        view.schedule_ui_state_save(cx);
                    }
                    Err(error) => {
                        view.show_error(
                            format!("操作失败：{error}；再次提交相同操作将复用原 mutation ID"),
                            cx,
                        );
                    }
                }
            });
        }));
        cx.notify();
    }

    fn render_project_editor(
        &self,
        project: &corbit_client::ProjectResource,
        can_mutate: bool,
        workspace_count: usize,
        delete_armed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let rename_id = project.id.clone();
        let delete_id = project.id.clone();
        div()
            .v_flex()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(format!("已选择：{}", project.name)),
            )
            .child(
                Input::new(&self.project_new_name)
                    .small()
                    .disabled(!can_mutate),
            )
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("rename-project")
                            .outline()
                            .small()
                            .label("重命名")
                            .disabled(!can_mutate)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.rename_project(&rename_id, cx);
                            })),
                    )
                    .child(
                        Button::new("delete-project")
                            .danger()
                            .small()
                            .label(if delete_armed {
                                "确认删除项目"
                            } else {
                                "删除项目"
                            })
                            .disabled(!can_mutate || workspace_count > 0)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.request_project_delete(&delete_id, cx);
                            })),
                    ),
            )
            .when(workspace_count > 0, |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_WARNING))
                        .child(format!("需先删除该项目下的 {workspace_count} 个工作区")),
                )
            })
    }

    fn render_workspace_editor(
        &self,
        workspace: &corbit_client::WorkspaceResource,
        can_mutate: bool,
        agent_count: usize,
        delete_armed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let rename_id = workspace.id.clone();
        let archive_id = workspace.id.clone();
        let delete_id = workspace.id.clone();
        let is_archived = workspace.status == corbit_client::WorkspaceStatus::Archived;
        div()
            .v_flex()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(format!("目录：{}", workspace.working_directory)),
            )
            .child(
                Input::new(&self.workspace_new_name)
                    .small()
                    .disabled(!can_mutate),
            )
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("rename-workspace")
                            .outline()
                            .small()
                            .label("重命名")
                            .disabled(!can_mutate)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.rename_workspace(&rename_id, cx);
                            })),
                    )
                    .child(
                        Button::new("archive-workspace")
                            .outline()
                            .small()
                            .label(if is_archived { "恢复" } else { "归档" })
                            .disabled(!can_mutate)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.set_workspace_archived(&archive_id, !is_archived, cx);
                            })),
                    )
                    .child(
                        Button::new("delete-workspace")
                            .danger()
                            .small()
                            .label(if delete_armed {
                                "确认删除工作区"
                            } else {
                                "删除工作区"
                            })
                            .disabled(!can_mutate || agent_count > 0)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.request_workspace_delete(&delete_id, cx);
                            })),
                    ),
            )
            .when(agent_count > 0, |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_WARNING))
                        .child(format!("需先停止并移除关联的 {agent_count} 个 Agent")),
                )
            })
    }

    fn render_agent_editor(
        &self,
        agent: &corbit_client::AgentResource,
        can_mutate: bool,
        delete_armed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let rename_id = agent.id.clone();
        let start_id = agent.id.clone();
        let stop_id = agent.id.clone();
        let delete_id = agent.id.clone();
        let is_stopped = agent.status == corbit_client::AgentStatus::Stopped;
        let can_start = matches!(
            agent.status,
            corbit_client::AgentStatus::Idle | corbit_client::AgentStatus::Error
        );
        let can_stop = !is_stopped;
        div()
            .v_flex()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(format!("模型提供商：{}", agent.provider)),
            )
            .child(
                Input::new(&self.agent_new_title)
                    .small()
                    .disabled(!can_mutate),
            )
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("rename-agent")
                            .outline()
                            .small()
                            .label("重命名")
                            .disabled(!can_mutate)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.rename_agent(&rename_id, cx);
                            })),
                    )
                    .child(
                        Button::new("start-agent")
                            .primary()
                            .small()
                            .label("启动")
                            .disabled(!can_mutate || !can_start)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.start_agent(&start_id, cx);
                            })),
                    )
                    .child(
                        Button::new("stop-agent")
                            .outline()
                            .small()
                            .label(if is_stopped { "已停止" } else { "停止" })
                            .disabled(!can_mutate || !can_stop)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.stop_agent(&stop_id, cx);
                            })),
                    )
                    .child(
                        Button::new("delete-agent")
                            .danger()
                            .small()
                            .label(if delete_armed {
                                "确认删除 Agent"
                            } else {
                                "删除 Agent"
                            })
                            .disabled(!can_mutate || !is_stopped)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.request_agent_delete(&delete_id, cx);
                            })),
                    ),
            )
            .when(!is_stopped, |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_WARNING))
                        .child("需先停止 Agent，随后才能删除。"),
                )
            })
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_resource_panel(
        &mut self,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot =
            self.snapshot
                .clone()
                .unwrap_or_else(|| corbit_client::AuthoritativeSnapshot {
                    schema_version: 0,
                    generated_at: String::new(),
                    revision: 0,
                    projects: Vec::new(),
                    workspaces: Vec::new(),
                    agents: Vec::new(),
                    extensions: std::collections::BTreeMap::default(),
                });

        let can_mutate = is_online && !self.operation_in_flight;
        let project_rows = snapshot
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| {
                let project_id = project.id.clone();
                let selected = self.selected_project_id.as_ref() == Some(&project.id);
                Button::new(("project-row", index))
                    .ghost()
                    .small()
                    .h(navigation_row_height())
                    .w_full()
                    .justify_start()
                    .selected(selected)
                    .label(format!("{} · {}", project.name, project.root_path))
                    .disabled(self.operation_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.select_project_in_settings(&project_id, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let selected_project = self.selected_project_id.as_ref().and_then(|id| {
            snapshot
                .projects
                .iter()
                .find(|project| &project.id == id)
                .cloned()
        });
        let project_workspace_count = selected_project.as_ref().map_or(0, |project| {
            snapshot
                .workspaces
                .iter()
                .filter(|workspace| workspace.project_id == project.id)
                .count()
        });
        let project_delete_armed = selected_project.as_ref().is_some_and(|project| {
            self.delete_confirmation == Some(DeleteTarget::Project(project.id.clone()))
        });

        let selected_project_id = self.selected_project_id.clone();
        let visible_workspaces = snapshot
            .workspaces
            .iter()
            .filter(|workspace| Some(&workspace.project_id) == selected_project_id.as_ref())
            .cloned()
            .collect::<Vec<_>>();
        let workspace_rows = visible_workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                let workspace_id = workspace.id.clone();
                let selected = self.selected_workspace_id.as_ref() == Some(&workspace.id);
                let status = match workspace.status {
                    corbit_client::WorkspaceStatus::Active => "活跃",
                    corbit_client::WorkspaceStatus::Archived => "已归档",
                };
                Button::new(("workspace-row", index))
                    .ghost()
                    .small()
                    .h(navigation_row_height())
                    .w_full()
                    .justify_start()
                    .selected(selected)
                    .label(format!("{} · {status}", workspace.name))
                    .disabled(self.operation_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.select_workspace_in_settings(&workspace_id, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let selected_workspace = self.selected_workspace_id.as_ref().and_then(|id| {
            visible_workspaces
                .iter()
                .find(|workspace| &workspace.id == id)
                .cloned()
        });
        let workspace_agent_count = selected_workspace.as_ref().map_or(0, |workspace| {
            snapshot
                .agents
                .iter()
                .filter(|agent| agent.workspace_id == workspace.id)
                .count()
        });
        let workspace_delete_armed = selected_workspace.as_ref().is_some_and(|workspace| {
            self.delete_confirmation == Some(DeleteTarget::Workspace(workspace.id.clone()))
        });
        let selected_workspace_id = self.selected_workspace_id.clone();
        let visible_agents = snapshot
            .agents
            .iter()
            .filter(|agent| Some(&agent.workspace_id) == selected_workspace_id.as_ref())
            .cloned()
            .collect::<Vec<_>>();
        let agent_rows = visible_agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let agent_id = agent.id.clone();
                let selected = self.selected_agent_id.as_ref() == Some(&agent.id);
                let status = match agent.status {
                    corbit_client::AgentStatus::Initializing => "初始化中",
                    corbit_client::AgentStatus::Idle => "空闲",
                    corbit_client::AgentStatus::Running => "运行中",
                    corbit_client::AgentStatus::Error => "错误",
                    corbit_client::AgentStatus::Stopped => "已停止",
                };
                Button::new(("agent-row", index))
                    .ghost()
                    .small()
                    .h(navigation_row_height())
                    .w_full()
                    .justify_start()
                    .selected(selected)
                    .label(format!("{} · {} · {status}", agent.title, agent.provider))
                    .disabled(self.operation_in_flight)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.select_agent_in_settings(&agent_id, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let selected_agent = self
            .selected_agent_id
            .as_ref()
            .and_then(|id| visible_agents.iter().find(|agent| &agent.id == id).cloned());
        let agent_delete_armed = selected_agent.as_ref().is_some_and(|agent| {
            self.delete_confirmation == Some(DeleteTarget::Agent(agent.id.clone()))
        });

        let project_editor = selected_project.map(|project| {
            self.render_project_editor(
                &project,
                can_mutate,
                project_workspace_count,
                project_delete_armed,
                cx,
            )
        });

        let workspace_editor = selected_workspace.as_ref().map(|workspace| {
            self.render_workspace_editor(
                workspace,
                can_mutate,
                workspace_agent_count,
                workspace_delete_armed,
                cx,
            )
        });
        let agent_editor = selected_agent
            .map(|agent| self.render_agent_editor(&agent, can_mutate, agent_delete_armed, cx));
        let selected_workspace_is_active = selected_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.status == corbit_client::WorkspaceStatus::Active);

        let project_panel = div()
            .v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_HEADING))
                            .font_semibold()
                            .child("项目"),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("项目定义代码根目录，可包含多个工作区。"),
                    ),
            )
            .when(snapshot.projects.is_empty(), |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("尚无项目，请先创建一个项目。"),
                )
            })
            .children(project_rows)
            .child(Input::new(&self.project_name).small().disabled(!can_mutate))
            .child(
                Input::new(&self.project_root_path)
                    .small()
                    .disabled(!can_mutate),
            )
            .child(
                Button::new("create-project")
                    .primary()
                    .small()
                    .label("创建项目")
                    .loading(self.operation_in_flight)
                    .disabled(!can_mutate)
                    .on_click(cx.listener(|view, _, _, cx| view.create_project(cx))),
            )
            .children(project_editor);

        let workspace_panel = div()
            .v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_HEADING))
                            .font_semibold()
                            .child("工作区"),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("为任务选择实际工作目录，并管理归档状态。"),
                    ),
            )
            .when(selected_project_id.is_none(), |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("选择项目后可创建工作区。"),
                )
            })
            .when(
                selected_project_id.is_some() && visible_workspaces.is_empty(),
                |panel| {
                    panel.child(
                        div()
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("当前项目尚无工作区。"),
                    )
                },
            )
            .children(workspace_rows)
            .child(
                Input::new(&self.workspace_name)
                    .small()
                    .disabled(!can_mutate || selected_project_id.is_none()),
            )
            .child(
                Input::new(&self.workspace_directory)
                    .small()
                    .disabled(!can_mutate || selected_project_id.is_none()),
            )
            .child(
                Button::new("create-workspace")
                    .primary()
                    .small()
                    .label("创建工作区")
                    .loading(self.operation_in_flight)
                    .disabled(!can_mutate || selected_project_id.is_none())
                    .on_click(cx.listener(|view, _, _, cx| view.create_workspace(cx))),
            )
            .children(workspace_editor);

        let agent_panel = div()
            .v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_HEADING))
                            .font_semibold()
                            .child("Agent"),
                    )
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_SM))
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("根据 Daemon 能力创建 Codex、Claude 或 ACP Agent。"),
                    ),
            )
            .when(selected_workspace_id.is_none(), |panel| {
                panel.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("选择工作区后可创建 Agent。"),
                )
            })
            .when(
                selected_workspace_id.is_some() && visible_agents.is_empty(),
                |panel| {
                    panel.child(
                        div()
                            .text_color(rgb(COLOR_TEXT_SECONDARY))
                            .child("当前工作区尚无 Agent。"),
                    )
                },
            )
            .children(agent_rows)
            .child(div().h_flex().flex_wrap().gap_2().children(
                self.provider_options().into_iter().enumerate().map(
                    |(index, (provider, label, _))| {
                        Button::new(("agent-provider", index))
                            .outline()
                            .small()
                            .selected(self.selected_provider == provider)
                            .label(label)
                            .disabled(!can_mutate || !selected_workspace_is_active)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.choose_default_provider(provider, cx);
                            }))
                    },
                ),
            ))
            .child(
                Input::new(&self.agent_title)
                    .small()
                    .disabled(!can_mutate || !selected_workspace_is_active),
            )
            .child(
                Button::new("create-agent")
                    .primary()
                    .small()
                    .label("创建 Agent")
                    .loading(self.operation_in_flight)
                    .disabled(!can_mutate || !selected_workspace_is_active)
                    .on_click(cx.listener(|view, _, _, cx| view.create_agent(cx))),
            )
            .when(
                selected_workspace_id.is_some() && !selected_workspace_is_active,
                |panel| {
                    panel.child(
                        div()
                            .text_color(rgb(COLOR_WARNING))
                            .child("归档工作区不能创建 Agent。"),
                    )
                },
            )
            .children(agent_editor);

        let selected_panel = match self.resource_section {
            ResourceSection::General => self.render_general_settings(is_online, cx),
            ResourceSection::Appearance => self.render_appearance_settings(cx),
            ResourceSection::Providers => self.render_provider_settings(cx),
            ResourceSection::Plugins => self.render_plugin_settings(is_online, cx),
            ResourceSection::Shortcuts => Self::render_shortcut_settings(),
            ResourceSection::Projects => project_panel,
            ResourceSection::Workspaces => workspace_panel,
            ResourceSection::Agents => agent_panel,
            ResourceSection::Devices => self.render_device_settings(is_online, cx),
            ResourceSection::About => Self::render_about_settings(),
        };

        div()
            .v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(shell_background())
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .v_flex()
                            .w(px(224.))
                            .h_full()
                            .flex_none()
                            .border_r_1()
                            .border_color(rgb(COLOR_BORDER_LIGHT))
                            .bg(sidebar_rgb())
                            .gap_4()
                            .px_2()
                            .pt(px(TOOLBAR_HEIGHT))
                            .pb_4()
                            .child(
                                Button::new("settings-back")
                                    .ghost()
                                    .small()
                                    .h(navigation_row_height())
                                    .w_full()
                                    .justify_start()
                                    .icon(Icon::new(AppIcon::Back))
                                    .label("返回应用")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.close_settings(cx);
                                    })),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("偏好设置"),
                            )
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("settings-general")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::General,
                                            )
                                            .icon(Icon::new(AppIcon::Settings))
                                            .label("常规")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::General,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-appearance")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section
                                                    == ResourceSection::Appearance,
                                            )
                                            .icon(Icon::new(AppIcon::Appearance))
                                            .label("外观")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Appearance,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-providers")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Providers,
                                            )
                                            .icon(Icon::new(AppIcon::Provider))
                                            .label("模型提供商")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Providers,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-plugins")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Plugins,
                                            )
                                            .icon(Icon::new(AppIcon::Tool))
                                            .label("插件")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Plugins,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-shortcuts")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Shortcuts,
                                            )
                                            .icon(Icon::new(AppIcon::Shortcuts))
                                            .label("快捷键")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Shortcuts,
                                                    cx,
                                                );
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("工作区"),
                            )
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("settings-projects")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Projects,
                                            )
                                            .icon(Icon::new(AppIcon::Project))
                                            .label("项目")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Projects,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-workspaces")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section
                                                    == ResourceSection::Workspaces,
                                            )
                                            .icon(Icon::new(AppIcon::Workspace))
                                            .label("工作区")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Workspaces,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-agents")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Agents,
                                            )
                                            .icon(Icon::new(AppIcon::Agent))
                                            .label("Agent")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Agents,
                                                    cx,
                                                );
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .text_size(font_px(FONT_SIZE_SM))
                                    .font_medium()
                                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                                    .child("连接"),
                            )
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("settings-devices")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::Devices,
                                            )
                                            .icon(Icon::new(AppIcon::Device))
                                            .label("设备")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::Devices,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-about")
                                            .ghost()
                                            .small()
                                            .h(navigation_row_height())
                                            .w_full()
                                            .justify_start()
                                            .selected(
                                                self.resource_section == ResourceSection::About,
                                            )
                                            .icon(Icon::new(AppIcon::Info))
                                            .label("关于 Corbit")
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.set_resource_section(
                                                    ResourceSection::About,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(0.))
                            .bg(rgb(COLOR_SURFACE))
                            .overflow_y_scrollbar()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(792.))
                                    .mx_auto()
                                    .px(px(36.))
                                    .pt(px(72.))
                                    .pb_8()
                                    .child(selected_panel),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_request_uses_first_folder_as_root_and_removes_duplicates() {
        let first = std::env::temp_dir().join("corbit-project-one");
        let second = std::env::temp_dir().join("corbit-project-two");

        let request =
            project_create_request("Corbit", &[first.clone(), first.clone(), second.clone()])
                .expect("absolute project paths should be accepted");

        assert_eq!(request.name, "Corbit");
        assert_eq!(request.root_path, first.to_string_lossy());
        assert_eq!(request.source_folders.len(), 2);
        assert_eq!(request.source_folders[0], first.to_string_lossy());
        assert_eq!(request.source_folders[1], second.to_string_lossy());
    }

    #[test]
    fn project_source_folders_reject_relative_paths() {
        let error = validated_project_source_folders([PathBuf::from("relative-project")])
            .expect_err("relative project paths must be rejected");

        assert_eq!(error, "请选择绝对路径下的项目文件夹");
    }

    #[test]
    fn project_request_requires_name_and_source_folder() {
        let folder = std::env::temp_dir().join("corbit-project");

        assert_eq!(
            project_create_request("  ", std::slice::from_ref(&folder))
                .expect_err("blank names must be rejected"),
            "项目名称不能为空"
        );
        assert_eq!(
            project_create_request("Corbit", &[]).expect_err("a source folder must be required"),
            "请至少添加一个源文件夹"
        );
    }

    #[test]
    fn project_name_autofill_uses_first_folder_only_when_name_is_empty() {
        let first = PathBuf::from("/work/corbit");
        let second = PathBuf::from("/work/other");

        assert_eq!(
            project_name_autofill("  ", &[first.clone(), second]),
            Some("corbit".into())
        );
        assert_eq!(project_name_autofill("My Project", &[first]), None);
        assert_eq!(project_name_autofill("", &[]), None);
    }

    #[test]
    fn project_sidebar_state_toggles_between_expanded_and_collapsed() {
        let mut collapsed_projects = BTreeSet::new();

        toggle_sidebar_project_state(&mut collapsed_projects, "project-1");
        assert!(collapsed_projects.contains("project-1"));

        toggle_sidebar_project_state(&mut collapsed_projects, "project-1");
        assert!(!collapsed_projects.contains("project-1"));
    }
}
