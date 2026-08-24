use super::settings::settings_page_header;
use super::timeline::composer::{MAX_PROMPT_ATTACHMENTS, load_prompt_attachments};
use super::*;
use std::{
    fs,
    io::{Read as _, Write as _},
    net::{IpAddr, SocketAddr, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration as StdDuration,
};
use url::{Host, Url};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BrowserOpenTarget {
    #[default]
    System,
    Safari,
    Chrome,
    Edge,
    Firefox,
}

impl BrowserOpenTarget {
    const ALL: [Self; 5] = [
        Self::System,
        Self::Safari,
        Self::Chrome,
        Self::Edge,
        Self::Firefox,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::System => "默认浏览器",
            Self::Safari => "Safari",
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Firefox => "Firefox",
        }
    }

    const fn macos_application_name(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::Safari => Some("Safari"),
            Self::Chrome => Some("Google Chrome"),
            Self::Edge => Some("Microsoft Edge"),
            Self::Firefox => Some("Firefox"),
        }
    }

    const fn supports_cdp(self) -> bool {
        matches!(self, Self::Chrome | Self::Edge)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ComputerControlPreferences {
    pub(super) observe_applications: bool,
    pub(super) allow_actions: bool,
    pub(super) allowed_applications: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct AppSnapshotPreferences {
    pub(super) include_window_shadow: bool,
    pub(super) play_sound: bool,
}

impl Default for AppSnapshotPreferences {
    fn default() -> Self {
        Self {
            include_window_shadow: false,
            play_sound: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct BrowserIntegrationPreferences {
    pub(super) enabled: bool,
    pub(super) open_target: BrowserOpenTarget,
    pub(super) connector_endpoint: String,
    pub(super) include_page_screenshots: bool,
    pub(super) allowed_domains: Vec<String>,
}

impl Default for BrowserIntegrationPreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            open_target: BrowserOpenTarget::System,
            connector_endpoint: "http://127.0.0.1:9222".into(),
            include_page_screenshots: true,
            allowed_domains: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct IntegrationPreferences {
    pub(super) computer_control: ComputerControlPreferences,
    pub(super) app_snapshot: AppSnapshotPreferences,
    pub(super) browser: BrowserIntegrationPreferences,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum IntegrationProbeState {
    #[default]
    NotChecked,
    Checking,
    Ready(String),
    Blocked(String),
}

impl IntegrationProbeState {
    fn badge(&self) -> (&'static str, u32) {
        match self {
            Self::NotChecked => ("未检查", COLOR_TEXT_TERTIARY),
            Self::Checking => ("正在检查", COLOR_TEXT_SECONDARY),
            Self::Ready(_) => ("可用", COLOR_SUCCESS),
            Self::Blocked(_) => ("需要处理", COLOR_WARNING),
        }
    }

    fn detail(&self) -> &'_ str {
        match self {
            Self::NotChecked => "尚未检查当前系统状态。",
            Self::Checking => "正在检查当前系统状态…",
            Self::Ready(detail) | Self::Blocked(detail) => detail,
        }
    }

    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

fn integration_row(
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

fn integration_divider() -> Div {
    div().h(px(1.)).w_full().bg(rgb(COLOR_BORDER_LIGHT))
}

fn integration_status_badge(state: &IntegrationProbeState) -> Div {
    let (label, color) = state.badge();
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
        .child(label)
}

fn status_detail(state: &IntegrationProbeState) -> Div {
    div()
        .text_size(font_px(FONT_SIZE_XS))
        .line_height(px(18.))
        .text_color(rgb(COLOR_TEXT_TERTIARY))
        .child(state.detail().to_owned())
}

fn validate_application_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("请输入应用名称");
    }
    if value.chars().count() > 80 || value.chars().any(char::is_control) {
        return Err("应用名称过长或包含无效字符");
    }
    Ok(value.to_owned())
}

fn normalize_domain(value: &str) -> Result<String, &'static str> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err("请输入域名");
    }
    let (wildcard, candidate) = value
        .strip_prefix("*.")
        .map_or((false, value.as_str()), |domain| (true, domain));
    let parsed = Url::parse(&format!("https://{candidate}"))
        .map_err(|_| "请输入有效域名，例如 example.com")?;
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty() && !host.contains(['/', ' ']))
        .ok_or("请输入有效域名，例如 example.com")?;
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("域名不能包含路径、查询参数或片段");
    }
    Ok(if wildcard {
        format!("*.{host}")
    } else {
        host.to_owned()
    })
}

fn loopback_connector_address(endpoint: &str) -> Result<(SocketAddr, String), String> {
    let url = Url::parse(endpoint).map_err(|_| "连接地址不是有效 URL".to_owned())?;
    if url.scheme() != "http" {
        return Err("浏览器连接器仅允许使用本机 HTTP 地址".into());
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "浏览器连接地址缺少端口".to_owned())?;
    let ip = match url.host() {
        Some(Host::Domain(host)) if host.eq_ignore_ascii_case("localhost") => {
            IpAddr::from([127, 0, 0, 1])
        }
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip),
        Some(Host::Ipv6(ip)) => IpAddr::V6(ip),
        Some(Host::Domain(_)) => {
            return Err("浏览器连接器只能使用 localhost 或回环 IP".into());
        }
        None => return Err("浏览器连接地址缺少主机".into()),
    };
    if !ip.is_loopback() {
        return Err("为避免暴露调试协议，浏览器连接器只能监听回环地址".into());
    }
    Ok((SocketAddr::new(ip, port), url.to_string()))
}

