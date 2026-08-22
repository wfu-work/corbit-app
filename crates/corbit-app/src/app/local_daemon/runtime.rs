use std::{
    collections::HashSet,
    env, fs,
    fs::{File, OpenOptions},
    io::Write as _,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
};

use anyhow::{Context as _, anyhow, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{EXPECTED_DAEMON_VERSION, ownership, ownership::OwnershipRecord};
use crate::app::connection;

const DAEMON_RUNTIME_ENVIRONMENT: &str = "CORBIT_DAEMON_RUNTIME";
const NODE_PATH_ENVIRONMENT: &str = "CORBIT_NODE_PATH";
const LOG_FILE_NAME: &str = "desktop-daemon.log";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const RETAINED_LOG_FILES: usize = 3;
const BUNDLED_RUNTIME_INFO: &str = ".corbit-bundle.json";
const BUNDLED_RUNTIME_SCHEMA_VERSION: u32 = 1;
const RUNTIME_INSTALL_SCHEMA_VERSION: u32 = 2;
const RUNTIME_INSTALL_MARKER: &str = ".corbit-runtime.json";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonBuildInfo {
    product: String,
    version: String,
    platform: String,
    arch: String,
    node: String,
    entrypoint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BundledRuntimeInfo {
    schema_version: u32,
    product: String,
    version: String,
    platform: String,
    arch: String,
    digest: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInstallInfo {
    schema_version: u32,
    product: String,
    version: String,
    platform: String,
    arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle_digest: Option<String>,
}

#[derive(Debug)]
pub(super) struct PreparedRuntime {
    pub(super) node: PathBuf,
    pub(super) directory: PathBuf,
    pub(super) entrypoint: PathBuf,
    pub(super) version: String,
}

#[derive(Debug)]
struct NodeInstallation {
    path: PathBuf,
    arch: String,
}

#[derive(Deserialize)]
struct NodeProbe {
    version: String,
    arch: String,
}

pub(super) fn prepare() -> anyhow::Result<PreparedRuntime> {
    let node = find_node_24()?;
    let bundled_directory = bundled_runtime_directory(&node.arch)?;
    let build_info = validate_runtime(&bundled_directory, &node.arch)?;
    let bundle_info = validate_bundled_runtime_info(&bundled_directory, &build_info)?;
    let directory = install_runtime(&bundled_directory, &build_info, &bundle_info)?;
    let build_info = validate_installed_runtime(&directory, &build_info, &bundle_info)?;
    let entrypoint = checked_entrypoint(&directory, &build_info.entrypoint)?;
    Ok(PreparedRuntime {
        node: node.path,
        directory,
        entrypoint,
        version: build_info.version,
    })
}

pub(super) fn detected_node_path() -> Option<PathBuf> {
    find_node_24().ok().map(|node| node.path)
}

pub(super) fn node_preflight() -> anyhow::Result<PathBuf> {
    find_node_24().map(|node| node.path)
}

pub(super) fn detected_installed_runtime_path() -> Option<PathBuf> {
    installed_runtime_path(EXPECTED_DAEMON_VERSION, current_platform(), current_arch())
        .ok()
        .filter(|path| path.is_dir())
}

pub(super) fn installed_runtime_containing(entrypoint: &Path) -> Option<PathBuf> {
    entrypoint
        .ancestors()
        .find(|directory| {
            read_install_marker(directory).is_ok_and(|info| {
                matches!(info.schema_version, 1 | RUNTIME_INSTALL_SCHEMA_VERSION)
                    && info.product == "Corbit Daemon"
            })
        })
        .map(Path::to_path_buf)
}

pub(super) fn log_directory() -> anyhow::Result<PathBuf> {
    Ok(connection::local_daemon_home()?.join("logs"))
}

pub(super) fn log_path() -> anyhow::Result<PathBuf> {
    Ok(log_directory()?.join(LOG_FILE_NAME))
}

pub(super) fn prepare_service_log() -> anyhow::Result<PathBuf> {
    let path = log_path()?;
    drop(open_log(&path)?);
    Ok(path)
}

pub(super) fn spawn(runtime: &PreparedRuntime, instance_id: &str) -> anyhow::Result<Child> {
    let log_path = log_path()?;
    let log = open_log(&log_path)?;
    let stderr = log.try_clone().context("无法复制 Daemon 日志文件句柄")?;
    Command::new(&runtime.node)
        .arg(&runtime.entrypoint)
        .current_dir(&runtime.directory)
        .env("CORBIT_HOME", connection::local_daemon_home()?)
        .env("CORBIT_DAEMON_HOST", "127.0.0.1")
        .env("CORBIT_DAEMON_PORT", "6768")
        .env("CORBIT_DESKTOP_OWNER_ID", instance_id)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("无法通过 {} 启动随包 Daemon", runtime.node.display()))
}

pub(super) fn reap_child(mut child: Child, ownership: OwnershipRecord) {
    let _ = thread::Builder::new()
        .name("corbit-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
            let _ = ownership::remove_if_matches(&ownership);
        });
}

fn find_node_24() -> anyhow::Result<NodeInstallation> {
    let candidates = node_candidates();
    let mut detected = Vec::new();
    for candidate in candidates {
        let Ok(path) = candidate.canonicalize() else {
            continue;
        };
        let Ok(output) = Command::new(&path)
            .args([
                "-p",
                "JSON.stringify({version:process.versions.node,arch:process.arch})",
            ])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Ok(probe) = serde_json::from_slice::<NodeProbe>(&output.stdout) else {
            continue;
        };
        detected.push(format!("{} ({})", path.display(), probe.version));
        if node_major(&probe.version) == Some(24) && matches!(probe.arch.as_str(), "arm64" | "x64")
        {
            return Ok(NodeInstallation {
                path,
                arch: probe.arch,
            });
        }
    }

    let detail = if detected.is_empty() {
        "未找到可执行的 Node.js".into()
    } else {
        format!("已找到但版本不兼容：{}", detected.join("，"))
    };
    bail!(
        "Corbit Daemon 需要系统安装 Node.js 24；{detail}。请从 https://nodejs.org/ 安装 Node.js 24 LTS，或通过 {NODE_PATH_ENVIRONMENT} 指定 Node 24 的绝对路径"
    )
}

fn node_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(configured) = env::var_os(NODE_PATH_ENVIRONMENT).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(configured);
        if path.is_absolute() {
            candidates.push(path);
        }
    }

    #[cfg(target_os = "macos")]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ]);
    #[cfg(target_os = "linux")]
    candidates.extend([
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ]);
    #[cfg(target_os = "windows")]
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("nodejs").join("node.exe"));
    }

    if let Some(search_path) = env::var_os("PATH") {
        let executable = if cfg!(target_os = "windows") {
            "node.exe"
        } else {
            "node"
        };
        candidates
            .extend(env::split_paths(&search_path).map(|directory| directory.join(executable)));
    }
    candidates.dedup();
    candidates
}

