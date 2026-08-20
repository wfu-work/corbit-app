use std::{
    env, fs,
    io::ErrorKind,
    net::IpAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

pub(super) const DEFAULT_DAEMON_ENDPOINT: &str = "http://127.0.0.1:6768";

const DAEMON_ENDPOINT_ENVIRONMENT: &str = "CORBIT_DAEMON_URL";
const DAEMON_TOKEN_ENVIRONMENT: &str = "CORBIT_AUTH_TOKEN";
const CORBIT_HOME_ENVIRONMENT: &str = "CORBIT_HOME";
const DAEMON_CREDENTIALS_FILE: &str = "credentials.json";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "com.xiaoxi.corbit.desktop";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "daemon-auth-token";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ConnectionPreferences {
    pub(super) endpoint: String,
}

impl Default for ConnectionPreferences {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_DAEMON_ENDPOINT.into(),
        }
    }
}

impl ConnectionPreferences {
    pub(super) fn load() -> Self {
        preferences_path()
            .and_then(|path| Self::load_from(&path).ok())
            .unwrap_or_default()
    }

    pub(super) fn save(&self) -> anyhow::Result<()> {
        let path = preferences_path().ok_or_else(|| anyhow!("无法确定当前用户的配置目录"))?;
        self.save_to(&path)
    }

    pub(super) fn resolved_endpoint(&self) -> ResolvedEndpoint {
        self.resolved_endpoint_with_override(environment_value(DAEMON_ENDPOINT_ENVIRONMENT))
    }

    fn resolved_endpoint_with_override(&self, environment: Option<String>) -> ResolvedEndpoint {
        environment.map_or_else(
            || ResolvedEndpoint {
                value: self.endpoint.clone(),
                environment_override: false,
            },
            |value| ResolvedEndpoint {
                value,
                environment_override: true,
            },
        )
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        let bytes =
            fs::read(path).with_context(|| format!("无法读取连接设置 {}", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("无法解析连接设置 {}", path.display()))
    }

    fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("连接设置路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建配置目录 {}", parent.display()))?;
        let bytes = serde_json::to_vec_pretty(self).context("无法序列化连接设置")?;
        fs::write(path, bytes).with_context(|| format!("无法写入连接设置 {}", path.display()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResolvedEndpoint {
    pub(super) value: String,
    pub(super) environment_override: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CredentialSource {
    Environment,
    SystemStore,
    LocalDaemon,
}

impl CredentialSource {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Environment => "环境变量（本次启动优先）",
            Self::SystemStore => system_store_label(),
            Self::LocalDaemon => "本机 Daemon（自动发现）",
        }
    }
}

pub(super) struct CredentialResolution {
    pub(super) token: Option<String>,
    pub(super) source: Option<CredentialSource>,
    pub(super) system_credential_present: bool,
    pub(super) error: Option<String>,
}

pub(super) fn resolve_credentials(endpoint: &str) -> CredentialResolution {
    let environment_token = environment_value(DAEMON_TOKEN_ENVIRONMENT);
    let environment_present = environment_token.is_some();

    match load_system_credential() {
        Ok(system_token) => {
            let system_credential_present = system_token.is_some();
            if environment_present || system_credential_present {
                return CredentialResolution {
                    token: environment_token.or(system_token),
                    source: if environment_present {
                        Some(CredentialSource::Environment)
                    } else {
                        Some(CredentialSource::SystemStore)
                    },
                    system_credential_present,
                    error: None,
                };
            }

            resolve_local_daemon_credential(endpoint, system_credential_present, None)
        }
        Err(_error) if environment_present => CredentialResolution {
            token: environment_token,
            source: Some(CredentialSource::Environment),
            system_credential_present: false,
            error: None,
        },
        Err(error) => resolve_local_daemon_credential(endpoint, false, Some(error.to_string())),
    }
}

fn resolve_local_daemon_credential(
    endpoint: &str,
    system_credential_present: bool,
    earlier_error: Option<String>,
) -> CredentialResolution {
    match load_local_daemon_credential(endpoint) {
        Ok(Some(token)) => CredentialResolution {
            token: Some(token),
            source: Some(CredentialSource::LocalDaemon),
            system_credential_present,
            error: None,
        },
        Ok(None) => CredentialResolution {
            token: None,
            source: None,
            system_credential_present,
            error: earlier_error,
        },
        Err(error) => CredentialResolution {
            token: None,
            source: None,
            system_credential_present,
            error: Some(match earlier_error {
                Some(earlier) => format!("{earlier}；{error}"),
                None => error.to_string(),
            }),
        },
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalDaemonCredentials {
    server_id: String,
    token: String,
}

fn load_local_daemon_credential(endpoint: &str) -> anyhow::Result<Option<String>> {
    if !is_loopback_endpoint(endpoint) {
        return Ok(None);
    }

    let path = local_daemon_credentials_path()?;
    load_local_daemon_credential_from(endpoint, &path)
}

fn load_local_daemon_credential_from(
    endpoint: &str,
    path: &Path,
) -> anyhow::Result<Option<String>> {
    if !is_loopback_endpoint(endpoint) {
        return Ok(None);
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("无法读取本机 Daemon 凭据 {}", path.display()));
        }
    };
    let credentials: LocalDaemonCredentials = serde_json::from_slice(&bytes)
        .with_context(|| format!("无法解析本机 Daemon 凭据 {}", path.display()))?;
    if credentials.server_id.trim().is_empty() {
        return Err(anyhow!("本机 Daemon 凭据中的 Server ID 无效"));
    }
    if credentials.token.len() < 32 || credentials.token.trim() != credentials.token {
        return Err(anyhow!("本机 Daemon 凭据中的 Token 无效"));
    }

    Ok(Some(credentials.token))
}

pub(super) fn is_loopback_endpoint(endpoint: &str) -> bool {
    let Ok(url) = Url::parse(endpoint) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

fn local_daemon_credentials_path() -> anyhow::Result<PathBuf> {
    Ok(resolve_local_daemon_home(
        environment_value(CORBIT_HOME_ENVIRONMENT).as_deref(),
        user_home_directory().as_deref(),
        env::current_dir().ok().as_deref(),
    )?
    .join(DAEMON_CREDENTIALS_FILE))
}

fn resolve_local_daemon_home(
    configured: Option<&str>,
    user_home: Option<&Path>,
    current_directory: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let Some(configured) = configured else {
        return user_home
            .map(|home| home.join(".corbit"))
            .ok_or_else(|| anyhow!("无法确定本机 Daemon 凭据目录"));
    };

    if configured == "~" {
        return user_home
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("CORBIT_HOME 使用了 ~，但无法确定用户目录"));
    }
    if let Some(relative) = configured.strip_prefix("~/") {
        return user_home
            .map(|home| home.join(relative))
            .ok_or_else(|| anyhow!("CORBIT_HOME 使用了 ~，但无法确定用户目录"));
    }

    let path = PathBuf::from(configured);
    if path.is_absolute() {
        return Ok(path);
    }
    current_directory
        .map(|directory| directory.join(path))
        .ok_or_else(|| anyhow!("CORBIT_HOME 是相对路径，但无法确定当前目录"))
}

fn user_home_directory() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(target_os = "windows")
                .then(|| env::var_os("USERPROFILE"))
                .flatten()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

pub(super) fn save_system_credential(token: &str) -> anyhow::Result<()> {
    if token.trim().is_empty() {
        return Err(anyhow!("Daemon Token 不能为空"));
    }
    platform_store::save(token)
}

pub(super) fn delete_system_credential() -> anyhow::Result<bool> {
    platform_store::delete()
}

pub(super) const fn system_store_supported() -> bool {
    cfg!(target_os = "macos")
}

pub(super) const fn system_store_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS 钥匙串"
    } else if cfg!(target_os = "windows") {
        "Windows 凭据管理器（待实现）"
    } else {
        "系统密钥环（待实现）"
    }
}

