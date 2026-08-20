use corbit_protocol::ProtocolErrorBody;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("failed to start the daemon client runtime: {0}")]
    RuntimeStart(String),
    #[error("invalid daemon configuration: {0}")]
    InvalidConfiguration(String),
    #[error("invalid daemon endpoint: {0}")]
    InvalidEndpoint(#[source] url::ParseError),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("WebSocket transport failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("daemon authentication failed")]
    AuthenticationFailed,
    #[error("daemon protocol {actual} is incompatible; client requires {expected}")]
    IncompatibleProtocol { expected: u32, actual: u32 },
    #[error("daemon closed the connection: code={code:?}, reason={reason}")]
    ConnectionClosed { code: Option<u16>, reason: String },
    #[error("daemon connection was lost: {0}")]
    ConnectionLost(String),
    #[error("daemon session failed: {0}")]
    SessionFailed(String),
    #[error("daemon is not connected")]
    NotConnected,
    #[error("timed out while {operation}")]
    Timeout { operation: &'static str },
    #[error("daemon health check returned status {0:?}")]
    Unhealthy(String),
    #[error("invalid daemon message: {0}")]
    InvalidMessage(#[from] serde_json::Error),
    #[error("daemon protocol error {0:?}")]
    Protocol(ProtocolErrorBody),
    #[error("daemon event synchronization failed: {0}")]
    EventSynchronization(String),
    #[error("daemon event sequence has a gap: expected {expected}, received {actual}")]
    EventSequenceGap { expected: u64, actual: u64 },
    #[error("daemon RPC error {0:?}")]
    Rpc(ProtocolErrorBody),
    #[error("unexpected daemon message while {operation}")]
    UnexpectedMessage { operation: &'static str },
}