fn node_major(version: &str) -> Option<u64> {
    version
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn bundled_runtime_directory(node_arch: &str) -> anyhow::Result<PathBuf> {
    if let Some(configured) =
        env::var_os(DAEMON_RUNTIME_ENVIRONMENT).filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(configured);
        if !path.is_absolute() {
            bail!("{DAEMON_RUNTIME_ENVIRONMENT} 必须是绝对路径");
        }
        return Ok(path);
    }
    runtime_directory_from_executable(
        &env::current_exe().context("无法确定桌面程序路径")?,
        node_arch,
    )
}

fn runtime_directory_from_executable(
    executable: &Path,
    node_arch: &str,
) -> anyhow::Result<PathBuf> {
    let executable_directory = executable
        .parent()
        .ok_or_else(|| anyhow!("桌面程序路径没有父目录：{}", executable.display()))?;
    #[cfg(target_os = "macos")]
    {
        let contents_directory = executable_directory
            .parent()
            .ok_or_else(|| anyhow!("macOS 桌面程序不在标准 App Bundle 中"))?;
        let base = contents_directory.join("Resources").join("corbit-daemon");
        let universal_runtime = base.join(node_arch);
        Ok(if universal_runtime.is_dir() {
            universal_runtime
        } else {
            base
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = node_arch;
        Ok(executable_directory.join("corbit-daemon"))
    }
}

fn validate_runtime(runtime_directory: &Path, node_arch: &str) -> anyhow::Result<DaemonBuildInfo> {
    let metadata_path = runtime_directory.join("build-info.json");
    let bytes = fs::read(&metadata_path)
        .with_context(|| format!("无法读取随包 Daemon 元数据 {}", metadata_path.display()))?;
    let build_info: DaemonBuildInfo = serde_json::from_slice(&bytes)
        .with_context(|| format!("无法解析随包 Daemon 元数据 {}", metadata_path.display()))?;
    let expected_platform = current_platform();
    if build_info.product != "Corbit Daemon"
        || build_info.version != EXPECTED_DAEMON_VERSION
        || build_info.platform != expected_platform
        || build_info.arch != node_arch
        || build_info.node != "24.x"
    {
        bail!(
            "随包 Daemon 与桌面端不匹配：需要 {EXPECTED_DAEMON_VERSION}/{expected_platform}/{node_arch}/Node 24，实际为 {}/{}/{}/{}",
            build_info.version,
            build_info.platform,
            build_info.arch,
            build_info.node
        );
    }
    Ok(build_info)
}

fn validate_bundled_runtime_info(
    runtime_directory: &Path,
    expected: &DaemonBuildInfo,
) -> anyhow::Result<BundledRuntimeInfo> {
    let path = runtime_directory.join(BUNDLED_RUNTIME_INFO);
    let bytes = fs::read(&path)
        .with_context(|| format!("无法读取随包 Daemon 内容指纹 {}", path.display()))?;
    let info: BundledRuntimeInfo = serde_json::from_slice(&bytes)
        .with_context(|| format!("无法解析随包 Daemon 内容指纹 {}", path.display()))?;
    if info.schema_version != BUNDLED_RUNTIME_SCHEMA_VERSION
        || info.product != "Corbit Daemon Bundle"
        || info.version != expected.version
        || info.platform != expected.platform
        || info.arch != expected.arch
        || !is_valid_bundle_digest(&info.digest)
    {
        bail!("随包 Daemon 内容指纹与构建元数据不匹配");
    }
    Ok(info)
}

fn is_valid_bundle_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn checked_entrypoint(runtime_directory: &Path, entrypoint: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(entrypoint);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("随包 Daemon 入口路径无效：{entrypoint}");
    }
    let path = runtime_directory.join(relative);
    if !path.is_file() {
        bail!("随包 Daemon 入口不存在：{}", path.display());
    }
    Ok(path)
}

fn managed_runtime_root() -> anyhow::Result<PathBuf> {
    Ok(connection::local_daemon_home()?
        .join("desktop")
        .join("runtimes")
        .join("daemon"))
}

fn installed_runtime_path(version: &str, platform: &str, arch: &str) -> anyhow::Result<PathBuf> {
    installed_runtime_path_in(&managed_runtime_root()?, version, platform, arch)
}

fn installed_runtime_path_in(
    root: &Path,
    version: &str,
    platform: &str,
    arch: &str,
) -> anyhow::Result<PathBuf> {
    validate_path_component("版本", version)?;
    validate_path_component("平台", platform)?;
    validate_path_component("架构", arch)?;
    Ok(root.join(version).join(format!("{platform}-{arch}")))
}

fn validate_path_component(label: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        bail!("Daemon {label}不能安全用于运行时目录：{value}");
    }
    Ok(())
}