fn environment_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn load_system_credential() -> anyhow::Result<Option<String>> {
    platform_store::load()
}

#[cfg(target_os = "macos")]
mod platform_store {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use security_framework_sys::base::errSecItemNotFound;

    pub(super) fn load() -> anyhow::Result<Option<String>> {
        match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Ok(bytes) => String::from_utf8(bytes)
                .context("macOS 钥匙串中的 Corbit Token 不是有效 UTF-8")
                .map(Some),
            Err(error) if error.code() == errSecItemNotFound => Ok(None),
            Err(error) => Err(anyhow!("无法读取 macOS 钥匙串中的 Corbit Token：{error}")),
        }
    }

    pub(super) fn save(token: &str) -> anyhow::Result<()> {
        set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, token.as_bytes())
            .map_err(|error| anyhow!("无法将 Corbit Token 保存到 macOS 钥匙串：{error}"))
    }

    pub(super) fn delete() -> anyhow::Result<bool> {
        match delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Ok(()) => Ok(true),
            Err(error) if error.code() == errSecItemNotFound => Ok(false),
            Err(error) => Err(anyhow!("无法从 macOS 钥匙串移除 Corbit Token：{error}")),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform_store {
    use super::*;

    pub(super) fn load() -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    pub(super) fn save(_token: &str) -> anyhow::Result<()> {
        Err(anyhow!(
            "当前平台尚未实现系统凭证存储，请使用 CORBIT_AUTH_TOKEN"
        ))
    }

    pub(super) fn delete() -> anyhow::Result<bool> {
        Err(anyhow!("当前平台尚未实现系统凭证存储"))
    }
}

#[cfg(target_os = "macos")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Corbit")
            .join("connection.json")
    })
}

