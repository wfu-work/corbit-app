//! Thin Corbit adapter for Relay Protocol v1.
//!
//! The relay is intentionally only a transport hop: Corbit messages stay as
//! opaque `stream.message.payload` values and are decoded by the normal daemon
//! protocol session after they arrive.  Endpoint proof signing happens here so
//! private key material never enters a URL or a Relay frame outside the proof.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use ring::{
    rand::SecureRandom,
    signature::{ED25519, KeyPair, UnparsedPublicKey},
};
use serde_json::{Value, json};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::ClientError;

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Endpoint credentials used by a Corbit client connecting to Relay.
#[derive(Clone, Debug)]
pub struct RelayConfig {
    pub url: Url,
    pub space_id: String,
    pub endpoint_id: String,
    pub endpoint_type: String,
    /// Short-lived Connect Token issued for this exact Endpoint public key.
    pub connect_token: String,
    /// Unpadded base64url raw Ed25519 public key (32 bytes).
    pub endpoint_public_key: String,
    /// Unpadded base64url PKCS#8 Ed25519 private key DER.
    pub endpoint_private_key: String,
    pub endpoint_name: Option<String>,
    pub target_endpoint_id: Option<String>,
}

impl RelayConfig {
    pub fn validate(&self) -> Result<(), ClientError> {
        if self.url.scheme() != "ws" && self.url.scheme() != "wss" {
            return Err(ClientError::InvalidConfiguration(
                "Relay endpoint must use ws or wss".into(),
            ));
        }
        if self.url.path() != "/v1/connect" {
            return Err(ClientError::InvalidConfiguration(
                "Relay URL must point to /v1/connect".into(),
            ));
        }
        if self.url.username() != ""
            || self.url.password().is_some()
            || self.url.query().is_some()
            || self.url.fragment().is_some()
        {
            return Err(ClientError::InvalidConfiguration(
                "Relay URL must not contain credentials, query, or fragment".into(),
            ));
        }
        for (name, value) in [
            ("Relay Space ID", &self.space_id),
            ("Relay Endpoint ID", &self.endpoint_id),
            ("Relay Endpoint type", &self.endpoint_type),
            ("Relay Connect Token", &self.connect_token),
        ] {
            if value.trim().is_empty() || value.len() > 256 {
                return Err(ClientError::InvalidConfiguration(format!(
                    "{name} is invalid"
                )));
            }
        }
        let public = decode(&self.endpoint_public_key, "Relay public key")?;
        if public.len() != 32 {
            return Err(ClientError::InvalidConfiguration(
                "Relay public key must contain 32 bytes".into(),
            ));
        }
        let private = decode(&self.endpoint_private_key, "Relay private key")?;
        if private.len() < 32 {
            return Err(ClientError::InvalidConfiguration(
                "Relay private key is invalid".into(),
            ));
        }
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(&private).map_err(|_| {
            ClientError::InvalidConfiguration(
                "Relay private key is not a PKCS#8 Ed25519 key".into(),
            )
        })?;
        if key_pair.public_key().as_ref() != public.as_slice() {
            return Err(ClientError::InvalidConfiguration(
                "Relay public and private keys do not match".into(),
            ));
        }
        Ok(())
    }

    fn hello(&self) -> Result<Value, ClientError> {
        self.validate()?;
        let request_id = format!("hello_{}", Uuid::new_v4());
        let issued_at = chrono_millis();
        let nonce = URL_SAFE_NO_PAD.encode(rand_nonce());
        let canonical = format!(
            "relay-connect-v1\n1\n{request_id}\n{}\n{}\n{}\n{}\n{issued_at}\n{nonce}",
            self.space_id, self.endpoint_id, self.endpoint_type, self.connect_token
        );
        let private = decode(&self.endpoint_private_key, "Relay private key")?;
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(&private).map_err(|_| {
            ClientError::InvalidConfiguration("Relay private key is invalid".into())
        })?;
        let signature = URL_SAFE_NO_PAD.encode(key_pair.sign(canonical.as_bytes()).as_ref());
        Ok(json!({
            "version": 1,
            "type": "connect.hello",
            "requestId": request_id,
            "spaceId": self.space_id,
            "endpointId": self.endpoint_id,
            "endpointType": self.endpoint_type,
            "endpointName": self.endpoint_name,
            "token": self.connect_token,
            "endpointProof": {
                "algorithm": "Ed25519",
                "publicKey": self.endpoint_public_key,
                "issuedAt": issued_at,
                "nonce": nonce,
                "signature": signature,
            },
            "capabilities": ["corbit.protocol.v1", "streams", "ack", "opaque-payload"],
        }))
    }
}