fn install_runtime(
    bundled_directory: &Path,
    build_info: &DaemonBuildInfo,
    bundle_info: &BundledRuntimeInfo,
) -> anyhow::Result<PathBuf> {
    install_runtime_into(
        bundled_directory,
        build_info,
        bundle_info,
        &managed_runtime_root()?,
    )
}

fn install_runtime_into(
    bundled_directory: &Path,
    build_info: &DaemonBuildInfo,
    bundle_info: &BundledRuntimeInfo,
    root: &Path,
) -> anyhow::Result<PathBuf> {
    let destination = installed_runtime_path_in(
        root,
        &build_info.version,
        &build_info.platform,
        &build_info.arch,
    )?;
    let install_info = RuntimeInstallInfo::from_build_info(build_info, bundle_info);
    if validate_installed_runtime(&destination, build_info, bundle_info).is_ok() {
        return Ok(destination);
    }
    if destination.exists() && !has_managed_install_marker(&destination, build_info) {
        bail!(
            "运行时目录 {} 不是由 Corbit 管理，未覆盖其中内容",
            destination.display()
        );
    }

    let destination_parent = destination
        .parent()
        .ok_or_else(|| anyhow!("Daemon 运行时安装路径没有父目录"))?;
    fs::create_dir_all(destination_parent).with_context(|| {
        format!(
            "无法创建 Daemon 运行时目录 {}",
            destination_parent.display()
        )
    })?;
    set_private_directory_permissions(root)?;
    set_private_directory_permissions(destination_parent)?;

    let install_id = Uuid::new_v4();
    let staging = root.join(format!(".install-{install_id}"));
    let backup = root.join(format!(".replace-{install_id}"));
    let result = (|| -> anyhow::Result<()> {
        copy_runtime_tree(bundled_directory, &staging)?;
        write_install_marker(&staging, &install_info)?;
        validate_installed_runtime(&staging, build_info, bundle_info)
            .context("复制后的 Daemon 运行时校验失败")?;

        if destination.exists() {
            fs::rename(&destination, &backup).with_context(|| {
                format!(
                    "无法暂存旧 Daemon 运行时 {} 到 {}",
                    destination.display(),
                    backup.display()
                )
            })?;
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            if backup.exists() {
                let _ = fs::rename(&backup, &destination);
            }
            return Err(error)
                .with_context(|| format!("无法提交 Daemon 运行时 {}", destination.display()));
        }
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(destination)
}

impl RuntimeInstallInfo {
    fn from_build_info(build_info: &DaemonBuildInfo, bundle_info: &BundledRuntimeInfo) -> Self {
        Self {
            schema_version: RUNTIME_INSTALL_SCHEMA_VERSION,
            product: build_info.product.clone(),
            version: build_info.version.clone(),
            platform: build_info.platform.clone(),
            arch: build_info.arch.clone(),
            bundle_digest: Some(bundle_info.digest.clone()),
        }
    }
}

fn validate_installed_runtime(
    directory: &Path,
    expected: &DaemonBuildInfo,
    expected_bundle: &BundledRuntimeInfo,
) -> anyhow::Result<DaemonBuildInfo> {
    let install_info = read_install_marker(directory)?;
    if install_info != RuntimeInstallInfo::from_build_info(expected, expected_bundle) {
        bail!("Daemon 私有运行时安装标记与随包版本不匹配");
    }
    let actual = validate_runtime(directory, &expected.arch)?;
    let actual_bundle = validate_bundled_runtime_info(directory, &actual)?;
    if actual.product != expected.product
        || actual.version != expected.version
        || actual.platform != expected.platform
        || actual.arch != expected.arch
        || actual.node != expected.node
        || actual.entrypoint != expected.entrypoint
    {
        bail!("Daemon 私有运行时内容与随包版本不匹配");
    }
    if actual_bundle != *expected_bundle {
        bail!("Daemon 私有运行时内容指纹与随包内容不匹配");
    }
    checked_entrypoint(directory, &actual.entrypoint)?;
    Ok(actual)
}

fn has_managed_install_marker(directory: &Path, expected: &DaemonBuildInfo) -> bool {
    read_install_marker(directory).is_ok_and(|actual| {
        matches!(actual.schema_version, 1 | RUNTIME_INSTALL_SCHEMA_VERSION)
            && actual.product == expected.product
            && actual.version == expected.version
            && actual.platform == expected.platform
            && actual.arch == expected.arch
    })
}

fn read_install_marker(directory: &Path) -> anyhow::Result<RuntimeInstallInfo> {
    let path = directory.join(RUNTIME_INSTALL_MARKER);
    let bytes = fs::read(&path)
        .with_context(|| format!("无法读取 Daemon 运行时安装标记 {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("无法解析 Daemon 运行时安装标记 {}", path.display()))
}

fn write_install_marker(directory: &Path, info: &RuntimeInstallInfo) -> anyhow::Result<()> {
    let path = directory.join(RUNTIME_INSTALL_MARKER);
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("无法创建 Daemon 运行时安装标记 {}", path.display()))?;
    let bytes = serde_json::to_vec_pretty(info).context("无法序列化 Daemon 运行时安装标记")?;
    file.write_all(&bytes)
        .context("无法写入 Daemon 运行时安装标记")?;
    file.sync_all().context("无法同步 Daemon 运行时安装标记")
}

fn copy_runtime_tree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let source = source
        .canonicalize()
        .with_context(|| format!("无法解析随包 Daemon 运行时 {}", source.display()))?;
    if !source.is_dir() {
        bail!("随包 Daemon 运行时不是目录：{}", source.display());
    }
    let mut active_directories = HashSet::new();
    copy_runtime_directory(&source, destination, &source, &mut active_directories)
}

fn copy_runtime_directory(
    source: &Path,
    destination: &Path,
    root: &Path,
    active_directories: &mut HashSet<PathBuf>,
) -> anyhow::Result<()> {
    let resolved = source
        .canonicalize()
        .with_context(|| format!("无法解析 Daemon 运行时目录 {}", source.display()))?;
    if !resolved.starts_with(root) {
        bail!("Daemon 运行时目录链接越过随包根目录：{}", source.display());
    }
    if !active_directories.insert(resolved.clone()) {
        bail!("Daemon 运行时包含循环目录链接：{}", source.display());
    }
    let result = (|| -> anyhow::Result<()> {
        fs::create_dir(destination)
            .with_context(|| format!("无法创建 Daemon 运行时目录 {}", destination.display()))?;
        set_private_directory_permissions(destination)?;
        for entry in fs::read_dir(&resolved)
            .with_context(|| format!("无法读取 Daemon 运行时目录 {}", resolved.display()))?
        {
            let entry = entry.context("无法读取 Daemon 运行时目录项")?;
            if entry.file_name() == RUNTIME_INSTALL_MARKER {
                continue;
            }
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            copy_runtime_entry(&source_path, &destination_path, root, active_directories)?;
        }
        Ok(())
    })();
    active_directories.remove(&resolved);
    result
}

fn copy_runtime_entry(
    source: &Path,
    destination: &Path,
    root: &Path,
    active_directories: &mut HashSet<PathBuf>,
) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("无法读取 Daemon 运行时项目 {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        let resolved = source
            .canonicalize()
            .with_context(|| format!("无法解析 Daemon 运行时链接 {}", source.display()))?;
        if !resolved.starts_with(root) {
            bail!("Daemon 运行时链接越过随包根目录：{}", source.display());
        }
        if resolved.is_dir() {
            return copy_runtime_directory(&resolved, destination, root, active_directories);
        }
        if resolved.is_file() {
            return copy_runtime_file(&resolved, destination);
        }
        bail!("Daemon 运行时链接目标类型不受支持：{}", source.display());
    }
    if metadata.is_dir() {
        return copy_runtime_directory(source, destination, root, active_directories);
    }
    if metadata.is_file() {
        return copy_runtime_file(source, destination);
    }
    bail!("Daemon 运行时项目类型不受支持：{}", source.display())
}

fn copy_runtime_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::copy(source, destination).with_context(|| {
        format!(
            "无法复制 Daemon 运行时文件 {} 到 {}",
            source.display(),
            destination.display()
        )
    })?;
    let permissions = fs::metadata(source)
        .with_context(|| format!("无法读取 Daemon 运行时文件权限 {}", source.display()))?
        .permissions();
    fs::set_permissions(destination, permissions)
        .with_context(|| format!("无法设置 Daemon 运行时文件权限 {}", destination.display()))
}

