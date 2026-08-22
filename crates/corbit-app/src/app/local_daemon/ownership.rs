use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, anyhow, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::super::connection;

const OWNERSHIP_SCHEMA_VERSION: u32 = 2;
const OWNERSHIP_FILE_NAME: &str = "desktop-daemon.json";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DaemonLaunchMode {
    #[default]
    DirectProcess,
    MacosLaunchAgent,
}

impl DaemonLaunchMode {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DirectProcess => "桌面端直接进程",
            Self::MacosLaunchAgent => "macOS LaunchAgent",
        }
    }

    const fn permits_pid_refresh(self) -> bool {
        matches!(self, Self::MacosLaunchAgent)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct DaemonOwner {
    pub(super) id: String,
    pub(super) pid: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OwnershipRecord {
    schema_version: u32,
    instance_id: String,
    pid: u32,
    version: String,
    node: PathBuf,
    entrypoint: PathBuf,
    #[serde(default)]
    launch_mode: DaemonLaunchMode,
}

impl OwnershipRecord {
    pub(super) fn new(
        instance_id: String,
        pid: u32,
        version: String,
        node: PathBuf,
        entrypoint: PathBuf,
        launch_mode: DaemonLaunchMode,
    ) -> anyhow::Result<Self> {
        let record = Self {
            schema_version: OWNERSHIP_SCHEMA_VERSION,
            instance_id,
            pid,
            version,
            node,
            entrypoint,
            launch_mode,
        };
        record.validate()?;
        Ok(record)
    }

    pub(super) fn owner(&self) -> DaemonOwner {
        DaemonOwner {
            id: self.instance_id.clone(),
            pid: self.pid,
        }
    }

    pub(super) fn node(&self) -> PathBuf {
        self.node.clone()
    }

    pub(super) fn entrypoint(&self) -> PathBuf {
        self.entrypoint.clone()
    }

    #[cfg(target_os = "macos")]
    pub(super) const fn pid(&self) -> u32 {
        self.pid
    }

    pub(super) const fn launch_mode(&self) -> DaemonLaunchMode {
        self.launch_mode
    }

    pub(super) fn matches(&self, version: &str, owner: &DaemonOwner) -> bool {
        self.validate().is_ok()
            && self.version == version
            && self.instance_id == owner.id
            && (self.pid == owner.pid || self.launch_mode.permits_pid_refresh())
    }

    fn validate(&self) -> anyhow::Result<()> {
        if !matches!(self.schema_version, 1 | OWNERSHIP_SCHEMA_VERSION) {
            bail!("不支持的桌面 Daemon 所有权记录版本");
        }
        if self.pid == 0 {
            bail!("桌面 Daemon 所有权记录中的 PID 无效");
        }
        Uuid::parse_str(&self.instance_id).context("桌面 Daemon 所有权记录中的实例 ID 无效")?;
        if self.version.trim().is_empty() {
            bail!("桌面 Daemon 所有权记录中的版本为空");
        }
        if !self.node.is_absolute() || !self.entrypoint.is_absolute() {
            bail!("桌面 Daemon 所有权记录中的可执行路径必须是绝对路径");
        }
        Ok(())
    }
}

pub(super) fn store(record: &OwnershipRecord) -> anyhow::Result<()> {
    write_to(&ownership_path()?, record)
}

pub(super) fn load_matching(version: &str, owner: &DaemonOwner) -> anyhow::Result<OwnershipRecord> {
    let path = ownership_path()?;
    let mut record = read_from(&path)?;
    if !record.matches(version, owner) {
        bail!("{} 与正在运行的 Daemon 身份不一致", path.display());
    }
    if record.pid != owner.pid {
        record.schema_version = OWNERSHIP_SCHEMA_VERSION;
        record.pid = owner.pid;
        write_to(&path, &record).with_context(|| {
            format!(
                "无法更新平台服务重启后的 Daemon 所有权记录 {}",
                path.display()
            )
        })?;
    }
    Ok(record)
}

pub(super) fn remove_if_matches(record: &OwnershipRecord) -> anyhow::Result<()> {
    let path = ownership_path()?;
    let current = match read_from(&path) {
        Ok(current) => current,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if current == *record {
        fs::remove_file(&path)
            .with_context(|| format!("无法清理桌面 Daemon 所有权记录 {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn terminate(record: &OwnershipRecord) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    if record.launch_mode == DaemonLaunchMode::MacosLaunchAgent {
        return super::service::stop(record, false);
    }
    signal(record.pid, false)
}

pub(super) fn force_kill(record: &OwnershipRecord) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    if record.launch_mode == DaemonLaunchMode::MacosLaunchAgent {
        return super::service::stop(record, true);
    }
    signal(record.pid, true)
}

fn ownership_path() -> anyhow::Result<PathBuf> {
    Ok(connection::local_daemon_home()?
        .join("runtime")
        .join(OWNERSHIP_FILE_NAME))
}

fn read_from(path: &Path) -> anyhow::Result<OwnershipRecord> {
    let bytes = fs::read(path)
        .with_context(|| format!("无法读取桌面 Daemon 所有权记录 {}", path.display()))?;
    let record: OwnershipRecord = serde_json::from_slice(&bytes)
        .with_context(|| format!("无法解析桌面 Daemon 所有权记录 {}", path.display()))?;
    record.validate()?;
    Ok(record)
}

fn write_to(path: &Path, record: &OwnershipRecord) -> anyhow::Result<()> {
    record.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("桌面 Daemon 所有权记录路径没有父目录"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建 Daemon 运行目录 {}", parent.display()))?;
    let temporary_path = parent.join(format!(".{OWNERSHIP_FILE_NAME}.{}.tmp", Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path).with_context(|| {
            format!(
                "无法创建临时 Daemon 所有权记录 {}",
                temporary_path.display()
            )
        })?;
        let bytes = serde_json::to_vec_pretty(record).context("无法序列化 Daemon 所有权记录")?;
        file.write_all(&bytes)
            .context("无法写入临时 Daemon 所有权记录")?;
        file.sync_all().context("无法同步临时 Daemon 所有权记录")?;
        drop(file);
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("无法替换旧 Daemon 所有权记录 {}", path.display()))?;
        }
        fs::rename(&temporary_path, path)
            .with_context(|| format!("无法提交 Daemon 所有权记录 {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(unix)]
fn signal(pid: u32, force: bool) -> anyhow::Result<()> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .with_context(|| format!("无法向桌面 Daemon PID {pid} 发送 {signal}"))?;
    if !status.success() {
        bail!("向桌面 Daemon PID {pid} 发送 {signal} 失败（{status}）");
    }
    Ok(())
}

#[cfg(windows)]
fn signal(pid: u32, force: bool) -> anyhow::Result<()> {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let taskkill = windows.join("System32").join("taskkill.exe");
    let mut command = Command::new(&taskkill);
    command.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let status = command
        .status()
        .with_context(|| format!("无法通过 {} 停止桌面 Daemon PID {pid}", taskkill.display()))?;
    if !status.success() {
        bail!("停止桌面 Daemon PID {pid} 失败（{status}）");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> OwnershipRecord {
        OwnershipRecord::new(
            "018f9d7a-7280-7c9b-9a35-9c4aef104b02".into(),
            4242,
            "0.1.0".into(),
            PathBuf::from("/opt/homebrew/bin/node"),
            PathBuf::from("/Applications/Corbit.app/Contents/Resources/corbit-daemon/src/main.js"),
            DaemonLaunchMode::DirectProcess,
        )
        .expect("valid ownership record")
    }

    #[test]
    fn matches_only_the_exact_health_identity_and_version() {
        let record = record();
        assert!(record.matches("0.1.0", &record.owner()));
        assert!(!record.matches("0.2.0", &record.owner()));
        assert!(!record.matches(
            "0.1.0",
            &DaemonOwner {
                id: record.instance_id.clone(),
                pid: 4243,
            }
        ));
    }

    #[test]
    fn supervised_service_accepts_a_restarted_pid_but_direct_process_does_not() {
        let direct = record();
        let restarted_owner = DaemonOwner {
            id: direct.instance_id.clone(),
            pid: 5252,
        };
        assert!(!direct.matches("0.1.0", &restarted_owner));

        let supervised = OwnershipRecord::new(
            direct.instance_id.clone(),
            direct.pid,
            direct.version.clone(),
            direct.node.clone(),
            direct.entrypoint.clone(),
            DaemonLaunchMode::MacosLaunchAgent,
        )
        .expect("valid supervised ownership record");
        assert!(supervised.matches("0.1.0", &restarted_owner));
    }

    #[test]
    fn writes_and_reads_a_restrictive_atomic_record() {
        let directory = std::env::temp_dir().join(format!("corbit-owner-{}", Uuid::new_v4()));
        let path = directory.join(OWNERSHIP_FILE_NAME);
        let expected = record();
        write_to(&path, &expected).expect("write ownership record");
        assert_eq!(read_from(&path).expect("read ownership record"), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path)
                    .expect("record metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).expect("remove ownership test directory");
    }
}