#[cfg(target_os = "windows")]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("APPDATA").map(|directory| {
        PathBuf::from(directory)
            .join("Corbit")
            .join("connection.json")
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn preferences_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|directory| directory.join("corbit").join("connection.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOKEN: &str = "local-daemon-test-token-with-32-characters";

    #[test]
    fn connection_preferences_round_trip_and_accept_partial_files() {
        let directory = std::env::temp_dir().join(format!(
            "corbit-connection-settings-test-{}",
            uuid::Uuid::new_v4()
        ));
        let path = directory.join("connection.json");
        let preferences = ConnectionPreferences {
            endpoint: "https://daemon.example.test/corbit".into(),
        };

        preferences
            .save_to(&path)
            .expect("connection preferences should save");
        assert_eq!(
            ConnectionPreferences::load_from(&path).expect("connection preferences should load"),
            preferences
        );

        fs::write(&path, b"{}").expect("partial connection preferences should save");
        assert_eq!(
            ConnectionPreferences::load_from(&path)
                .expect("partial connection preferences should load"),
            ConnectionPreferences::default()
        );

        fs::remove_dir_all(directory).expect("temporary connection settings should be removable");
    }

    #[test]
    fn environment_endpoint_takes_precedence_without_changing_preferences() {
        let preferences = ConnectionPreferences {
            endpoint: "http://127.0.0.1:6768".into(),
        };
        let resolved =
            preferences.resolved_endpoint_with_override(Some("https://daemon.example.test".into()));

        assert_eq!(resolved.value, "https://daemon.example.test");
        assert!(resolved.environment_override);
        assert_eq!(preferences.endpoint, "http://127.0.0.1:6768");
    }

    #[test]
    fn connection_preferences_never_serialize_a_token() {
        let json = serde_json::to_string(&ConnectionPreferences::default())
            .expect("connection preferences should serialize");

        assert!(json.contains("endpoint"));
        assert!(!json.to_ascii_lowercase().contains("token"));
    }

    #[test]
    fn recognizes_only_http_loopback_endpoints_for_local_discovery() {
        assert!(is_loopback_endpoint("http://127.0.0.1:6768"));
        assert!(is_loopback_endpoint("https://127.20.30.40:6768"));
        assert!(is_loopback_endpoint("http://localhost:6768"));
        assert!(is_loopback_endpoint("http://[::1]:6768"));

        assert!(!is_loopback_endpoint("https://daemon.example.test"));
        assert!(!is_loopback_endpoint("http://192.168.1.8:6768"));
        assert!(!is_loopback_endpoint("ws://127.0.0.1:6768"));
        assert!(!is_loopback_endpoint("not a URL"));
    }

    #[test]
    fn reads_daemon_credentials_for_a_loopback_endpoint() {
        let directory = std::env::temp_dir().join(format!(
            "corbit-local-daemon-credentials-test-{}",
            uuid::Uuid::new_v4()
        ));
        let path = directory.join("credentials.json");
        fs::create_dir_all(&directory).expect("temporary credential directory should be created");
        fs::write(
            &path,
            format!(r#"{{"serverId":"srv_test","token":"{TEST_TOKEN}"}}"#),
        )
        .expect("temporary credentials should be written");

        assert_eq!(
            load_local_daemon_credential_from(DEFAULT_DAEMON_ENDPOINT, &path)
                .expect("local credentials should load"),
            Some(TEST_TOKEN.to_owned())
        );

        fs::remove_dir_all(directory).expect("temporary credential directory should be removed");
    }

    #[test]
    fn never_reads_local_root_credentials_for_a_remote_endpoint() {
        let missing_path = std::env::temp_dir().join(format!(
            "corbit-missing-local-daemon-credentials-{}",
            uuid::Uuid::new_v4()
        ));

        assert_eq!(
            load_local_daemon_credential_from("https://daemon.example.test", &missing_path)
                .expect("remote endpoints should skip local credential discovery"),
            None
        );
    }

    #[test]
    fn resolves_default_custom_and_tilde_daemon_homes() {
        let user_home = Path::new("/Users/corbit-test");
        let current_directory = Path::new("/work/corbit-daemon");

        assert_eq!(
            resolve_local_daemon_home(None, Some(user_home), Some(current_directory)).unwrap(),
            user_home.join(".corbit")
        );
        assert_eq!(
            resolve_local_daemon_home(Some("~/daemon-data"), Some(user_home), None).unwrap(),
            user_home.join("daemon-data")
        );
        assert_eq!(
            resolve_local_daemon_home(
                Some("development-data"),
                Some(user_home),
                Some(current_directory)
            )
            .unwrap(),
            current_directory.join("development-data")
        );
    }
}