fn probe_browser_connector(endpoint: &str) -> Result<String, String> {
    let (address, endpoint) = loopback_connector_address(endpoint)?;
    let mut stream = TcpStream::connect_timeout(&address, StdDuration::from_secs(2))
        .map_err(|error| format!("无法连接 {address}：{error}"))?;
    stream
        .set_read_timeout(Some(StdDuration::from_secs(2)))
        .map_err(|error| format!("无法设置读取超时：{error}"))?;
    stream
        .set_write_timeout(Some(StdDuration::from_secs(2)))
        .map_err(|error| format!("无法设置写入超时：{error}"))?;
    let host = address.ip();
    let request = format!(
        "GET /json/version HTTP/1.0\r\nHost: {host}:{}\r\nConnection: close\r\n\r\n",
        address.port()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("无法请求浏览器调试信息：{error}"))?;
    let mut response = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut response)
        .map_err(|error| format!("无法读取浏览器调试信息：{error}"))?;
    let response = String::from_utf8_lossy(&response);
    if !response.starts_with("HTTP/1.0 200") && !response.starts_with("HTTP/1.1 200") {
        return Err("端口可以访问，但没有返回 Chrome DevTools Protocol 信息".into());
    }
    if !response.contains("webSocketDebuggerUrl") && !response.contains("\"Browser\"") {
        return Err("服务响应不是有效的浏览器调试端点".into());
    }
    Ok(format!("已连接本机浏览器调试端点 {endpoint}"))
}

#[cfg(target_os = "macos")]
fn check_accessibility_permission() -> Result<String, String> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application \"System Events\" to get UI elements enabled",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("无法检查辅助功能权限：{error}"))?;
    if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" {
        Ok("Corbit 已获得辅助功能访问，可读取允许应用的界面结构。".into())
    } else {
        Err("尚未获得辅助功能访问，请在系统设置中允许 Corbit 后重新检查。".into())
    }
}

#[cfg(not(target_os = "macos"))]
fn check_accessibility_permission() -> Result<String, String> {
    Err("电脑操控第一阶段仅在 macOS 提供。".into())
}

#[cfg(target_os = "macos")]
fn open_privacy_settings(anchor: &str) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{anchor}"
        ))
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开系统设置：{error}"))
}

#[cfg(not(target_os = "macos"))]
fn open_privacy_settings(_anchor: &str) -> Result<(), String> {
    Err("当前平台暂不支持直接打开该权限页面。".into())
}

#[cfg(target_os = "macos")]
fn launch_application(name: &str) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("-a")
        .arg(name)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动 {name}：{error}"))
}

#[cfg(not(target_os = "macos"))]
fn launch_application(_name: &str) -> Result<(), String> {
    Err("应用启动第一阶段仅支持 macOS。".into())
}

#[cfg(target_os = "macos")]
fn browser_is_installed(target: BrowserOpenTarget) -> bool {
    let Some(name) = target.macos_application_name() else {
        return true;
    };
    let system = PathBuf::from("/Applications").join(format!("{name}.app"));
    let user = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Applications").join(format!("{name}.app")));
    system.is_dir() || user.is_some_and(|path| path.is_dir())
}

#[cfg(not(target_os = "macos"))]
fn browser_is_installed(target: BrowserOpenTarget) -> bool {
    target == BrowserOpenTarget::System
}