/// Result of the Relay handshake. The caller owns the socket and can tunnel
/// Corbit JSON frames with [`RelayConnection::send_json`] and [`recv_json`].
pub struct RelayConnection {
    socket: Socket,
    pub connection_id: String,
    pub session_id: String,
    pub max_frame_size: u64,
    config: RelayConfig,
    send_sequence: u64,
}

impl RelayConnection {
    pub async fn connect(config: RelayConfig) -> Result<Self, ClientError> {
        let url = config.url.clone();
        let (mut socket, _) = connect_async(url.as_str()).await?;
        socket
            .send(Message::text(config.hello()?.to_string()))
            .await?;
        loop {
            let frame = socket.next().await.ok_or(ClientError::NotConnected)??;
            match frame {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_ref())?;
                    match value.get("type").and_then(Value::as_str) {
                        Some("connect.welcome") => {
                            let version = value
                                .get("version")
                                .and_then(Value::as_u64)
                                .unwrap_or_default();
                            if version != 1 {
                                return Err(ClientError::IncompatibleProtocol {
                                    expected: 1,
                                    actual: u32::try_from(version).unwrap_or_default(),
                                });
                            }
                            let connection_id = required_string(&value, "connectionId")?;
                            let session_id = required_string(&value, "sessionId")?;
                            let space_id = required_string(&value, "spaceId")?;
                            if space_id != config.space_id {
                                return Err(ClientError::RelayAuthentication {
                                    code: "identity_mismatch".into(),
                                });
                            }
                            let endpoint_id = required_string(&value, "endpointId")?;
                            if endpoint_id != config.endpoint_id {
                                return Err(ClientError::RelayAuthentication {
                                    code: "identity_mismatch".into(),
                                });
                            }
                            let max_frame_size = value
                                .get("maxFrameSize")
                                .and_then(Value::as_u64)
                                .unwrap_or(0);
                            if max_frame_size == 0 {
                                return Err(ClientError::InvalidMessage(serde_json::Error::io(
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "Relay maxFrameSize is invalid",
                                    ),
                                )));
                            }
                            return Ok(Self {
                                socket,
                                connection_id,
                                session_id,
                                max_frame_size,
                                config,
                                send_sequence: 0,
                            });
                        }
                        Some("relay.error") => {
                            let code = value
                                .get("code")
                                .and_then(Value::as_str)
                                .unwrap_or("auth.failed");
                            if code.starts_with("auth.") {
                                return Err(ClientError::RelayAuthentication { code: code.into() });
                            }
                            return Err(ClientError::ConnectionLost(code.into()));
                        }
                        _ => {
                            return Err(ClientError::UnexpectedMessage {
                                operation: "Relay handshaking",
                            });
                        }
                    }
                }
                Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
                Message::Close(frame) => {
                    return Err(ClientError::ConnectionClosed {
                        code: frame.as_ref().map(|f| u16::from(f.code)),
                        reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                    });
                }
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Binary(_) => {
                    return Err(ClientError::UnexpectedMessage {
                        operation: "Relay handshaking",
                    });
                }
            }
        }
    }

    pub async fn send_json(&mut self, message: &Value) -> Result<(), ClientError> {
        self.send_sequence = self.send_sequence.saturating_add(1);
        let mut envelope = json!({
            "version": 1,
            "type": "stream.message",
            "messageId": format!("msg_{}", Uuid::new_v4()),
            "streamId": format!("corbit:{}", self.config.endpoint_id),
            "sequence": self.send_sequence,
            "protocol": "corbit.v1",
            "payload": message,
        });
        if let Some(target) = &self.config.target_endpoint_id {
            envelope["to"] = Value::String(target.clone());
        }
        self.socket
            .send(Message::text(envelope.to_string()))
            .await?;
        Ok(())
    }

    pub async fn recv_json(&mut self) -> Result<Value, ClientError> {
        loop {
            let frame = self
                .socket
                .next()
                .await
                .ok_or(ClientError::NotConnected)??;
            match frame {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_ref())?;
                    match value.get("type").and_then(Value::as_str) {
                        Some("stream.message") => {
                            let protocol = value
                                .get("protocol")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let stream_id = value
                                .get("streamId")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if protocol == "relay.v1" && stream_id == "relay-control" {
                                continue;
                            }
                            if protocol != "corbit.v1" {
                                continue;
                            }
                            let payload =
                                value.get("payload").ok_or(ClientError::UnexpectedMessage {
                                    operation: "reading the Relay payload",
                                })?;
                            return Ok(payload.clone());
                        }
                        Some("relay.error") => {
                            let code = value
                                .get("code")
                                .and_then(Value::as_str)
                                .unwrap_or("relay.error");
                            if code.starts_with("auth.") {
                                return Err(ClientError::RelayAuthentication { code: code.into() });
                            }
                            return Err(ClientError::ConnectionLost(code.into()));
                        }
                        Some("stream.open")
                        | Some("stream.ack")
                        | Some("pong")
                        | Some("relay.peer_joined")
                        | Some("relay.peer_left") => {}
                        _ => {
                            return Err(ClientError::UnexpectedMessage {
                                operation: "Relay session",
                            });
                        }
                    }
                }
                Message::Ping(payload) => self.socket.send(Message::Pong(payload)).await?,
                Message::Close(frame) => {
                    return Err(ClientError::ConnectionClosed {
                        code: frame.as_ref().map(|f| u16::from(f.code)),
                        reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                    });
                }
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Binary(_) => {
                    return Err(ClientError::UnexpectedMessage {
                        operation: "Relay session",
                    });
                }
            }
        }
    }

    pub async fn close(&mut self) -> Result<(), ClientError> {
        self.socket.close(None).await?;
        Ok(())
    }

    pub(crate) fn into_parts(self) -> (Socket, RelayConfig) {
        (self.socket, self.config)
    }
}

