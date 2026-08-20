use super::*;
use gpui_component::{
    PixelsExt,
    resizable::{h_resizable, resizable_panel},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileOperationState {
    Idle,
    Loading,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FileOperationTarget {
    Directory(String),
    File(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitOperationState {
    Idle,
    LoadingStatus,
    LoadingDiff,
}

#[derive(Debug, Default)]
pub(super) struct WorkspaceRefreshQueue {
    directory: Option<String>,
    file: Option<String>,
    git: bool,
}

impl WorkspaceRefreshQueue {
    fn clear_files(&mut self) {
        self.directory = None;
        self.file = None;
    }

    fn clear_git(&mut self) {
        self.git = false;
    }
}

fn workspace_paths_affect_file(paths: &[String], file: &str) -> bool {
    paths.is_empty() || paths.iter().any(|path| path == file)
}

fn workspace_paths_affect_directory(paths: &[String], directory: &str) -> bool {
    paths.is_empty()
        || paths.iter().any(|path| {
            path == directory || path.rsplit_once('/').map_or("", |(parent, _)| parent) == directory
        })
}

impl ConnectionView {
    pub(super) fn clear_workspace_files(&mut self) {
        self.workspace_listing = None;
        self.workspace_file = None;
        self.workspace_refresh_queue.clear_files();
        self.file_operation_state = FileOperationState::Idle;
        self.file_operation_target = None;
        self.file_task = None;
    }

    pub(super) fn clear_workspace_git(&mut self) {
        self.workspace_git_status = None;
        self.workspace_git_diff = None;
        self.workspace_refresh_queue.clear_git();
        self.git_operation_state = GitOperationState::Idle;
        self.git_task = None;
    }

    fn workspace_client(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(String, corbit_client::DaemonRuntimeClient)> {
        let Some(workspace_id) = self.selected_workspace_id.clone() else {
            self.show_validation_error("请先选择一个工作区", cx);
            return None;
        };
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            self.show_validation_error("Daemon 尚未连接", cx);
            return None;
        };
        if !matches!(self.state, corbit_client::ConnectionState::Online) {
            self.show_validation_error("请等待 Daemon 连接完成", cx);
            return None;
        }
        Some((workspace_id, client))
    }

    pub(super) fn apply_workspace_changed(
        &mut self,
        change: &corbit_client::WorkspaceChanged,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.state, corbit_client::ConnectionState::Online)
            || self.selected_workspace_id.as_deref() != Some(change.workspace_id.as_str())
        {
            return;
        }

        let active_directory = match self.file_operation_target.as_ref() {
            Some(FileOperationTarget::Directory(path)) => Some(path.clone()),
            _ => None,
        };
        let directory = active_directory.or_else(|| {
            self.workspace_listing
                .as_ref()
                .map(|listing| listing.path.clone())
        });
        let active_file = match self.file_operation_target.as_ref() {
            Some(FileOperationTarget::File(path)) => Some(path.clone()),
            _ => None,
        };
        let file =
            active_file.or_else(|| self.workspace_file.as_ref().map(|file| file.path.clone()));
        let directory =
            directory.filter(|path| workspace_paths_affect_directory(&change.paths, path.as_str()));
        let file = file.filter(|path| workspace_paths_affect_file(&change.paths, path.as_str()));

        if directory.is_some() || file.is_some() {
            self.queue_workspace_file_refresh(directory, file, cx);
        }
        if self.workspace_git_status.is_some()
            || self.workspace_git_diff.is_some()
            || self.git_operation_state != GitOperationState::Idle
        {
            self.queue_workspace_git_refresh(cx);
        }
    }

    fn queue_workspace_file_refresh(
        &mut self,
        directory: Option<String>,
        file: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(directory) = directory {
            self.workspace_refresh_queue.directory = Some(directory);
        }
        if let Some(file) = file {
            self.workspace_refresh_queue.file = Some(file);
        }
        self.run_pending_workspace_file_refresh(cx);
    }

    fn run_pending_workspace_file_refresh(&mut self, cx: &mut Context<Self>) {
        if self.file_operation_state == FileOperationState::Loading {
            return;
        }
        if let Some(path) = self.workspace_refresh_queue.directory.take() {
            self.load_workspace_directory(path, cx);
        } else if let Some(path) = self.workspace_refresh_queue.file.take() {
            self.load_workspace_file(path, cx);
        }
    }

    fn queue_workspace_git_refresh(&mut self, cx: &mut Context<Self>) {
        self.workspace_refresh_queue.git = true;
        self.run_pending_workspace_git_refresh(cx);
    }

    fn run_pending_workspace_git_refresh(&mut self, cx: &mut Context<Self>) {
        if self.git_operation_state != GitOperationState::Idle || !self.workspace_refresh_queue.git
        {
            return;
        }
        self.workspace_refresh_queue.git = false;
        self.load_workspace_git_status(cx);
    }

    pub(super) fn load_workspace_directory(&mut self, path: String, cx: &mut Context<Self>) {
        if self.file_operation_state == FileOperationState::Loading {
            return;
        }
        let Some((workspace_id, client)) = self.workspace_client(cx) else {
            return;
        };

        self.file_operation_state = FileOperationState::Loading;
        self.file_operation_target = Some(FileOperationTarget::Directory(path.clone()));
        self.workspace_file = None;
        self.detail = if path.is_empty() {
            "正在读取工作区根目录…".into()
        } else {
            format!("正在读取目录 {path}…")
        };
        let requested_workspace_id = workspace_id.clone();
        let requested_path = path.clone();
        self.file_task = Some(cx.spawn(async move |view, cx| {
            let result = client.list_workspace_files(workspace_id, path).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.selected_workspace_id.as_deref() != Some(requested_workspace_id.as_str()) {
                    return;
                }
                view.file_operation_state = FileOperationState::Idle;
                view.file_operation_target = None;
                view.file_task = None;
                match result {
                    Ok(listing)
                        if listing.workspace_id == requested_workspace_id
                            && listing.path == requested_path =>
                    {
                        let entry_count = listing.entries.len();
                        view.workspace_listing = Some(listing);
                        view.workspace_file = None;
                        view.detail = format!("目录已加载 · {entry_count} 项");
                    }
                    Ok(_) => {
                        view.show_warning("文件目录响应与当前请求不匹配，已忽略", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("目录读取失败：{error}"), cx);
                    }
                }
                view.run_pending_workspace_file_refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn load_workspace_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.file_operation_state == FileOperationState::Loading {
            return;
        }
        let Some((workspace_id, client)) = self.workspace_client(cx) else {
            return;
        };

        self.file_operation_state = FileOperationState::Loading;
        self.file_operation_target = Some(FileOperationTarget::File(path.clone()));
        self.workspace_file = None;
        self.detail = format!("正在读取文件 {path}…");
        let requested_workspace_id = workspace_id.clone();
        let requested_path = path.clone();
        self.file_task = Some(cx.spawn(async move |view, cx| {
            let result = client.read_workspace_file(workspace_id, path).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.selected_workspace_id.as_deref() != Some(requested_workspace_id.as_str()) {
                    return;
                }
                view.file_operation_state = FileOperationState::Idle;
                view.file_operation_target = None;
                view.file_task = None;
                match result {
                    Ok(file)
                        if file.workspace_id == requested_workspace_id
                            && file.path == requested_path =>
                    {
                        view.detail = format!("文本文件已加载 · {} 字节", file.byte_length);
                        view.workspace_file = Some(file);
                    }
                    Ok(_) => {
                        view.show_warning("文件内容响应与当前请求不匹配，已忽略", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("文件读取失败：{error}"), cx);
                    }
                }
                view.run_pending_workspace_file_refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn load_workspace_git_status(&mut self, cx: &mut Context<Self>) {
        if self.git_operation_state != GitOperationState::Idle {
            return;
        }
        let Some((workspace_id, client)) = self.workspace_client(cx) else {
            return;
        };

        self.git_operation_state = GitOperationState::LoadingStatus;
        self.workspace_git_diff = None;
        self.detail = "正在读取工作区 Git 状态…".into();
        let requested_workspace_id = workspace_id.clone();
        self.git_task = Some(cx.spawn(async move |view, cx| {
            let result = client.workspace_git_status(workspace_id).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.selected_workspace_id.as_deref() != Some(requested_workspace_id.as_str()) {
                    return;
                }
                view.git_operation_state = GitOperationState::Idle;
                view.git_task = None;
                match result {
                    Ok(status) if status.workspace_id == requested_workspace_id => {
                        if status.is_repository {
                            view.detail =
                                format!("Git 状态已加载 · {} 项变更", status.changes.len());
                        } else {
                            view.show_info("当前工作区不在 Git 仓库中", cx);
                        }
                        view.workspace_git_status = Some(status);
                        view.workspace_git_diff = None;
                    }
                    Ok(_) => {
                        view.show_warning("Git 状态响应与当前工作区不匹配，已忽略", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("Git 状态读取失败：{error}"), cx);
                    }
                }
                view.run_pending_workspace_git_refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn load_workspace_git_diff(&mut self, path: String, cx: &mut Context<Self>) {
        if self.git_operation_state != GitOperationState::Idle {
            return;
        }
        let Some((workspace_id, client)) = self.workspace_client(cx) else {
            return;
        };

        self.git_operation_state = GitOperationState::LoadingDiff;
        self.workspace_git_diff = None;
        self.detail = format!("正在读取 Git 差异 {path}…");
        let requested_workspace_id = workspace_id.clone();
        let requested_path = path.clone();
        self.git_task = Some(cx.spawn(async move |view, cx| {
            let result = client.workspace_git_diff(workspace_id, path).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.selected_workspace_id.as_deref() != Some(requested_workspace_id.as_str()) {
                    return;
                }
                view.git_operation_state = GitOperationState::Idle;
                view.git_task = None;
                match result {
                    Ok(diff)
                        if diff.workspace_id == requested_workspace_id
                            && diff.path == requested_path =>
                    {
                        view.detail = if diff.is_binary {
                            format!("二进制 Git 差异已确认 · {} 字节", diff.byte_length)
                        } else {
                            format!("Git 差异已加载 · {} 字节", diff.byte_length)
                        };
                        view.workspace_git_diff = Some(diff);
                    }
                    Ok(_) => {
                        view.show_warning("Git 差异响应与当前请求不匹配，已忽略", cx);
                    }
                    Err(error) => {
                        view.show_error(format!("Git 差异读取失败：{error}"), cx);
                    }
                }
                view.run_pending_workspace_git_refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_workspace_files_panel(
        &self,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected_workspace = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_workspace_id.as_ref()?;
            snapshot
                .workspaces
                .iter()
                .find(|workspace| &workspace.id == selected)
                .cloned()
        });
        let panel = div()
            .v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(rgb(COLOR_SURFACE))
            .child(
                div()
                    .h_flex()
                    .h(px(PANE_TOOLBAR_HEIGHT))
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .child(div().font_semibold().child("工作区文件"))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child("只读 · 由 Daemon 安全访问"),
                    ),
            );

        let Some(workspace) = selected_workspace else {
            return panel.child(
                div()
                    .v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child("选择工作区后可浏览目录和预览 UTF-8 文本文件。"),
            );
        };
        let is_loading = self.file_operation_state == FileOperationState::Loading;
        let can_browse = is_online && !is_loading;
        let panel = panel.child(
            div()
                .h_flex()
                .h(px(PANE_TOOLBAR_HEIGHT))
                .flex_none()
                .items_center()
                .gap_2()
                .px_5()
                .border_b_1()
                .border_color(rgb(COLOR_BORDER_LIGHT))
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(Icon::new(AppIcon::FolderOpen))
                .child(div().font_medium().child(workspace.name))
                .child("·")
                .child(div().truncate().child(workspace.working_directory)),
        );

        let Some(listing) = self.workspace_listing.clone() else {
            return panel.child(
                div()
                    .v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(
                        Icon::new(AppIcon::FolderOpen)
                            .with_size(gpui_component::Size::Large)
                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                    )
                    .child("目录尚未加载。文件内容不会由桌面端直接读取。")
                    .child(
                        Button::new("workspace-files-load-root")
                            .primary()
                            .small()
                            .label("加载根目录")
                            .loading(is_loading)
                            .disabled(!can_browse)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.load_workspace_directory(String::new(), cx);
                            })),
                    ),
            );
        };

        let current_path_label = if listing.path.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", listing.path)
        };
        let refresh_path = listing.path.clone();
        let parent_path = listing
            .path
            .rsplit_once('/')
            .map_or_else(String::new, |(parent, _)| parent.to_owned());
        let can_go_up = !listing.path.is_empty();
        let parent_path_for_click = parent_path.clone();
        let selected_file_path = self.workspace_file.as_ref().map(|file| file.path.as_str());
        let entry_rows = listing
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let entry_path = entry.path.clone();
                let entry_kind = entry.kind.clone();
                let actionable = matches!(
                    &entry.kind,
                    corbit_client::WorkspaceFileEntryKind::Directory
                        | corbit_client::WorkspaceFileEntryKind::File
                );
                let selected = selected_file_path == Some(entry.path.as_str());
                let (icon, label) = match entry.kind {
                    corbit_client::WorkspaceFileEntryKind::Directory => {
                        (AppIcon::Folder, entry.name.clone())
                    }
                    corbit_client::WorkspaceFileEntryKind::File => {
                        (AppIcon::File, entry.name.clone())
                    }
                    corbit_client::WorkspaceFileEntryKind::Symlink => {
                        (AppIcon::ExternalLink, format!("{}（符号链接）", entry.name))
                    }
                    corbit_client::WorkspaceFileEntryKind::Other => {
                        (AppIcon::File, format!("{}（不可打开）", entry.name))
                    }
                };
                Button::new(("workspace-file-entry", index))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(selected)
                    .icon(Icon::new(icon))
                    .label(label)
                    .disabled(!can_browse || !actionable)
                    .on_click(cx.listener(move |view, _, _, cx| match &entry_kind {
                        corbit_client::WorkspaceFileEntryKind::Directory => {
                            view.load_workspace_directory(entry_path.clone(), cx);
                        }
                        corbit_client::WorkspaceFileEntryKind::File => {
                            view.load_workspace_file(entry_path.clone(), cx);
                        }
                        corbit_client::WorkspaceFileEntryKind::Symlink
                        | corbit_client::WorkspaceFileEntryKind::Other => {}
                    }))
            })
            .collect::<Vec<_>>();
        let has_entries = !entry_rows.is_empty();
        let entries_panel = div()
            .v_flex()
            .size_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(rgb(COLOR_BORDER_LIGHT))
            .bg(rgb(COLOR_SURFACE_UNDER))
            .when(!has_entries, |list| {
                list.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("当前目录为空。"),
                )
            })
            .children(entry_rows)
            .overflow_y_scrollbar();

        let preview_panel = if let Some(file) = self.workspace_file.clone() {
            div()
                .v_flex()
                .size_full()
                .min_w(px(0.))
                .min_h(px(0.))
                .gap_3()
                .p_4()
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .child(div().font_semibold().child(file.path))
                        .child(format!("{} 字节", file.byte_length)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .bg(rgb(COLOR_EDITOR))
                        .font_family(mono_font_family())
                        .text_size(font_px(FONT_SIZE_MONO))
                        .line_height(px(20.))
                        .whitespace_nowrap()
                        .child(file.content)
                        .overflow_scrollbar(),
                )
        } else {
            div()
                .v_flex()
                .size_full()
                .min_w(px(0.))
                .min_h(px(0.))
                .items_center()
                .justify_center()
                .bg(rgb(COLOR_SURFACE))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(if is_loading {
                    "正在读取…"
                } else {
                    "选择普通文本文件以预览内容"
                })
        };
        let view = cx.weak_entity();
        let split_panel = h_resizable("workspace-files-split")
            .child(
                resizable_panel()
                    .size(px(self.panel_widths.workspace_files_list()))
                    .size_range(
                        px(PanelWidths::MIN_WORKSPACE_LIST)..px(PanelWidths::MAX_WORKSPACE_LIST),
                    )
                    .child(entries_panel),
            )
            .child(resizable_panel().child(preview_panel))
            .on_resize(move |state, _, cx| {
                let Some(width) = state.read(cx).sizes().first().copied() else {
                    return;
                };
                let _ = view.update(cx, |view, cx| {
                    view.panel_widths.set_workspace_files_list(width.as_f32());
                    view.schedule_ui_state_save(cx);
                });
            });

        panel
            .child(
                div()
                    .h_flex()
                    .h(px(PANE_TOOLBAR_HEIGHT))
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .text_size(font_px(FONT_SIZE_SM))
                    .child(format!("当前目录：{current_path_label}"))
                    .child(
                        Button::new("workspace-files-up")
                            .outline()
                            .small()
                            .label("返回上级")
                            .disabled(!can_browse || !can_go_up)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.load_workspace_directory(parent_path_for_click.clone(), cx);
                            })),
                    )
                    .child(
                        Button::new("workspace-files-refresh")
                            .outline()
                            .small()
                            .label("刷新")
                            .loading(is_loading)
                            .disabled(!can_browse)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.load_workspace_directory(refresh_path.clone(), cx);
                            })),
                    ),
            )
            .child(div().h_flex().flex_1().min_h(px(0.)).child(split_panel))
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .px_4()
                    .py_2()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                    .child("符号链接和特殊文件仅展示，不允许从界面打开。预览上限为 1 MiB。"),
            )
    }

    fn git_change_kind_label(kind: corbit_client::WorkspaceGitChangeKind) -> &'static str {
        match kind {
            corbit_client::WorkspaceGitChangeKind::Added => "新增",
            corbit_client::WorkspaceGitChangeKind::Modified => "修改",
            corbit_client::WorkspaceGitChangeKind::Deleted => "删除",
            corbit_client::WorkspaceGitChangeKind::Renamed => "重命名",
            corbit_client::WorkspaceGitChangeKind::Copied => "复制",
            corbit_client::WorkspaceGitChangeKind::TypeChanged => "类型变化",
            corbit_client::WorkspaceGitChangeKind::Conflicted => "冲突",
            corbit_client::WorkspaceGitChangeKind::Untracked => "未跟踪",
        }
    }

    fn git_change_label(change: &corbit_client::WorkspaceGitChange) -> String {
        let index = change.index_status.map(Self::git_change_kind_label);
        let worktree = change.worktree_status.map(Self::git_change_kind_label);
        match (index, worktree) {
            (Some(index), Some(worktree)) => {
                format!("{} · 暂存：{index} · 工作区：{worktree}", change.path)
            }
            (Some(index), None) => format!("{} · 暂存：{index}", change.path),
            (None, Some(worktree)) => format!("{} · 工作区：{worktree}", change.path),
            (None, None) => change.path.clone(),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_workspace_git_panel(
        &self,
        is_online: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected_workspace = self.snapshot.as_ref().and_then(|snapshot| {
            let selected = self.selected_workspace_id.as_ref()?;
            snapshot
                .workspaces
                .iter()
                .find(|workspace| &workspace.id == selected)
                .cloned()
        });
        let panel = div()
            .v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(rgb(COLOR_SURFACE))
            .child(
                div()
                    .h_flex()
                    .h(px(PANE_TOOLBAR_HEIGHT))
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .child(div().font_semibold().child("Git 变更"))
                    .child(
                        div()
                            .text_size(font_px(FONT_SIZE_XS))
                            .text_color(rgb(COLOR_TEXT_TERTIARY))
                            .child("只读 · 由 Daemon 执行 Git"),
                    ),
            );

        let Some(workspace) = selected_workspace else {
            return panel.child(
                div()
                    .v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child("选择工作区后可检查 Git 状态和统一差异。"),
            );
        };
        let is_loading_status = self.git_operation_state == GitOperationState::LoadingStatus;
        let is_loading_diff = self.git_operation_state == GitOperationState::LoadingDiff;
        let can_inspect = is_online && self.git_operation_state == GitOperationState::Idle;
        let panel = panel.child(
            div()
                .h_flex()
                .h(px(PANE_TOOLBAR_HEIGHT))
                .flex_none()
                .items_center()
                .gap_2()
                .px_5()
                .border_b_1()
                .border_color(rgb(COLOR_BORDER_LIGHT))
                .text_size(font_px(FONT_SIZE_XS))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(Icon::new(AppIcon::Changes))
                .child(div().font_medium().child(workspace.name))
                .child("·")
                .child(div().truncate().child(workspace.working_directory)),
        );

        let Some(status) = self.workspace_git_status.clone() else {
            return panel.child(
                div()
                    .v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(
                        Icon::new(AppIcon::Changes)
                            .with_size(gpui_component::Size::Large)
                            .text_color(rgb(COLOR_TEXT_TERTIARY)),
                    )
                    .child("Git 状态尚未加载。桌面端不会直接执行 Git 命令。")
                    .child(
                        Button::new("workspace-git-load-status")
                            .primary()
                            .small()
                            .label("加载 Git 状态")
                            .loading(is_loading_status)
                            .disabled(!can_inspect)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.load_workspace_git_status(cx);
                            })),
                    ),
            );
        };

        if !status.is_repository {
            return panel.child(
                div()
                    .v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child("当前工作区不在 Git 仓库中。")
                    .child(
                        Button::new("workspace-git-refresh-non-repository")
                            .outline()
                            .small()
                            .label("重新检查")
                            .loading(is_loading_status)
                            .disabled(!can_inspect)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.load_workspace_git_status(cx);
                            })),
                    ),
            );
        }

        let branch = status.branch.as_deref().unwrap_or("未命名分支");
        let selected_diff_path = self
            .workspace_git_diff
            .as_ref()
            .map(|diff| diff.path.as_str());
        let change_rows = status
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                let path = change.path.clone();
                let selected = selected_diff_path == Some(change.path.as_str());
                Button::new(("workspace-git-change", index))
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(selected)
                    .icon(Icon::new(AppIcon::File))
                    .label(Self::git_change_label(change))
                    .disabled(!can_inspect)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.load_workspace_git_diff(path.clone(), cx);
                    }))
            })
            .collect::<Vec<_>>();
        let has_changes = !change_rows.is_empty();
        let changes_panel = div()
            .v_flex()
            .size_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(rgb(COLOR_BORDER_LIGHT))
            .bg(rgb(COLOR_SURFACE_UNDER))
            .when(!has_changes, |list| {
                list.child(
                    div()
                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                        .child("工作区没有未提交变更。"),
                )
            })
            .children(change_rows)
            .overflow_y_scrollbar();

        let diff_panel = if let Some(diff) = self.workspace_git_diff.clone() {
            let preview = if diff.is_binary {
                "二进制文件存在变更，不提供文本差异预览。".to_owned()
            } else if diff.unified_diff.is_empty() {
                "该路径当前没有可显示的文本差异。".to_owned()
            } else {
                diff.unified_diff.clone()
            };
            div()
                .v_flex()
                .size_full()
                .min_w(px(0.))
                .min_h(px(0.))
                .gap_3()
                .p_4()
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .child(div().font_semibold().child(diff.path))
                        .child(if diff.is_binary {
                            format!("二进制 · {} 字节", diff.byte_length)
                        } else {
                            format!("{} 字节", diff.byte_length)
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .bg(rgb(COLOR_EDITOR))
                        .font_family(mono_font_family())
                        .text_size(font_px(FONT_SIZE_MONO))
                        .line_height(px(20.))
                        .whitespace_nowrap()
                        .child(preview)
                        .overflow_scrollbar(),
                )
        } else {
            div()
                .v_flex()
                .size_full()
                .min_w(px(0.))
                .min_h(px(0.))
                .items_center()
                .justify_center()
                .bg(rgb(COLOR_SURFACE))
                .text_color(rgb(COLOR_TEXT_SECONDARY))
                .child(if is_loading_diff {
                    "正在生成统一差异…"
                } else if has_changes {
                    "选择一个变更路径以预览统一差异"
                } else {
                    "暂无差异"
                })
        };
        let view = cx.weak_entity();
        let split_panel = h_resizable("workspace-changes-split")
            .child(
                resizable_panel()
                    .size(px(self.panel_widths.workspace_changes_list()))
                    .size_range(
                        px(PanelWidths::MIN_WORKSPACE_LIST)..px(PanelWidths::MAX_WORKSPACE_LIST),
                    )
                    .child(changes_panel),
            )
            .child(resizable_panel().child(diff_panel))
            .on_resize(move |state, _, cx| {
                let Some(width) = state.read(cx).sizes().first().copied() else {
                    return;
                };
                let _ = view.update(cx, |view, cx| {
                    view.panel_widths.set_workspace_changes_list(width.as_f32());
                    view.schedule_ui_state_save(cx);
                });
            });

        panel
            .child(
                div()
                    .h_flex()
                    .h(px(PANE_TOOLBAR_HEIGHT))
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .text_size(font_px(FONT_SIZE_SM))
                    .child(format!("分支：{branch}"))
                    .child(format!("{} 项变更", status.changes.len()))
                    .child(
                        Button::new("workspace-git-refresh-status")
                            .outline()
                            .small()
                            .label("刷新")
                            .loading(is_loading_status)
                            .disabled(!can_inspect)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.load_workspace_git_status(cx);
                            })),
                    ),
            )
            .child(div().h_flex().flex_1().min_h(px(0.)).child(split_panel))
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(rgb(COLOR_BORDER_LIGHT))
                    .px_4()
                    .py_2()
                    .text_size(font_px(FONT_SIZE_XS))
                    .text_color(rgb(COLOR_TEXT_TERTIARY))
                    .child(
                        "仅展示未提交状态和差异；Daemon 禁用外部 diff/textconv，预览上限为 1 MiB。",
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{workspace_paths_affect_directory, workspace_paths_affect_file};

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn broad_change_affects_every_cached_path() {
        assert!(workspace_paths_affect_directory(&[], "src/nested"));
        assert!(workspace_paths_affect_file(&[], "src/main.rs"));
    }

    #[test]
    fn directory_change_only_refreshes_the_visible_listing() {
        let changed = paths(&["README.md", "src/main.rs", "src/nested/value.rs"]);

        assert!(workspace_paths_affect_directory(&changed, ""));
        assert!(workspace_paths_affect_directory(&changed, "src"));
        assert!(workspace_paths_affect_directory(&changed, "src/nested"));
        assert!(!workspace_paths_affect_directory(&changed, "docs"));
    }

    #[test]
    fn file_change_requires_an_exact_path_match() {
        let changed = paths(&["src", "src/main.rs", "src/nested/value.rs"]);

        assert!(workspace_paths_affect_file(&changed, "src/main.rs"));
        assert!(!workspace_paths_affect_file(&changed, "src/lib.rs"));
    }
}