#[cfg(target_os = "macos")]
fn open_browser(target: BrowserOpenTarget, url: &str) -> Result<(), String> {
    let mut command = Command::new("/usr/bin/open");
    if let Some(application) = target.macos_application_name() {
        command.arg("-a").arg(application);
    }
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开浏览器：{error}"))
}

#[cfg(not(target_os = "macos"))]
fn open_browser(_target: BrowserOpenTarget, url: &str) -> Result<(), String> {
    let command = if cfg!(target_os = "windows") {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    Command::new(command)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开浏览器：{error}"))
}

#[cfg(target_os = "macos")]
fn launch_controlled_browser(
    target: BrowserOpenTarget,
    connector_endpoint: &str,
) -> Result<(), String> {
    if !target.supports_cdp() {
        return Err("请选择 Google Chrome 或 Microsoft Edge。".into());
    }
    let (address, _) = loopback_connector_address(connector_endpoint)?;
    let profile = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定当前用户目录".to_owned())?
        .join("Library/Application Support/Corbit/BrowserProfile");
    fs::create_dir_all(&profile).map_err(|error| format!("无法创建受控浏览器配置：{error}"))?;
    let application = target
        .macos_application_name()
        .ok_or_else(|| "请选择支持的 Chromium 浏览器".to_owned())?;
    Command::new("/usr/bin/open")
        .args(["-na", application, "--args"])
        .arg(format!("--remote-debugging-port={}", address.port()))
        .arg(format!("--user-data-dir={}", profile.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动受控浏览器：{error}"))
}

#[cfg(not(target_os = "macos"))]
fn launch_controlled_browser(
    _target: BrowserOpenTarget,
    _connector_endpoint: &str,
) -> Result<(), String> {
    Err("受控浏览器启动第一阶段仅支持 macOS。".into())
}

#[cfg(target_os = "macos")]
fn capture_window_snapshot(include_shadow: bool) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("corbit-appshot-{}.jpg", uuid::Uuid::new_v4()));
    let mut command = Command::new("/usr/sbin/screencapture");
    command.args(["-i", "-W", "-t", "jpg", "-x"]);
    if !include_shadow {
        command.arg("-o");
    }
    let status = command
        .arg(&path)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("无法启动窗口快照：{error}"))?;
    let valid_file = fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 0);
    if !status.success() || !valid_file {
        let _ = fs::remove_file(&path);
        return Err("快照已取消，或 Corbit 尚未获得屏幕录制权限。".into());
    }
    Ok(path)
}

#[cfg(not(target_os = "macos"))]
fn capture_window_snapshot(_include_shadow: bool) -> Result<PathBuf, String> {
    Err("应用快照第一阶段仅支持 macOS。".into())
}

