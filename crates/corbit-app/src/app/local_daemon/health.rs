use std::{
    io::{Read as _, Write as _},
    net::{TcpStream, ToSocketAddrs as _},
    time::Duration,
};

use serde::Deserialize;
use url::{Host, Url};

use super::ownership::DaemonOwner;

const HEALTH_TIMEOUT: Duration = Duration::from_millis(350);
const MAX_HEALTH_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HealthStatus {
    Offline,
    Online {
        version: String,
        owner: Option<DaemonOwner>,
    },
}

pub(super) fn managed_health_url(endpoint: &str) -> Option<Url> {
    let mut url = Url::parse(endpoint).ok()?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port_or_known_default() != Some(6768)
    {
        return None;
    }
    let loopback = match url.host()? {
        Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    };
    if !loopback {
        return None;
    }
    url.set_path("/health");
    Some(url)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: String,
    version: Option<String>,
    desktop_owner: Option<DaemonOwner>,
}

pub(super) fn status(url: &Url) -> HealthStatus {
    let Some(host) = url.host_str() else {
        return HealthStatus::Offline;
    };
    let Some(port) = url.port_or_known_default() else {
        return HealthStatus::Offline;
    };
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return HealthStatus::Offline;
    };
    let host_header = match url.host() {
        Some(Host::Ipv6(address)) => format!("[{address}]:{port}"),
        _ => format!("{host}:{port}"),
    };

    for address in addresses {
        let Ok(mut stream) = TcpStream::connect_timeout(&address, HEALTH_TIMEOUT) else {
            continue;
        };
        if stream.set_read_timeout(Some(HEALTH_TIMEOUT)).is_err()
            || stream.set_write_timeout(Some(HEALTH_TIMEOUT)).is_err()
        {
            continue;
        }
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {host_header}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            url.path()
        );
        if stream.write_all(request.as_bytes()).is_err() {
            continue;
        }
        let mut response = String::new();
        if stream
            .take(MAX_HEALTH_RESPONSE_BYTES)
            .read_to_string(&mut response)
            .is_err()
        {
            continue;
        }
        let Some((headers, body)) = response.split_once("\r\n\r\n") else {
            continue;
        };
        let status_ok = headers.lines().next().is_some_and(|line| {
            line.starts_with("HTTP/1.1 200 ") || line.starts_with("HTTP/1.0 200 ")
        });
        let Ok(body) = serde_json::from_str::<HealthResponse>(body) else {
            continue;
        };
        if status_ok && body.status == "ok" {
            return HealthStatus::Online {
                version: body.version.unwrap_or_else(|| "未知版本".into()),
                owner: body.desktop_owner,
            };
        }
    }
    HealthStatus::Offline
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        thread,
    };

    use super::*;

    fn status_for_body(body: String) -> HealthStatus {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("health test listener");
        let address = listener.local_addr().expect("health test address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("health test connection");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("health test response");
        });
        let url = Url::parse(&format!("http://{address}/health")).expect("health test URL");
        let status = status(&url);
        server.join().expect("health test server");
        status
    }

    #[test]
    fn manages_only_the_default_plain_http_loopback_endpoint() {
        assert_eq!(
            managed_health_url("http://127.0.0.1:6768")
                .expect("default endpoint should be managed")
                .as_str(),
            "http://127.0.0.1:6768/health"
        );
        assert!(managed_health_url("http://localhost:6768/").is_some());
        assert!(managed_health_url("http://[::1]:6768").is_some());
        assert!(managed_health_url("https://127.0.0.1:6768").is_none());
        assert!(managed_health_url("http://127.0.0.1:7777").is_none());
        assert!(managed_health_url("http://192.168.1.8:6768").is_none());
        assert!(managed_health_url("http://127.0.0.1:6768/api").is_none());
    }

    #[test]
    fn reads_versions_and_optional_desktop_ownership() {
        assert_eq!(
            status_for_body(r#"{"status":"ok","version":"0.1.0"}"#.into()),
            HealthStatus::Online {
                version: "0.1.0".into(),
                owner: None,
            }
        );
        assert_eq!(
            status_for_body(r#"{"status":"ok"}"#.into()),
            HealthStatus::Online {
                version: "未知版本".into(),
                owner: None,
            }
        );
        let owner = DaemonOwner {
            id: "018f9d7a-7280-7c9b-9a35-9c4aef104b02".into(),
            pid: 4242,
        };
        assert_eq!(
            status_for_body(format!(
                r#"{{"status":"ok","version":"0.1.0","desktopOwner":{{"id":"{}","pid":{}}}}}"#,
                owner.id, owner.pid
            )),
            HealthStatus::Online {
                version: "0.1.0".into(),
                owner: Some(owner),
            }
        );
    }
}
