//! Platform process ownership for the bundled Corbit Daemon.
//!
//! Release macOS apps keep the Daemon alive independently from the UI through
//! a per-user `LaunchAgent`. Development builds intentionally retain the direct
//! child-process path so rebuilding or stopping a debug app cannot touch the
//! installed production service. Other platforms use the same abstraction and
//! currently fall back to a directly supervised process until their native
//! service adapters are introduced.

use std::process::{Child, ExitStatus};

#[cfg(target_os = "macos")]
use std::{
    collections::HashSet,
    env,
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Write as _,
    path::PathBuf,
    process::Command,
};

use anyhow::Context as _;
#[cfg(target_os = "macos")]
use anyhow::{anyhow, bail};

use super::{
    ownership::{DaemonLaunchMode, OwnershipRecord},
    runtime,
    runtime::PreparedRuntime,
};
#[cfg(target_os = "macos")]
use crate::app::connection;

#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "com.xiaoxi.corbit.daemon";

pub(super) struct StartedDaemon {
    mode: DaemonLaunchMode,
    child: Option<Child>,
}

impl StartedDaemon {
    pub(super) const fn mode(&self) -> DaemonLaunchMode {
        self.mode
    }
}

pub(super) fn preferred_mode() -> DaemonLaunchMode {
    #[cfg(target_os = "macos")]
    if should_use_launch_agent() {
        return DaemonLaunchMode::MacosLaunchAgent;
    }
    DaemonLaunchMode::DirectProcess
}

pub(super) fn requires_migration(current: DaemonLaunchMode) -> bool {
    let preferred = preferred_mode();
    preferred == DaemonLaunchMode::MacosLaunchAgent && current != preferred
}

pub(super) fn start(runtime: &PreparedRuntime, instance_id: &str) -> anyhow::Result<StartedDaemon> {
    #[cfg(target_os = "macos")]
    if should_use_launch_agent() {
        install_launch_agent(runtime, instance_id)?;
        return Ok(StartedDaemon {
            mode: DaemonLaunchMode::MacosLaunchAgent,
            child: None,
        });
    }

    Ok(StartedDaemon {
        mode: DaemonLaunchMode::DirectProcess,
        child: Some(runtime::spawn(runtime, instance_id)?),
    })
}

pub(super) fn try_wait(started: &mut StartedDaemon) -> anyhow::Result<Option<ExitStatus>> {
    match &mut started.child {
        Some(child) => child.try_wait().context("无法检查本机 Daemon 进程状态"),
        None => Ok(None),
    }
}

pub(super) fn abort(started: &mut StartedDaemon) {
    if let Some(child) = &mut started.child {
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(target_os = "macos")]
    if started.mode == DaemonLaunchMode::MacosLaunchAgent {
        let _ = bootout_launch_agent();
    }
}

pub(super) fn reap(started: StartedDaemon, ownership: OwnershipRecord) {
    if let Some(child) = started.child {
        runtime::reap_child(child, ownership);
    }
}

#[cfg(target_os = "macos")]
pub(super) fn stop(ownership: &OwnershipRecord, force: bool) -> anyhow::Result<()> {
    if ownership.launch_mode() == DaemonLaunchMode::MacosLaunchAgent {
        bootout_launch_agent()?;
        if force {
            kill_pid(ownership.pid(), true)?;
        }
        return Ok(());
    }
    unreachable!("only macOS LaunchAgent ownership is routed through service::stop")
}

#[cfg(target_os = "macos")]
fn should_use_launch_agent() -> bool {
    !super::super::build_info::is_development()
        && env::var_os("CORBIT_AUTH_TOKEN").is_none()
        && env::var("CORBIT_DAEMON_SERVICE")
            .map_or(true, |value| !value.trim().eq_ignore_ascii_case("direct"))
}