#[cfg(target_os = "macos")]
fn play_snapshot_sound() {
    let _ = Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn play_snapshot_sound() {}

impl ConnectionView {
    fn persist_integration_preferences(&mut self, success: &'static str, cx: &mut Context<Self>) {
        match self.ui_preferences(cx).save() {
            Ok(()) => {
                self.ui_state_error = None;
                self.show_success(success, cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.ui_state_error = Some(message.clone());
                self.show_error(format!("集成设置保存失败：{message}"), cx);
            }
        }
    }

    pub(super) fn capture_app_snapshot_action(
        &mut self,
        _: &CaptureAppSnapshot,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_app_snapshot(cx);
    }

    fn set_computer_observation(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled && !self.computer_access_status.is_ready() {
            self.show_warning("请先检查并授予辅助功能权限", cx);
            return;
        }
        self.integration_preferences
            .computer_control
            .observe_applications = enabled;
        if !enabled {
            self.integration_preferences.computer_control.allow_actions = false;
        }
        self.persist_integration_preferences("电脑操控设置已更新", cx);
    }

    fn set_computer_actions(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let preferences = &self.integration_preferences.computer_control;
        if enabled
            && (!self.computer_access_status.is_ready()
                || !preferences.observe_applications
                || preferences.allowed_applications.is_empty())
        {
            self.show_warning(
                "允许操作前需要辅助功能权限、读取开关和至少一个应用白名单",
                cx,
            );
            return;
        }
        self.integration_preferences.computer_control.allow_actions = enabled;
        self.persist_integration_preferences("电脑操控设置已更新", cx);
    }

    fn check_computer_access(&mut self, cx: &mut Context<Self>) {
        if self.computer_access_task.is_some() {
            return;
        }
        self.computer_access_status = IntegrationProbeState::Checking;
        let task = cx.background_spawn(async { check_accessibility_permission() });
        self.computer_access_task = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.computer_access_task = None;
                view.computer_access_status = match result {
                    Ok(detail) => IntegrationProbeState::Ready(detail),
                    Err(detail) => IntegrationProbeState::Blocked(detail),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn open_accessibility_settings(&mut self, cx: &mut Context<Self>) {
        match open_privacy_settings("Privacy_Accessibility") {
            Ok(()) => self.show_info("已打开辅助功能隐私设置", cx),
            Err(error) => self.show_error(error, cx),
        }
    }

    fn add_allowed_application(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = Self::input_value(&self.computer_allowed_application, cx);
        let application = match validate_application_name(&value) {
            Ok(application) => application,
            Err(error) => {
                self.show_error(error, cx);
                return;
            }
        };
        let applications = &mut self
            .integration_preferences
            .computer_control
            .allowed_applications;
        if applications
            .iter()
            .any(|saved| saved.eq_ignore_ascii_case(&application))
        {
            self.show_info("这个应用已经在白名单中", cx);
            return;
        }
        applications.push(application);
        applications.sort_by_key(|value| value.to_ascii_lowercase());
        self.computer_allowed_application
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.persist_integration_preferences("应用白名单已更新", cx);
    }

    fn remove_allowed_application(&mut self, application: &str, cx: &mut Context<Self>) {
        self.integration_preferences
            .computer_control
            .allowed_applications
            .retain(|saved| saved != application);
        if self
            .integration_preferences
            .computer_control
            .allowed_applications
            .is_empty()
        {
            self.integration_preferences.computer_control.allow_actions = false;
        }
        self.persist_integration_preferences("应用白名单已更新", cx);
    }

    fn launch_allowed_application(&mut self, application: &str, cx: &mut Context<Self>) {
        match launch_application(application) {
            Ok(()) => self.show_success(format!("正在打开 {application}"), cx),
            Err(error) => self.show_error(error, cx),
        }
    }

    pub(super) fn capture_app_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.app_snapshot_task.is_some() || self.attachment_in_flight {
            self.show_warning("另一个附件或快照操作正在进行", cx);
            return;
        }
        let Some(agent_id) = self.selected_agent_id.clone() else {
            self.show_warning("请先打开一个任务，再截取应用快照", cx);
            return;
        };
        if self.prompt_attachments.len() >= MAX_PROMPT_ATTACHMENTS {
            self.show_validation_error(
                format!("每条消息最多可添加 {MAX_PROMPT_ATTACHMENTS} 个附件"),
                cx,
            );
            return;
        }
        let include_shadow = self
            .integration_preferences
            .app_snapshot
            .include_window_shadow;
        let play_sound = self.integration_preferences.app_snapshot.play_sound;
        let existing_bytes = self
            .prompt_attachments
            .iter()
            .map(|attachment| attachment.size_bytes)
            .sum();
        self.snapshot_capture_status = IntegrationProbeState::Checking;
        self.attachment_in_flight = true;
        let task = cx.background_spawn(async move {
            let path = capture_window_snapshot(include_shadow)?;
            let loaded = load_prompt_attachments(vec![path.clone()], 1, existing_bytes);
            let _ = fs::remove_file(path);
            loaded.and_then(|mut attachments| {
                attachments
                    .pop()
                    .ok_or_else(|| "没有生成可用的快照附件".to_owned())
            })
        });
        self.app_snapshot_task = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.app_snapshot_task = None;
                view.attachment_in_flight = false;
                match result {
                    Ok(attachment) if view.selected_agent_id.as_deref() == Some(&agent_id) => {
                        view.prompt_attachments.push(attachment);
                        view.snapshot_capture_status = IntegrationProbeState::Ready(
                            "最近一次窗口快照已添加到当前任务输入框。".into(),
                        );
                        if play_sound {
                            play_snapshot_sound();
                        }
                        view.show_success("应用快照已添加到当前任务", cx);
                    }
                    Ok(_) => {
                        view.snapshot_capture_status = IntegrationProbeState::Blocked(
                            "任务已切换，快照未添加；请在目标任务中重试。".into(),
                        );
                        view.show_warning("任务已切换，快照未添加", cx);
                    }
                    Err(error) => {
                        view.snapshot_capture_status =
                            IntegrationProbeState::Blocked(error.clone());
                        view.show_error(error, cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn open_screen_recording_settings(&mut self, cx: &mut Context<Self>) {
        match open_privacy_settings("Privacy_ScreenCapture") {
            Ok(()) => self.show_info("已打开屏幕录制隐私设置", cx),
            Err(error) => self.show_error(error, cx),
        }
    }

    fn set_snapshot_shadow(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.integration_preferences
            .app_snapshot
            .include_window_shadow = enabled;
        self.persist_integration_preferences("应用快照设置已更新", cx);
    }

    fn set_snapshot_sound(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.integration_preferences.app_snapshot.play_sound = enabled;
        self.persist_integration_preferences("应用快照设置已更新", cx);
    }

    fn choose_browser_target(&mut self, target: BrowserOpenTarget, cx: &mut Context<Self>) {
        self.integration_preferences.browser.open_target = target;
        self.browser_connection_status = IntegrationProbeState::NotChecked;
        self.persist_integration_preferences("浏览器打开方式已更新", cx);
    }

    fn set_browser_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled && !self.browser_connection_status.is_ready() {
            self.show_warning("请先连接并验证本机浏览器调试端点", cx);
            return;
        }
        self.integration_preferences.browser.enabled = enabled;
        self.persist_integration_preferences("浏览器连接设置已更新", cx);
    }

    fn set_page_screenshots(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.integration_preferences
            .browser
            .include_page_screenshots = enabled;
        self.persist_integration_preferences("浏览器截图设置已更新", cx);
    }

    fn test_browser_connection(&mut self, cx: &mut Context<Self>) {
        if self.browser_connection_task.is_some() {
            return;
        }
        let endpoint = Self::input_value(&self.browser_connector_endpoint, cx);
        if let Err(error) = loopback_connector_address(&endpoint) {
            self.show_error(error, cx);
            return;
        }
        self.browser_connection_status = IntegrationProbeState::Checking;
        let task = cx.background_spawn(async move { probe_browser_connector(&endpoint) });
        self.browser_connection_task = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.browser_connection_task = None;
                view.browser_connection_status = match result {
                    Ok(detail) => {
                        view.integration_preferences.browser.connector_endpoint =
                            Self::input_value(&view.browser_connector_endpoint, cx);
                        view.schedule_ui_state_save(cx);
                        IntegrationProbeState::Ready(detail)
                    }
                    Err(detail) => {
                        view.integration_preferences.browser.enabled = false;
                        IntegrationProbeState::Blocked(detail)
                    }
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn launch_browser_test_page(&mut self, cx: &mut Context<Self>) {
        let target = self.integration_preferences.browser.open_target;
        if target != BrowserOpenTarget::System && !browser_is_installed(target) {
            self.show_error(format!("未检测到 {}", target.label()), cx);
            return;
        }
        match open_browser(target, "https://example.com") {
            Ok(()) => self.show_success("已在所选浏览器中打开测试页", cx),
            Err(error) => self.show_error(error, cx),
        }
    }

    fn launch_controlled_browser(&mut self, cx: &mut Context<Self>) {
        let target = self.integration_preferences.browser.open_target;
        let endpoint = Self::input_value(&self.browser_connector_endpoint, cx);
        match launch_controlled_browser(target, &endpoint) {
            Ok(()) => {
                self.browser_connection_status = IntegrationProbeState::NotChecked;
                self.show_info("受控浏览器已启动；页面加载后点击“测试连接”", cx);
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    fn add_browser_domain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = Self::input_value(&self.browser_allowed_domain, cx);
        let domain = match normalize_domain(&value) {
            Ok(domain) => domain,
            Err(error) => {
                self.show_error(error, cx);
                return;
            }
        };
        let domains = &mut self.integration_preferences.browser.allowed_domains;
        if domains.iter().any(|saved| saved == &domain) {
            self.show_info("这个域名已经在允许列表中", cx);
            return;
        }
        domains.push(domain);
        domains.sort();
        self.browser_allowed_domain
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.persist_integration_preferences("浏览器域名策略已更新", cx);
    }

    fn remove_browser_domain(&mut self, domain: &str, cx: &mut Context<Self>) {
        self.integration_preferences
            .browser
            .allowed_domains
            .retain(|saved| saved != domain);
        self.persist_integration_preferences("浏览器域名策略已更新", cx);
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_computer_control_settings(&self, cx: &mut Context<Self>) -> Div {
        let preferences = &self.integration_preferences.computer_control;
        let access_ready = self.computer_access_status.is_ready();
        let mut applications = settings_card("始终允许的应用").child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .line_height(px(18.))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child("仅白名单中的应用可以被激活或操作；敏感操作仍会要求单独批准。"),
        );
        if preferences.allowed_applications.is_empty() {
            applications = applications.child(
                div()
                    .py_2()
                    .text_size(font_px(FONT_SIZE_SM))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child("尚未添加应用。建议按需添加，不要授予任意应用权限。"),
            );
        }
        for (index, application) in preferences.allowed_applications.iter().enumerate() {
            if index > 0 {
                applications = applications.child(integration_divider());
            }
            let launch_name = application.clone();
            let remove_name = application.clone();
            applications = applications.child(integration_row(
                application.clone(),
                "允许 Corbit 在授权范围内读取或激活这个应用。",
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        settings_action_button(("computer-open-app", index), cx)
                            .label("打开")
                            .disabled(!preferences.allow_actions)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.launch_allowed_application(&launch_name, cx);
                            })),
                    )
                    .child(
                        settings_danger_action_button(("computer-remove-app", index), cx)
                            .label("移除")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.remove_allowed_application(&remove_name, cx);
                            })),
                    ),
            ));
        }
        applications = applications.child(
            div()
                .h_flex()
                .items_end()
                .gap_3()
                .child(settings_input(&self.computer_allowed_application).flex_1())
                .child(
                    settings_primary_action_button("computer-add-app", cx)
                        .label("添加应用")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.add_allowed_application(window, cx);
                        })),
                ),
        );

        settings_page_header(
            "电脑操控",
            "管理 Corbit 如何读取和操作这台电脑上的其他应用。",
        )
        .child(
            settings_card("控制")
                .child(integration_row(
                    "辅助功能权限",
                    "macOS 使用辅助功能权限读取窗口控件；Corbit 不会绕过系统授权。",
                    integration_status_badge(&self.computer_access_status),
                ))
                .child(status_detail(&self.computer_access_status))
                .child(
                    div()
                        .h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            settings_action_button("computer-open-accessibility", cx)
                                .label("打开系统设置")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.open_accessibility_settings(cx);
                                })),
                        )
                        .child(
                            settings_primary_action_button("computer-check-access", cx)
                                .icon(Icon::new(AppIcon::Refresh))
                                .label("检查权限")
                                .loading(self.computer_access_task.is_some())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.check_computer_access(cx);
                                })),
                        ),
                )
                .child(integration_divider())
                .child(integration_row(
                    "读取应用状态",
                    "允许读取白名单应用的窗口标题和界面结构，不执行点击或输入。",
                    settings_switch("computer-observe", preferences.observe_applications)
                        .disabled(!access_ready)
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_computer_observation(*checked, cx);
                        })),
                ))
                .child(integration_divider())
                .child(integration_row(
                    "允许受控操作",
                    "允许在白名单应用中激活窗口；点击、输入和高风险动作仍需任务授权。",
                    settings_switch("computer-actions", preferences.allow_actions)
                        .disabled(
                            !access_ready
                                || !preferences.observe_applications
                                || preferences.allowed_applications.is_empty(),
                        )
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_computer_actions(*checked, cx);
                        })),
                )),
        )
        .child(applications)
        .child(
            settings_card("安全边界").child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .line_height(px(19.))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(
                        "Corbit 不提供“任意应用”默认授权。密码管理器、支付、系统安全设置和删除操作不会因这里的开关自动获批。",
                    ),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_app_snapshot_settings(&self, cx: &mut Context<Self>) -> Div {
        let preferences = self.integration_preferences.app_snapshot;
        settings_page_header(
            "应用快照",
            "截取一个应用窗口，并把图像直接添加到当前任务输入框。",
        )
        .child(
            settings_card("截取应用快照")
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap_4()
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
                                        .child("选择要截取的窗口"),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_XS))
                                        .line_height(px(18.))
                                        .text_color(rgb(COLOR_TEXT_TERTIARY))
                                        .child(
                                            "启动后点击目标窗口；快照完成后可在当前任务发送前预览和移除。",
                                        ),
                                ),
                        )
                        .child(
                            settings_primary_action_button("snapshot-capture", cx)
                                .icon(Icon::new(AppIcon::Snapshot))
                                .label("截取窗口")
                                .loading(self.app_snapshot_task.is_some())
                                .disabled(
                                    self.selected_agent_id.is_none()
                                        || self.app_snapshot_task.is_some()
                                        || self.attachment_in_flight,
                                )
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.capture_app_snapshot(cx);
                                })),
                        ),
                )
                .child(status_detail(&self.snapshot_capture_status)),
        )
        .child(
            settings_card("快照设置")
                .child(integration_row(
                    "发送目标",
                    "快照不会自动发送；它会进入当前任务的附件列表。",
                    div()
                        .rounded(px(8.))
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .px_3()
                        .py_1()
                        .text_size(font_px(FONT_SIZE_XS))
                        .child("当前任务附件"),
                ))
                .child(integration_divider())
                .child(integration_row(
                    "应用内快捷键",
                    "Corbit 窗口激活时可快速启动窗口选择。",
                    div()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(COLOR_BORDER_HEAVY))
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .px_2()
                        .py_1()
                        .font_family(mono_font_family())
                        .text_size(font_px(FONT_SIZE_XS))
                        .child("⇧⌘2"),
                ))
                .child(integration_divider())
                .child(integration_row(
                    "包含窗口阴影",
                    "关闭后生成更紧凑的窗口图像。",
                    settings_switch("snapshot-shadow", preferences.include_window_shadow)
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_snapshot_shadow(*checked, cx);
                        })),
                ))
                .child(integration_divider())
                .child(integration_row(
                    "播放音效",
                    "成功添加快照后播放系统提示音。",
                    settings_switch("snapshot-sound", preferences.play_sound).on_click(
                        cx.listener(|view, checked, _, cx| {
                            view.set_snapshot_sound(*checked, cx);
                        }),
                    ),
                ))
                .child(
                    div().h_flex().justify_end().child(
                        settings_action_button("snapshot-open-privacy", cx)
                            .label("屏幕录制权限")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.open_screen_recording_settings(cx);
                            })),
                    ),
                ),
        )
        .child(
            settings_card("隐私说明").child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .line_height(px(19.))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(
                        "窗口图像只会作为当前消息附件保存在内存中；临时快照文件在读取后立即删除。请不要截取密码、支付或其他敏感信息。",
                    ),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn render_browser_integration_settings(&self, cx: &mut Context<Self>) -> Div {
        let preferences = &self.integration_preferences.browser;
        let target = preferences.open_target;
        let browser_view = cx.entity();
        let target_button = settings_select_button("browser-open-target", cx)
            .label(target.label())
            .dropdown_menu(move |menu, _, _| {
                let mut menu = settings_select_menu(menu);
                for choice in BrowserOpenTarget::ALL {
                    let item_view = browser_view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(choice.label())
                            .checked(target == choice)
                            .on_click(move |_, _, cx| {
                                item_view.update(cx, |view, cx| {
                                    view.choose_browser_target(choice, cx);
                                });
                            }),
                    );
                }
                menu
            });
        let mut domains = settings_card("允许的域名").child(
            div()
                .text_size(font_px(FONT_SIZE_XS))
                .line_height(px(18.))
                .text_color(rgb(COLOR_TEXT_TERTIARY))
                .child(
                    "留空时浏览器连接保持只读测试状态；添加域名后，后续页面操作只能发生在允许范围内。支持 *.example.com。",
                ),
        );
        if preferences.allowed_domains.is_empty() {
            domains = domains.child(
                div()
                    .py_2()
                    .text_size(font_px(FONT_SIZE_SM))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child("尚未允许任何网站。"),
            );
        }
        for (index, domain) in preferences.allowed_domains.iter().enumerate() {
            if index > 0 {
                domains = domains.child(integration_divider());
            }
            let remove_domain = domain.clone();
            domains = domains.child(integration_row(
                domain.clone(),
                "仅允许匹配这个域名的网页进入自动化范围。",
                settings_danger_action_button(("browser-remove-domain", index), cx)
                    .label("移除")
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.remove_browser_domain(&remove_domain, cx);
                    })),
            ));
        }
        domains = domains.child(
            div()
                .h_flex()
                .items_end()
                .gap_3()
                .child(settings_input(&self.browser_allowed_domain).flex_1())
                .child(
                    settings_primary_action_button("browser-add-domain", cx)
                        .label("添加域名")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.add_browser_domain(window, cx);
                        })),
                ),
        );

        settings_page_header(
            "浏览器连接",
            "连接本机浏览器调试端点，并限制 Corbit 可以访问的网站范围。",
        )
        .child(
            settings_card("浏览器")
                .child(integration_row(
                    "网页和链接打开位置",
                    "普通链接默认使用这里选择的浏览器。",
                    target_button,
                ))
                .child(integration_divider())
                .child(integration_row(
                    "安装状态",
                    "检查所选浏览器是否安装在标准应用目录。",
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(rgb(if browser_is_installed(target) {
                                    COLOR_SUCCESS
                                } else {
                                    COLOR_WARNING
                                })),
                        )
                        .text_size(font_px(FONT_SIZE_XS))
                        .child(if browser_is_installed(target) {
                            "已检测到"
                        } else {
                            "未检测到"
                        }),
                ))
                .child(
                    div().h_flex().justify_end().child(
                        settings_action_button("browser-open-test-page", cx)
                            .icon(Icon::new(AppIcon::ExternalLink))
                            .label("打开测试页")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.launch_browser_test_page(cx);
                            })),
                    ),
                ),
        )
        .child(
            settings_card("本机连接器")
                .child(integration_row(
                    "连接状态",
                    "仅允许 localhost 或回环 IP，避免 Chrome 调试协议暴露到局域网。",
                    integration_status_badge(&self.browser_connection_status),
                ))
                .child(status_detail(&self.browser_connection_status))
                .child(settings_input(&self.browser_connector_endpoint))
                .child(
                    div()
                        .h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            settings_action_button("browser-launch-controlled", cx)
                                .label("启动受控浏览器")
                                .disabled(!target.supports_cdp())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.launch_controlled_browser(cx);
                                })),
                        )
                        .child(
                            settings_primary_action_button("browser-test-connector", cx)
                                .icon(Icon::new(AppIcon::Refresh))
                                .label("测试连接")
                                .loading(self.browser_connection_task.is_some())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.test_browser_connection(cx);
                                })),
                        ),
                )
                .child(integration_divider())
                .child(integration_row(
                    "启用浏览器连接",
                    "只有调试端点验证成功后才能开启；域名策略仍会继续生效。",
                    settings_switch("browser-enabled", preferences.enabled)
                        .disabled(!self.browser_connection_status.is_ready())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_browser_enabled(*checked, cx);
                        })),
                ))
                .child(integration_divider())
                .child(integration_row(
                    "包含页面截图",
                    "页面交给任务时同时保留可见区域图像，可能增加上下文用量。",
                    settings_switch(
                        "browser-page-screenshots",
                        preferences.include_page_screenshots,
                    )
                    .on_click(cx.listener(|view, checked, _, cx| {
                        view.set_page_screenshots(*checked, cx);
                    })),
                )),
        )
        .child(domains)
        .child(
            settings_card("数据边界").child(
                div()
                    .text_size(font_px(FONT_SIZE_XS))
                    .line_height(px(19.))
                    .text_color(rgb(COLOR_TEXT_SECONDARY))
                    .child(
                        "Corbit 不保存浏览器密码、联系人或自动填充数据。受控浏览器使用独立配置目录，不会直接复用个人浏览器会话。",
                    ),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_defaults_keep_sensitive_capabilities_disabled() {
        let preferences = IntegrationPreferences::default();
        assert!(!preferences.computer_control.observe_applications);
        assert!(!preferences.computer_control.allow_actions);
        assert!(!preferences.browser.enabled);
        assert!(preferences.browser.include_page_screenshots);
    }

    #[test]
    fn integration_preferences_accept_partial_json() {
        let preferences: IntegrationPreferences = serde_json::from_str(
            r#"{"appSnapshot":{"playSound":false},"browser":{"openTarget":"chrome"}}"#,
        )
        .expect("partial integration preferences should decode");
        assert!(!preferences.app_snapshot.play_sound);
        assert_eq!(preferences.browser.open_target, BrowserOpenTarget::Chrome);
        assert!(!preferences.browser.enabled);
    }

    #[test]
    fn browser_connector_only_accepts_loopback_http() {
        assert!(loopback_connector_address("http://127.0.0.1:9222").is_ok());
        assert!(loopback_connector_address("http://localhost:9222").is_ok());
        assert!(loopback_connector_address("http://[::1]:9222").is_ok());
        assert!(loopback_connector_address("https://127.0.0.1:9222").is_err());
        assert!(loopback_connector_address("http://192.168.1.20:9222").is_err());
        assert!(loopback_connector_address("http://example.com:9222").is_err());
    }

    #[test]
    fn domain_policy_normalizes_exact_and_wildcard_hosts() {
        assert_eq!(normalize_domain(" Example.COM "), Ok("example.com".into()));
        assert_eq!(
            normalize_domain("*.Docs.Example.com"),
            Ok("*.docs.example.com".into())
        );
        assert!(normalize_domain("example.com/path").is_err());
    }
}
