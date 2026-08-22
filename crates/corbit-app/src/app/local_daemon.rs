use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, bail};
use health::HealthStatus;
use ownership::{DaemonLaunchMode, DaemonOwner, OwnershipRecord};
use url::Url;
use uuid::Uuid;

mod health;
mod ownership;
mod runtime;
mod service;

const EXPECTED_DAEMON_VERSION: &str = env!("CORBIT_DAEMON_VERSION");
const START_TIMEOUT: Duration = Duration::from_secs(10);
const START_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCE_STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DaemonPhase {
    Unmanaged,
    Checking,
    Offline,
    Restarting,
    Ready,
    Blocked,
    Failed,
}

impl DaemonPhase {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Unmanaged => "未托管",
            Self::Checking => "正在检测",
            Self::Offline => "未运行",
            Self::Restarting => "正在安全重启",
            Self::Ready => "运行正常",
            Self::Blocked => "需要处理",
            Self::Failed => "检测失败",
        }
    }

    pub(super) const fn is_busy(self) -> bool {
        matches!(self, Self::Checking | Self::Restarting)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DaemonStatus {
    pub(super) phase: DaemonPhase,
    pub(super) expected_version: String,
    pub(super) version: Option<String>,
    pub(super) pid: Option<u32>,
    pub(super) desktop_owned: bool,
    pub(super) launch_mode: Option<DaemonLaunchMode>,
    pub(super) node: Option<PathBuf>,
    pub(super) runtime_path: Option<PathBuf>,
    pub(super) log_path: Option<PathBuf>,
    pub(super) detail: String,
}

impl DaemonStatus {
    pub(super) fn checking() -> Self {
        Self::transient(DaemonPhase::Checking, "正在检查本机 Daemon 状态…")
    }

    pub(super) fn restarting() -> Self {
        Self::transient(
            DaemonPhase::Restarting,
            "正在确认桌面端所有权并安全重启 Daemon…",
        )
    }

    pub(super) fn failed(error: &anyhow::Error) -> Self {
        Self {
            phase: DaemonPhase::Failed,
            expected_version: EXPECTED_DAEMON_VERSION.into(),
            version: None,
            pid: None,
            desktop_owned: false,
            launch_mode: Some(service::preferred_mode()),
            node: None,
            runtime_path: runtime::detected_installed_runtime_path(),
            log_path: runtime::log_path().ok(),
            detail: error.to_string(),
        }
    }

    fn transient(phase: DaemonPhase, detail: &str) -> Self {
        Self {
            phase,
            expected_version: EXPECTED_DAEMON_VERSION.into(),
            version: None,
            pid: None,
            desktop_owned: false,
            launch_mode: Some(service::preferred_mode()),
            node: None,
            runtime_path: runtime::detected_installed_runtime_path(),
            log_path: runtime::log_path().ok(),
            detail: detail.into(),
        }
    }

    pub(super) fn diagnostics(&self, endpoint: &str) -> String {
        let value = |value: Option<String>| value.unwrap_or_else(|| "无".into());
        format!(
            "Corbit Desktop Daemon 诊断\n状态：{}\n端点：{}\n期望版本：{}\n运行版本：{}\nPID：{}\n所有权：{}\n托管方式：{}\nNode：{}\n运行包：{}\n日志：{}\n详情：{}",
            self.phase.label(),
            sanitized_endpoint(endpoint),
            self.expected_version,
            value(self.version.clone()),
            value(self.pid.map(|pid| pid.to_string())),
            if self.desktop_owned {
                "Corbit Desktop"
            } else {
                "外部或无"
            },
            value(self.launch_mode.map(|mode| mode.label().into())),
            value(self.node.as_ref().map(|path| path.display().to_string())),
            value(
                self.runtime_path
                    .as_ref()
                    .map(|path| path.display().to_string())
            ),
            value(
                self.log_path
                    .as_ref()
                    .map(|path| path.display().to_string())
            ),
            self.detail,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnsureOutcome {
    NotManaged,
    AlreadyRunning,
    Started,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EnsureResult {
    pub(super) outcome: EnsureOutcome,
    pub(super) status: DaemonStatus,
}

pub(super) async fn ensure_available(endpoint: String) -> anyhow::Result<EnsureResult> {
    run_blocking("corbit-daemon-preflight", move || {
        ensure_available_blocking(&endpoint)
    })
    .await
}

pub(super) async fn diagnose(endpoint: String) -> anyhow::Result<DaemonStatus> {
    run_blocking("corbit-daemon-diagnostics", move || {
        Ok(diagnose_blocking(&endpoint))
    })
    .await
}

pub(super) async fn restart_owned(endpoint: String) -> anyhow::Result<DaemonStatus> {
    run_blocking("corbit-daemon-restart", move || {
        restart_owned_blocking(&endpoint)
    })
    .await
}

async fn run_blocking<T, F>(name: &str, operation: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    let (sender, receiver) = async_channel::bounded(1);
    thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = sender.send_blocking(operation());
        })
        .with_context(|| format!("无法创建 {name} 后台线程"))?;
    receiver
        .recv()
        .await
        .with_context(|| format!("{name} 后台线程意外退出"))?
}

fn ensure_available_blocking(endpoint: &str) -> anyhow::Result<EnsureResult> {
    let Some(health_url) = health::managed_health_url(endpoint) else {
        return Ok(EnsureResult {
            outcome: EnsureOutcome::NotManaged,
            status: unmanaged_status(endpoint),
        });
    };
    match health::status(&health_url) {
        HealthStatus::Online {
            ref version,
            ref owner,
        } if version == EXPECTED_DAEMON_VERSION => {
            let requires_migration = owner
                .as_ref()
                .and_then(|owner| ownership::load_matching(version, owner).ok())
                .is_some_and(|record| service::requires_migration(record.launch_mode()));
            if requires_migration {
                stop_owned_daemon(&health_url, version, owner.as_ref())?;
                start_daemon(&health_url)
            } else {
                Ok(EnsureResult {
                    outcome: EnsureOutcome::AlreadyRunning,
                    status: online_status(version, owner.as_ref()),
                })
            }
        }
        HealthStatus::Online { version, owner } => {
            stop_owned_daemon(&health_url, &version, owner.as_ref())?;
            start_daemon(&health_url)
        }
        HealthStatus::Offline => start_daemon(&health_url),
    }
}

fn diagnose_blocking(endpoint: &str) -> DaemonStatus {
    let Some(health_url) = health::managed_health_url(endpoint) else {
        return unmanaged_status(endpoint);
    };
    match health::status(&health_url) {
        HealthStatus::Offline => {
            let node = runtime::node_preflight();
            let detail = match &node {
                Ok(_) => {
                    "默认本机端点没有响应；连接时会校验并安装私有运行包，再自动启动 Daemon".into()
                }
                Err(error) => format!("默认本机端点没有响应；启动前置条件未满足：{error}"),
            };
            DaemonStatus {
                phase: DaemonPhase::Offline,
                expected_version: EXPECTED_DAEMON_VERSION.into(),
                version: None,
                pid: None,
                desktop_owned: false,
                launch_mode: Some(service::preferred_mode()),
                node: node.ok(),
                runtime_path: runtime::detected_installed_runtime_path(),
                log_path: runtime::log_path().ok(),
                detail,
            }
        }
        HealthStatus::Online { version, owner } => online_status(&version, owner.as_ref()),
    }
}

fn restart_owned_blocking(endpoint: &str) -> anyhow::Result<DaemonStatus> {
    let Some(health_url) = health::managed_health_url(endpoint) else {
        bail!(
            "当前端点 {} 不是可由桌面端管理的本机端点",
            sanitized_endpoint(endpoint)
        );
    };
    if let HealthStatus::Online { version, owner } = health::status(&health_url) {
        stop_owned_daemon(&health_url, &version, owner.as_ref())?;
    }
    Ok(start_daemon(&health_url)?.status)
}

fn start_daemon(health_url: &Url) -> anyhow::Result<EnsureResult> {
    let prepared = runtime::prepare()?;
    let log_path = runtime::log_path()?;
    let instance_id = Uuid::new_v4().to_string();
    let mut started = service::start(&prepared, &instance_id)?;
    let started_at = Instant::now();

    while started_at.elapsed() < START_TIMEOUT {
        match health::status(health_url) {
            HealthStatus::Online { version, owner }
                if version == EXPECTED_DAEMON_VERSION
                    && owner.as_ref().is_some_and(|owner| owner.id == instance_id) =>
            {
                let daemon_owner = owner
                    .as_ref()
                    .expect("owner is checked before creating ownership record");
                let ownership = match OwnershipRecord::new(
                    instance_id.clone(),
                    daemon_owner.pid,
                    prepared.version.clone(),
                    prepared.node.clone(),
                    prepared.entrypoint.clone(),
                    started.mode(),
                ) {
                    Ok(ownership) => ownership,
                    Err(error) => {
                        service::abort(&mut started);
                        return Err(error);
                    }
                };
                if let Err(error) = ownership::store(&ownership) {
                    service::abort(&mut started);
                    return Err(error).context("Daemon 已启动，但无法保存桌面端所有权记录");
                }
                let status = online_status(&version, owner.as_ref());
                service::reap(started, ownership);
                return Ok(EnsureResult {
                    outcome: EnsureOutcome::Started,
                    status,
                });
            }
            HealthStatus::Online { version, owner } if version == EXPECTED_DAEMON_VERSION => {
                service::abort(&mut started);
                return Ok(EnsureResult {
                    outcome: EnsureOutcome::AlreadyRunning,
                    status: online_status(&version, owner.as_ref()),
                });
            }
            HealthStatus::Online { version, .. } => {
                service::abort(&mut started);
                bail!(
                    "Daemon 启动期间端口 6768 被不兼容版本 {version} 占用；需要 {EXPECTED_DAEMON_VERSION}"
                );
            }
            HealthStatus::Offline => {}
        }
        if let Some(status) =
            service::try_wait(&mut started).context("无法检查本机 Daemon 进程状态")?
        {
            service::abort(&mut started);
            bail!(
                "随包 Daemon 在健康检查通过前退出（{status}）；日志：{}",
                log_path.display()
            );
        }
        thread::sleep(START_POLL_INTERVAL);
    }

    service::abort(&mut started);
    bail!(
        "随包 Daemon 启动超过 {} 秒仍未就绪；日志：{}",
        START_TIMEOUT.as_secs(),
        log_path.display()
    )
}

fn online_status(version: &str, observed_owner: Option<&DaemonOwner>) -> DaemonStatus {
    let matching_record =
        observed_owner.and_then(|owner| ownership::load_matching(version, owner).ok());
    let desktop_owned = matching_record.is_some();
    let launch_mode = matching_record.as_ref().map(OwnershipRecord::launch_mode);
    let pid = observed_owner.map(|owner| owner.pid);
    let node = matching_record
        .as_ref()
        .map(OwnershipRecord::node)
        .or_else(runtime::detected_node_path);
    let runtime_path = matching_record
        .as_ref()
        .and_then(|record| runtime::installed_runtime_containing(&record.entrypoint()))
        .or_else(runtime::detected_installed_runtime_path);
    let compatible = version == EXPECTED_DAEMON_VERSION;
    let detail = if compatible && desktop_owned {
        "Daemon 版本和桌面端所有权身份均已确认".into()
    } else if compatible {
        "检测到兼容的外部 Daemon；桌面端会复用它，但不会停止或重启它".into()
    } else if desktop_owned {
        format!("桌面端拥有的 Daemon 版本 {version} 与当前版本不一致")
    } else {
        format!("外部 Daemon 版本 {version} 与需要的 {EXPECTED_DAEMON_VERSION} 不一致")
    };
    DaemonStatus {
        phase: if compatible {
            DaemonPhase::Ready
        } else {
            DaemonPhase::Blocked
        },
        expected_version: EXPECTED_DAEMON_VERSION.into(),
        version: Some(version.into()),
        pid,
        desktop_owned,
        launch_mode,
        node,
        runtime_path,
        log_path: runtime::log_path().ok(),
        detail,
    }
}

fn unmanaged_status(endpoint: &str) -> DaemonStatus {
    DaemonStatus {
        phase: DaemonPhase::Unmanaged,
        expected_version: EXPECTED_DAEMON_VERSION.into(),
        version: None,
        pid: None,
        desktop_owned: false,
        launch_mode: None,
        node: None,
        runtime_path: runtime::detected_installed_runtime_path(),
        log_path: runtime::log_path().ok(),
        detail: format!(
            "端点 {} 不是默认本机 Daemon；桌面端只负责连接，不管理其进程",
            sanitized_endpoint(endpoint)
        ),
    }
}

fn stop_owned_daemon(
    health_url: &Url,
    version: &str,
    observed_owner: Option<&DaemonOwner>,
) -> anyhow::Result<()> {
    let Some(observed_owner) = observed_owner else {
        bail!(
            "端口 6768 上的 Corbit Daemon 版本为 {version}；该进程没有桌面端所有权身份，为避免误停其他程序，请手动停止它"
        );
    };
    let record = ownership::load_matching(version, observed_owner).with_context(|| {
        format!(
            "端口 6768 上的 Daemon（PID {}，版本 {version}）不属于当前 Corbit 桌面端；为避免误停其他程序，未自动重启",
            observed_owner.pid
        )
    })?;
    if !ensure_same_owned_instance(health_url, &record)? {
        ownership::remove_if_matches(&record)?;
        return Ok(());
    }

    if let Err(error) = ownership::terminate(&record) {
        if wait_for_owned_stop(health_url, &record, Duration::ZERO)? {
            ownership::remove_if_matches(&record)?;
            return Ok(());
        }
        return Err(error);
    }
    if wait_for_owned_stop(health_url, &record, STOP_TIMEOUT)? {
        ownership::remove_if_matches(&record)?;
        return Ok(());
    }

    if !ensure_same_owned_instance(health_url, &record)? {
        ownership::remove_if_matches(&record)?;
        return Ok(());
    }
    if let Err(error) = ownership::force_kill(&record) {
        if wait_for_owned_stop(health_url, &record, Duration::ZERO)? {
            ownership::remove_if_matches(&record)?;
            return Ok(());
        }
        return Err(error);
    }
    if wait_for_owned_stop(health_url, &record, FORCE_STOP_TIMEOUT)? {
        ownership::remove_if_matches(&record)?;
        return Ok(());
    }
    bail!(
        "桌面端拥有的旧 Daemon（PID {}）在强制停止后仍占用端口 6768",
        observed_owner.pid
    )
}

fn ensure_same_owned_instance(health_url: &Url, record: &OwnershipRecord) -> anyhow::Result<bool> {
    match health::status(health_url) {
        HealthStatus::Online { owner, .. } if owner.as_ref() == Some(&record.owner()) => Ok(true),
        HealthStatus::Offline => Ok(false),
        HealthStatus::Online { owner, .. } => bail!(
            "端口 6768 上的 Daemon 身份在重启前发生变化（当前：{}）；已中止，未发送进程信号",
            owner.map_or_else(
                || "无桌面端身份".into(),
                |owner| format!("PID {}", owner.pid)
            )
        ),
    }
}

fn wait_for_owned_stop(
    health_url: &Url,
    record: &OwnershipRecord,
    timeout: Duration,
) -> anyhow::Result<bool> {
    let started_at = Instant::now();
    loop {
        match health::status(health_url) {
            HealthStatus::Offline => return Ok(true),
            HealthStatus::Online { owner, .. } if owner.as_ref() == Some(&record.owner()) => {}
            HealthStatus::Online { owner, .. } => bail!(
                "端口 6768 已由另一个 Daemon 接管（当前：{}）；已停止管理原进程",
                owner.map_or_else(
                    || "无桌面端身份".into(),
                    |owner| format!("PID {}", owner.pid)
                )
            ),
        }
        if started_at.elapsed() >= timeout {
            return Ok(false);
        }
        thread::sleep(START_POLL_INTERVAL);
    }
}

pub(super) fn open_log_directory() -> anyhow::Result<()> {
    let directory = runtime::log_directory()?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("无法创建 Daemon 日志目录 {}", directory.display()))?;
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command.arg(&directory);
    let mut child = command
        .spawn()
        .with_context(|| format!("无法打开 Daemon 日志目录 {}", directory.display()))?;
    let _ = thread::Builder::new()
        .name("corbit-log-directory-opener".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

fn sanitized_endpoint(endpoint: &str) -> String {
    let Ok(mut url) = Url::parse(endpoint) else {
        return "无效端点".into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_diagnostics_strip_endpoint_secrets() {
        let status = DaemonStatus::checking();
        let diagnostics = status.diagnostics("http://alice:secret@127.0.0.1:6768/?token=hidden");
        assert!(diagnostics.contains("http://127.0.0.1:6768"));
        assert!(!diagnostics.contains("alice"));
        assert!(!diagnostics.contains("secret"));
        assert!(!diagnostics.contains("hidden"));
    }

    #[test]
    fn only_transient_phases_are_busy() {
        assert!(DaemonPhase::Checking.is_busy());
        assert!(DaemonPhase::Restarting.is_busy());
        assert!(!DaemonPhase::Ready.is_busy());
        assert!(!DaemonPhase::Failed.is_busy());
    }
}