fn set_private_directory_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("无法限制 Daemon 运行时目录权限 {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn open_log(path: &Path) -> anyhow::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Daemon 日志路径没有父目录"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建 Daemon 日志目录 {}", parent.display()))?;
    rotate_logs(path)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("无法打开 Daemon 日志 {}", path.display()))
}

fn rotate_logs(path: &Path) -> anyhow::Result<()> {
    let should_rotate = fs::metadata(path).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES);
    if !should_rotate {
        return Ok(());
    }
    rotate_log_generations(path)
}

fn rotate_log_generations(path: &Path) -> anyhow::Result<()> {
    for index in (1..=RETAINED_LOG_FILES).rev() {
        let source = rotated_log_path(path, index);
        if index == RETAINED_LOG_FILES {
            if source.exists() {
                fs::remove_file(&source)
                    .with_context(|| format!("无法删除旧 Daemon 日志 {}", source.display()))?;
            }
        } else if source.exists() {
            let destination = rotated_log_path(path, index + 1);
            fs::rename(&source, &destination).with_context(|| {
                format!(
                    "无法轮转 Daemon 日志 {} 到 {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }
    fs::rename(path, rotated_log_path(path, 1))
        .with_context(|| format!("无法轮转 Daemon 日志 {}", path.display()))
}

fn rotated_log_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

const fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

const fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_build_info() -> DaemonBuildInfo {
        DaemonBuildInfo {
            product: "Corbit Daemon".into(),
            version: EXPECTED_DAEMON_VERSION.into(),
            platform: current_platform().into(),
            arch: current_arch().into(),
            node: "24.x".into(),
            entrypoint: "src/main.js".into(),
        }
    }

    fn fixture_bundle_info(digest_character: char) -> BundledRuntimeInfo {
        BundledRuntimeInfo {
            schema_version: BUNDLED_RUNTIME_SCHEMA_VERSION,
            product: "Corbit Daemon Bundle".into(),
            version: EXPECTED_DAEMON_VERSION.into(),
            platform: current_platform().into(),
            arch: current_arch().into(),
            digest: format!("sha256:{}", digest_character.to_string().repeat(64)),
        }
    }

    fn write_runtime_fixture(
        directory: &Path,
        build_info: &DaemonBuildInfo,
        bundle_info: &BundledRuntimeInfo,
    ) {
        fs::create_dir_all(directory.join("src")).expect("create runtime fixture");
        fs::write(
            directory.join("build-info.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "product": build_info.product,
                "version": build_info.version,
                "platform": build_info.platform,
                "arch": build_info.arch,
                "node": build_info.node,
                "entrypoint": build_info.entrypoint,
            }))
            .expect("serialize build info"),
        )
        .expect("write build info");
        fs::write(
            directory.join(BUNDLED_RUNTIME_INFO),
            serde_json::to_vec_pretty(bundle_info).expect("serialize bundle info"),
        )
        .expect("write bundle info");
        fs::write(directory.join("src/main.js"), "console.log('fixture');")
            .expect("write fixture entrypoint");
    }

    #[test]
    fn parses_only_numeric_node_major_versions() {
        assert_eq!(node_major("24.7.0"), Some(24));
        assert_eq!(node_major("v24.7.0"), Some(24));
        assert_eq!(node_major("node-24"), None);
    }

    #[test]
    fn rejects_unsafe_runtime_entrypoints() {
        let root = Path::new("/tmp/corbit-runtime-test");
        assert!(checked_entrypoint(root, "../src/main.js").is_err());
        assert!(checked_entrypoint(root, "/src/main.js").is_err());
    }

    #[test]
    fn accepts_only_safe_runtime_path_components() {
        let root = Path::new("/tmp/corbit-runtime-root");
        assert!(installed_runtime_path_in(root, "0.1.0", "macos", "arm64").is_ok());
        assert!(installed_runtime_path_in(root, "../0.1.0", "macos", "arm64").is_err());
        assert!(installed_runtime_path_in(root, "0.1.0", "mac/os", "arm64").is_err());
        assert!(installed_runtime_path_in(root, "0.1.0", "macos", "").is_err());
    }

    #[test]
    fn atomically_installs_reuses_and_repairs_a_managed_runtime() {
        let directory =
            std::env::temp_dir().join(format!("corbit-runtime-install-{}", Uuid::new_v4()));
        let bundled = directory.join("bundled");
        let installed_root = directory.join("installed");
        let build_info = fixture_build_info();
        let mut bundle_info = fixture_bundle_info('a');
        write_runtime_fixture(&bundled, &build_info, &bundle_info);

        let installed = install_runtime_into(&bundled, &build_info, &bundle_info, &installed_root)
            .expect("install runtime");
        assert_eq!(
            fs::read_to_string(installed.join("src/main.js")).expect("read installed entrypoint"),
            "console.log('fixture');"
        );
        assert_eq!(
            read_install_marker(&installed).expect("read install marker"),
            RuntimeInstallInfo::from_build_info(&build_info, &bundle_info)
        );
        assert_eq!(
            install_runtime_into(&bundled, &build_info, &bundle_info, &installed_root,)
                .expect("reuse runtime"),
            installed
        );

        fs::write(
            bundled.join("src/main.js"),
            "console.log('updated fixture');",
        )
        .expect("update bundled entrypoint");
        bundle_info = fixture_bundle_info('b');
        fs::write(
            bundled.join(BUNDLED_RUNTIME_INFO),
            serde_json::to_vec_pretty(&bundle_info).expect("serialize updated bundle info"),
        )
        .expect("write updated bundle info");
        install_runtime_into(&bundled, &build_info, &bundle_info, &installed_root)
            .expect("replace stale runtime with the same version");
        assert_eq!(
            fs::read_to_string(installed.join("src/main.js")).expect("read updated entrypoint"),
            "console.log('updated fixture');"
        );

        fs::remove_file(installed.join("src/main.js")).expect("corrupt installed runtime");
        install_runtime_into(&bundled, &build_info, &bundle_info, &installed_root)
            .expect("repair managed runtime");
        assert!(installed.join("src/main.js").is_file());
        assert!(
            fs::read_dir(&installed_root)
                .expect("read installed root")
                .all(|entry| {
                    let name = entry.expect("read installed entry").file_name();
                    !name.to_string_lossy().starts_with(".install-")
                        && !name.to_string_lossy().starts_with(".replace-")
                })
        );

        fs::remove_dir_all(directory).expect("remove runtime install test directory");
    }

    #[test]
    fn never_overwrites_an_unmanaged_runtime_directory() {
        let directory =
            std::env::temp_dir().join(format!("corbit-runtime-unmanaged-{}", Uuid::new_v4()));
        let bundled = directory.join("bundled");
        let installed_root = directory.join("installed");
        let build_info = fixture_build_info();
        let bundle_info = fixture_bundle_info('a');
        write_runtime_fixture(&bundled, &build_info, &bundle_info);
        let destination = installed_runtime_path_in(
            &installed_root,
            &build_info.version,
            &build_info.platform,
            &build_info.arch,
        )
        .expect("installed runtime path");
        fs::create_dir_all(&destination).expect("create unmanaged destination");
        fs::write(destination.join("keep.txt"), "user data").expect("write unmanaged sentinel");

        let error = install_runtime_into(&bundled, &build_info, &bundle_info, &installed_root)
            .expect_err("unmanaged runtime must not be overwritten");
        assert!(error.to_string().contains("不是由 Corbit 管理"));
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).expect("read unmanaged sentinel"),
            "user data"
        );

        fs::remove_dir_all(directory).expect("remove unmanaged runtime test directory");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_runtime_links_that_escape_the_bundle() {
        use std::os::unix::fs::symlink;

        let directory =
            std::env::temp_dir().join(format!("corbit-runtime-link-{}", Uuid::new_v4()));
        let bundled = directory.join("bundled");
        let destination = directory.join("copied");
        fs::create_dir_all(&bundled).expect("create linked runtime fixture");
        let external = directory.join("external.js");
        fs::write(&external, "external").expect("write external target");
        symlink(&external, bundled.join("external.js")).expect("create external runtime link");

        let error = copy_runtime_tree(&bundled, &destination)
            .expect_err("external runtime link must be rejected");
        assert!(error.to_string().contains("越过随包根目录"));

        fs::remove_dir_all(directory).expect("remove linked runtime test directory");
    }

    #[test]
    fn rotates_bounded_log_generations() {
        let directory = std::env::temp_dir().join(format!("corbit-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create log test directory");
        let path = directory.join(LOG_FILE_NAME);
        fs::write(&path, "current").expect("write current log");
        fs::write(rotated_log_path(&path, 1), "one").expect("write first log");
        fs::write(rotated_log_path(&path, 2), "two").expect("write second log");
        fs::write(rotated_log_path(&path, 3), "three").expect("write third log");

        rotate_log_generations(&path).expect("rotate logs");

        assert_eq!(
            fs::read_to_string(rotated_log_path(&path, 1)).expect("read first log"),
            "current"
        );
        assert_eq!(
            fs::read_to_string(rotated_log_path(&path, 2)).expect("read second log"),
            "one"
        );
        assert_eq!(
            fs::read_to_string(rotated_log_path(&path, 3)).expect("read third log"),
            "two"
        );
        fs::remove_dir_all(directory).expect("remove log test directory");
    }
}