fn decode(value: &str, name: &str) -> Result<Vec<u8>, ClientError> {
    URL_SAFE_NO_PAD.decode(value).map_err(|_| {
        ClientError::InvalidConfiguration(format!("{name} is not canonical base64url"))
    })
}

fn required_string(value: &Value, name: &'static str) -> Result<String, ClientError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or(ClientError::UnexpectedMessage {
            operation: "Relay handshaking",
        })
}

fn rand_nonce() -> [u8; 24] {
    let mut nonce = [0_u8; 24];
    // `ring::rand` avoids pulling an OS-specific RNG into the adapter.
    ring::rand::SystemRandom::new()
        .fill(&mut nonce)
        .expect("system RNG unavailable");
    nonce
}

fn chrono_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// Keep the public-key verification primitive linked into builds where a caller
// wants to verify a fixture before connecting; signing uses Ed25519KeyPair above.
#[allow(dead_code)]
fn verify_public_key(public: &[u8], message: &[u8], signature: &[u8]) -> bool {
    UnparsedPublicKey::new(&ED25519, public)
        .verify(message, signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    fn fixture() -> RelayConfig {
        let rng = SystemRandom::new();
        let key = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).expect("key");
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(key.as_ref()).expect("pair");
        RelayConfig {
            url: Url::parse("wss://relay.example.test/v1/connect").expect("url"),
            space_id: "space-1".into(),
            endpoint_id: "desktop-1".into(),
            endpoint_type: "app".into(),
            connect_token: "token-1".into(),
            endpoint_public_key: URL_SAFE_NO_PAD.encode(pair.public_key().as_ref()),
            endpoint_private_key: URL_SAFE_NO_PAD.encode(key.as_ref()),
            endpoint_name: Some("Corbit Desktop".into()),
            target_endpoint_id: Some("bridge-1".into()),
        }
    }

    #[test]
    fn signs_protocol_v1_endpoint_proof() {
        let config = fixture();
        let hello = config.hello().expect("hello");
        let proof = hello.get("endpointProof").expect("proof");
        let canonical = format!(
            "relay-connect-v1\n1\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            hello["requestId"].as_str().unwrap(),
            hello["spaceId"].as_str().unwrap(),
            hello["endpointId"].as_str().unwrap(),
            hello["endpointType"].as_str().unwrap(),
            hello["token"].as_str().unwrap(),
            proof["issuedAt"].as_i64().unwrap(),
            proof["nonce"].as_str().unwrap(),
        );
        let public = URL_SAFE_NO_PAD
            .decode(config.endpoint_public_key.as_bytes())
            .unwrap();
        let signature = URL_SAFE_NO_PAD
            .decode(proof["signature"].as_str().unwrap())
            .unwrap();
        assert!(verify_public_key(&public, canonical.as_bytes(), &signature));
    }

    #[test]
    fn rejects_relay_urls_with_credentials_or_query() {
        let mut config = fixture();
        config.url = Url::parse("wss://relay.example.test/v1/connect?token=secret").unwrap();
        assert!(config.validate().is_err());
    }
}
