use std::{collections::BTreeMap, time::Duration};

use corbit_protocol::{ClientIdentity, ClientKind};
use url::Url;

use crate::ClientError;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoint: Url,
    pub token: String,
    pub client: ClientIdentity,
    pub capabilities: Vec<String>,
    pub connect_timeout: Duration,
    pub rpc_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub reconnect_initial_delay: Duration,
    pub reconnect_max_delay: Duration,
}

impl ClientConfig {
    /// Creates the default desktop client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint is not an absolute HTTP(S) URL or the
    /// token is shorter than the minimum accepted length.
    pub fn desktop(
        endpoint: impl AsRef<str>,
        token: impl Into<String>,
    ) -> Result<Self, ClientError> {
        let endpoint = normalize_endpoint(endpoint.as_ref())?;
        let token = token.into();
        if token.len() < 32 {
            return Err(ClientError::InvalidConfiguration(
                "daemon token must contain at least 32 characters".into(),
            ));
        }

        Ok(Self {
            endpoint,
            token,
            client: ClientIdentity {
                id: format!("desktop_{}", uuid::Uuid::new_v4()),
                kind: ClientKind::Desktop,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                platform: std::env::consts::OS.to_owned(),
                extensions: BTreeMap::default(),
            },
            capabilities: vec![
                "heartbeat.v1".into(),
                "rpc.system.echo.v1".into(),
                "rpc.state.snapshot.v1".into(),
                "rpc.resource.mutations.v1".into(),
                "workspace.watch.v1".into(),
            ],
            connect_timeout: Duration::from_secs(10),
            rpc_timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(20),
            reconnect_initial_delay: Duration::from_millis(500),
            reconnect_max_delay: Duration::from_secs(15),
        })
    }

    pub(crate) fn http_url(&self, path: &str) -> Result<Url, ClientError> {
        self.endpoint
            .join(path)
            .map_err(ClientError::InvalidEndpoint)
    }

    pub(crate) fn websocket_url(&self) -> Result<Url, ClientError> {
        let mut url = self.http_url("ws")?;
        match url.scheme() {
            "http" => url
                .set_scheme("ws")
                .map_err(|()| ClientError::InvalidConfiguration("invalid HTTP scheme".into()))?,
            "https" => url
                .set_scheme("wss")
                .map_err(|()| ClientError::InvalidConfiguration("invalid HTTPS scheme".into()))?,
            scheme => {
                return Err(ClientError::InvalidConfiguration(format!(
                    "unsupported daemon endpoint scheme: {scheme}"
                )));
            }
        }
        Ok(url)
    }
}

fn normalize_endpoint(value: &str) -> Result<Url, ClientError> {
    let mut endpoint = Url::parse(value).map_err(ClientError::InvalidEndpoint)?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(ClientError::InvalidConfiguration(
            "daemon endpoint must use http or https".into(),
        ));
    }
    if endpoint.cannot_be_a_base() || endpoint.host_str().is_none() {
        return Err(ClientError::InvalidConfiguration(
            "daemon endpoint must be an absolute URL".into(),
        ));
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(ClientError::InvalidConfiguration(
            "daemon endpoint must not include query parameters or fragments".into(),
        ));
    }
    if !endpoint.path().ends_with('/') {
        endpoint.set_path(&format!("{}/", endpoint.path()));
    }
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::ClientConfig;

    const TOKEN: &str = "a-token-containing-at-least-32-characters";

    #[test]
    fn normalizes_endpoint_and_derives_routes() {
        let config = ClientConfig::desktop("http://127.0.0.1:6768", TOKEN).unwrap();
        assert_eq!(
            config.http_url("health").unwrap().as_str(),
            "http://127.0.0.1:6768/health"
        );
        assert_eq!(
            config.websocket_url().unwrap().as_str(),
            "ws://127.0.0.1:6768/ws"
        );
    }

    #[test]
    fn rejects_unsafe_or_incomplete_configuration() {
        assert!(ClientConfig::desktop("ws://127.0.0.1:6768", TOKEN).is_err());
        assert!(ClientConfig::desktop("http://127.0.0.1:6768?token=secret", TOKEN).is_err());
        assert!(ClientConfig::desktop("http://127.0.0.1:6768", "short").is_err());
    }

    #[test]
    fn default_runtime_intervals_are_non_zero() {
        let config = ClientConfig::desktop("http://127.0.0.1:6768", TOKEN).unwrap();

        assert!(!config.rpc_timeout.is_zero());
        assert!(!config.connect_timeout.is_zero());
        assert!(!config.heartbeat_interval.is_zero());
        assert!(!config.reconnect_initial_delay.is_zero());
        assert!(!config.reconnect_max_delay.is_zero());
    }
}