#[cfg(target_os = "macos")]
fn install_launch_agent(runtime: &PreparedRuntime, instance_id: &str) -> anyhow::Result<()> {
    let path = launch_agent_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("macOS LaunchAgent 路径没有父目录"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建 macOS LaunchAgent 目录 {}", parent.display()))?;
    let home = connection::local_daemon_home()?;
    let log = runtime::prepare_service_log()?;
    let plist = launch_agent_plist(runtime, instance_id, &home, &log);
    write_private_file(&path, plist.as_bytes())?;

    // The label and plist are both owned by this user. Booting out only this
    // exact label avoids touching another Daemon or another user's service.
    bootout_launch_agent()?;
    let domain = launch_domain()?;
    let path_string = path.to_string_lossy();
    if let Err(error) = run_launchctl(["bootstrap", domain.as_str(), path_string.as_ref()]) {
        let _ = bootout_launch_agent();
        return Err(error)
            .with_context(|| format!("无法加载 macOS LaunchAgent {}", path.display()));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_agent_plist(
    runtime: &PreparedRuntime,
    instance_id: &str,
    home: &std::path::Path,
    log: &std::path::Path,
) -> String {
    let values = [
        ("Label", xml_string(LAUNCH_AGENT_LABEL)),
        (
            "WorkingDirectory",
            xml_string(&runtime.directory.display().to_string()),
        ),
        ("StandardOutPath", xml_string(&log.display().to_string())),
        ("StandardErrorPath", xml_string(&log.display().to_string())),
    ];
    let arguments = format!(
        "<array><string>{}</string><string>{}</string></array>",
        xml_escape(&runtime.node.display().to_string()),
        xml_escape(&runtime.entrypoint.display().to_string()),
    );
    let environment = format!(
        "<dict><key>CORBIT_HOME</key><string>{}</string><key>CORBIT_DAEMON_HOST</key><string>127.0.0.1</string><key>CORBIT_DAEMON_PORT</key><string>6768</string><key>CORBIT_DESKTOP_OWNER_ID</key><string>{}</string><key>PATH</key><string>{}</string></dict>",
        xml_escape(&home.display().to_string()),
        xml_escape(instance_id),
        xml_escape(&launch_agent_path_environment(runtime)),
    );
    let mut scalar_values = String::new();
    for (key, value) in values {
        let _ = write!(scalar_values, "<key>{key}</key><string>{value}</string>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"https://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict>{scalar_values}<key>ProgramArguments</key>{arguments}<key>EnvironmentVariables</key>{environment}<key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n"
    )
}

#[cfg(target_os = "macos")]
fn launch_agent_path_environment(runtime: &PreparedRuntime) -> String {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if path.is_absolute() && seen.insert(path.clone()) {
            paths.push(path);
        }
    };

    if let Some(parent) = runtime.node.parent() {
        push(parent.to_path_buf());
    }
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        push(home.join(".local").join("bin"));
    }
    for path in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        push(PathBuf::from(path));
    }
    if let Some(value) = env::var_os("PATH") {
        for path in env::split_paths(&value) {
            push(path);
        }
    }

    env::join_paths(paths)
        .unwrap_or_else(|_| std::ffi::OsString::from("/usr/local/bin:/usr/bin:/bin"))
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> anyhow::Result<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("无法确定当前用户的 HOME 目录"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn launch_domain() -> anyhow::Result<String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .context("无法读取当前 macOS 用户 ID")?;
    if !output.status.success() {
        bail!("读取当前 macOS 用户 ID 失败：{}", output.status);
    }
    let uid = String::from_utf8(output.stdout)
        .context("macOS 用户 ID 输出不是有效 UTF-8")?
        .trim()
        .to_owned();
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("macOS 用户 ID 无效");
    }
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn bootout_launch_agent() -> anyhow::Result<()> {
    let domain = launch_domain()?;
    let target = format!("{domain}/{LAUNCH_AGENT_LABEL}");
    let result = Command::new("/bin/launchctl")
        .args(["bootout", &target])
        .output()
        .context("无法停止 macOS LaunchAgent")?;
    let error = String::from_utf8_lossy(&result.stderr);
    if result.status.success()
        || error.contains("No such process")
        || error.contains("Could not find service")
    {
        return Ok(());
    }
    bail!("停止 macOS LaunchAgent 失败：{}", error.trim())
}

#[cfg(target_os = "macos")]
fn run_launchctl<const N: usize>(args: [&str; N]) -> anyhow::Result<()> {
    let output = Command::new("/bin/launchctl")
        .args(args)
        .output()
        .context("无法执行 launchctl")?;
    if !output.status.success() {
        bail!(
            "launchctl 执行失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = path.with_extension(format!("plist.{}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let result = (|| -> anyhow::Result<()> {
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("无法创建 LaunchAgent 临时文件 {}", temporary.display()))?;
        file.write_all(bytes).context("无法写入 LaunchAgent 配置")?;
        file.sync_all().context("无法同步 LaunchAgent 配置")?;
        drop(file);
        let validation = Command::new("/usr/bin/plutil")
            .args(["-lint", temporary.to_string_lossy().as_ref()])
            .output()
            .context("无法校验 LaunchAgent plist")?;
        if !validation.status.success() {
            bail!(
                "LaunchAgent plist 校验失败：{}",
                String::from_utf8_lossy(&validation.stderr).trim()
            );
        }
        fs::rename(&temporary, path)
            .with_context(|| format!("无法提交 LaunchAgent 配置 {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "macos")]
fn xml_string(value: &str) -> String {
    xml_escape(value)
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn kill_pid(pid: u32, force: bool) -> anyhow::Result<()> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .with_context(|| format!("无法向 Daemon PID {pid} 发送 {signal}"))?;
    if !status.success() {
        bail!("向 Daemon PID {pid} 发送 {signal} 失败（{status}）");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::{LAUNCH_AGENT_LABEL, launch_agent_plist, write_private_file};
    #[cfg(target_os = "macos")]
    use crate::app::local_daemon::runtime::PreparedRuntime;
    #[cfg(target_os = "macos")]
    use std::path::PathBuf;

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_plist_contains_only_the_expected_daemon_identity() {
        let runtime = PreparedRuntime {
            node: PathBuf::from("/opt/homebrew/bin/node"),
            directory: PathBuf::from(
                "/Users/test/.corbit/desktop/runtimes/daemon/1.0.0/macos-arm64",
            ),
            entrypoint: PathBuf::from(
                "/Users/test/.corbit/desktop/runtimes/daemon/1.0.0/macos-arm64/src/main.js",
            ),
            version: "1.0.0".into(),
        };
        let plist = launch_agent_plist(
            &runtime,
            "018f9d7a-7280-7c9b-9a35-9c4aef104b02",
            std::path::Path::new("/Users/test/.corbit"),
            std::path::Path::new("/Users/test/.corbit/logs/desktop-daemon.log"),
        );
        assert!(plist.contains(LAUNCH_AGENT_LABEL));
        assert!(plist.contains("CORBIT_DESKTOP_OWNER_ID"));
        assert!(plist.contains("KeepAlive"));
        assert!(plist.contains("<key>PATH</key>"));
        assert!(plist.contains("/opt/homebrew/bin"));
        assert!(!plist.contains("CORBIT_AUTH_TOKEN"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_plist_escapes_runtime_paths() {
        let runtime = PreparedRuntime {
            node: PathBuf::from("/Applications/Node & Tools/node"),
            directory: PathBuf::from("/Users/test/Corbit <runtime>"),
            entrypoint: PathBuf::from("/Users/test/Corbit <runtime>/main.js"),
            version: "1.0.0".into(),
        };
        let plist = launch_agent_plist(
            &runtime,
            "018f9d7a-7280-7c9b-9a35-9c4aef104b02",
            std::path::Path::new("/Users/test/.corbit"),
            std::path::Path::new("/Users/test/.corbit/logs/daemon.log"),
        );
        assert!(plist.contains("Node &amp; Tools"));
        assert!(plist.contains("Corbit &lt;runtime&gt;"));
        assert!(!plist.contains("Node & Tools"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn writes_an_atomic_private_and_valid_launch_agent_plist() {
        use std::{fs, os::unix::fs::PermissionsExt as _};

        let directory = std::env::temp_dir().join(format!(
            "corbit-launch-agent-plist-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("create launch agent test directory");
        let path = directory.join("com.xiaoxi.corbit.daemon.plist");
        let runtime = PreparedRuntime {
            node: PathBuf::from("/opt/homebrew/bin/node"),
            directory: PathBuf::from("/Users/test/.corbit/runtime"),
            entrypoint: PathBuf::from("/Users/test/.corbit/runtime/src/main.js"),
            version: "1.0.0".into(),
        };
        let plist = launch_agent_plist(
            &runtime,
            "018f9d7a-7280-7c9b-9a35-9c4aef104b02",
            std::path::Path::new("/Users/test/.corbit"),
            std::path::Path::new("/Users/test/.corbit/logs/daemon.log"),
        );

        write_private_file(&path, plist.as_bytes()).expect("write valid launch agent plist");
        assert_eq!(
            fs::metadata(&path)
                .expect("read launch agent plist metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read launch agent plist"),
            plist,
        );
        assert!(
            fs::read_dir(&directory)
                .expect("read launch agent test directory")
                .all(|entry| !entry
                    .expect("read launch agent test entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
        );
        fs::remove_dir_all(directory).expect("remove launch agent test directory");
    }
}
