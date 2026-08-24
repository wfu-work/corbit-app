use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use corbit_protocol::{
    AgentApprovalDecision, AgentApprovalResolveAcknowledgement, AgentInterruptAcknowledgement,
    AgentPermissionPayload, AgentPromptAcknowledgement, AgentPromptOptions,
    AgentSteerAcknowledgement, AgentTimelinePayload, AuthoritativeSnapshot, ClientMessage,
    DeviceCredentialSummary, DeviceListResponse, EventSyncPhase, HealthResponse, PROTOCOL_VERSION,
    PairingOffer, ProviderCatalog, ResourceMutationAcknowledgement, ResumeCursor, ScheduledRun,
    ScheduledTask, ScheduledTaskCreateInput, ScheduledTaskDeleteAcknowledgement,
    ScheduledTaskUpdateInput, ServerInfo, ServerMessage, WorkspaceChanged,
    WorkspaceDirectoryListing, WorkspaceFileContent, WorkspaceGitDiff, WorkspaceGitStatus,
};
use futures_util::{SinkExt, StreamExt};
use http::header::{AUTHORIZATION, HeaderValue};
use reqwest::StatusCode;
use serde_json::Value;
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot, watch},
    time::timeout,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

use crate::{ClientConfig, ClientError};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 512;

#[derive(Debug, Default)]
struct RecoveryState {
    server_id: Option<String>,
    last_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Offline,
    Connecting,
    Authenticating,
    Online,
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
        reason: String,
    },
    AuthenticationFailed,
    Incompatible {
        expected: u32,
        actual: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionEvent {
    StateChanged(ConnectionState),
    ServerInfo(ServerInfo),
    HistoryReset,
    AgentTimeline {
        sequence: u64,
        payload: AgentTimelinePayload,
    },
    AgentPermission {
        sequence: u64,
        payload: AgentPermissionPayload,
    },
    WorkspaceChanged(WorkspaceChanged),
}

#[derive(Clone, Debug)]
pub struct CorbitClient {
    config: ClientConfig,
    http: reqwest::Client,
    events: broadcast::Sender<ConnectionEvent>,
    recovery: Arc<Mutex<RecoveryState>>,
}

impl CorbitClient {
    /// Creates a client with isolated HTTP state and a connection event channel.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured HTTP client cannot be constructed.
    pub fn new(config: ClientConfig) -> Result<Self, ClientError> {
        if config.rpc_timeout.is_zero() {
            return Err(ClientError::InvalidConfiguration(
                "RPC timeout must be greater than zero".into(),
            ));
        }
        if config.connect_timeout.is_zero() {
            return Err(ClientError::InvalidConfiguration(
                "connect timeout must be greater than zero".into(),
            ));
        }
        if config.heartbeat_interval.is_zero() {
            return Err(ClientError::InvalidConfiguration(
                "heartbeat interval must be greater than zero".into(),
            ));
        }
        if config.reconnect_initial_delay.is_zero() || config.reconnect_max_delay.is_zero() {
            return Err(ClientError::InvalidConfiguration(
                "reconnect delays must be greater than zero".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.rpc_timeout)
            .build()?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Ok(Self {
            config,
            http,
            events,
            recovery: Arc::new(Mutex::new(RecoveryState::default())),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.events.subscribe()
    }

    /// Checks whether the Daemon HTTP server is alive.
    ///
    /// # Errors
    ///
    /// Returns an error for transport failures, non-success status codes, or an
    /// invalid response body.
    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        let response = self
            .http
            .get(self.config.http_url("health")?)
            .send()
            .await?;
        response
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }

    /// Retrieves authenticated Daemon version and feature information.
    ///
    /// # Errors
    ///
    /// Returns an authentication error for an invalid token, or a transport or
    /// decoding error when the request cannot be completed.
    pub async fn info(&self) -> Result<ServerInfo, ClientError> {
        let response = self
            .http
            .get(self.config.http_url("info")?)
            .bearer_auth(&self.config.token)
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::AuthenticationFailed);
        }
        response
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }

    /// Lists mobile and remote devices paired with the root Daemon credential.
    ///
    /// # Errors
    ///
    /// Returns an authentication, transport, status, or decoding error.
    pub async fn devices(&self) -> Result<Vec<DeviceCredentialSummary>, ClientError> {
        let response = self
            .http
            .get(self.config.http_url("devices")?)
            .bearer_auth(&self.config.token)
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::AuthenticationFailed);
        }
        let response: DeviceListResponse = response.error_for_status()?.json().await?;
        Ok(response.devices)
    }

    /// Creates a single-use mobile pairing offer for this Daemon.
    ///
    /// # Errors
    ///
    /// Returns an authentication, validation, transport, status, or decoding error.
    pub async fn create_pairing(
        &self,
        endpoint: impl Into<String>,
        host_name: impl Into<String>,
    ) -> Result<PairingOffer, ClientError> {
        let response = self
            .http
            .post(self.config.http_url("pair/sessions")?)
            .bearer_auth(&self.config.token)
            .json(&serde_json::json!({
                "endpoint": endpoint.into(),
                "hostName": host_name.into(),
            }))
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::AuthenticationFailed);
        }
        response
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }

    /// Revokes a previously issued device credential.
    ///
    /// # Errors
    ///
    /// Returns an authentication, transport, or status error.
    pub async fn revoke_device(&self, device_id: &str) -> Result<(), ClientError> {
        let path = format!("devices/{device_id}");
        let response = self
            .http
            .delete(self.config.http_url(&path)?)
            .bearer_auth(&self.config.token)
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::AuthenticationFailed);
        }
        response.error_for_status()?;
        Ok(())
    }

    /// Opens the WebSocket, completes the Corbit protocol handshake, and starts
    /// the session driver.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport, authentication, or protocol version
    /// negotiation fails.
    pub async fn connect(&self) -> Result<CorbitConnection, ClientError> {
        let result = self.connect_without_online_event().await;
        if let Ok(connection) = &result {
            self.announce_online(connection);
        }
        result
    }

    pub(crate) async fn connect_without_online_event(
        &self,
    ) -> Result<CorbitConnection, ClientError> {
        self.emit(ConnectionEvent::StateChanged(ConnectionState::Connecting));

        let result = self.connect_inner().await;
        if let Err(error) = &result {
            self.emit(ConnectionEvent::StateChanged(state_for_error(error)));
        }
        result
    }

    async fn connect_inner(&self) -> Result<CorbitConnection, ClientError> {
        let websocket_url = self.config.websocket_url()?;
        let mut request = websocket_url
            .as_str()
            .into_client_request()
            .map_err(ClientError::WebSocket)?;
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.config.token)).map_err(|error| {
                ClientError::InvalidConfiguration(format!("invalid daemon token: {error}"))
            })?,
        );

        self.emit(ConnectionEvent::StateChanged(
            ConnectionState::Authenticating,
        ));
        let (mut socket, _) = timeout(self.config.connect_timeout, connect_async(request))
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "connecting to the daemon",
            })??;

        let resume = {
            let recovery = lock_recovery(&self.recovery);
            ResumeCursor {
                session_id: None,
                server_id: recovery.server_id.clone(),
                last_sequence: Some(recovery.last_sequence),
                extensions: BTreeMap::default(),
            }
        };
        let requested_after = resume.last_sequence.unwrap_or_default();
        let requested_server_id = resume.server_id.clone();
        send_json(
            &mut socket,
            &ClientMessage::hello_with_resume(
                self.config.client.clone(),
                self.config.capabilities.clone(),
                Some(resume),
            ),
        )
        .await?;

        let message =
            next_server_message(&mut socket, self.config.connect_timeout, "handshaking").await?;
        let (session_id, server_info) = parse_server_info(message)?;
        let cursor_recovery = server_info
            .features
            .get("eventCursorRecovery")
            .copied()
            .unwrap_or(false);
        if cursor_recovery {
            synchronize_event_history(
                &mut socket,
                self.config.connect_timeout,
                requested_after,
                requested_server_id.as_deref(),
                &server_info.server_id,
                &self.recovery,
                &self.events,
            )
            .await?;
        } else {
            self.prepare_legacy_history(&server_info.server_id);
        }

        let (commands, command_receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (cancellations, cancellation_receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (session_end, session_end_receiver) = watch::channel(None);
        let events = self.events.clone();
        let recovery = Arc::clone(&self.recovery);
        tokio::spawn(run_session_driver(
            socket,
            command_receiver,
            cancellation_receiver,
            session_end,
            events,
            recovery,
            cursor_recovery,
        ));

        Ok(CorbitConnection {
            commands,
            cancellations,
            session_end: session_end_receiver,
            session_id: Arc::from(session_id),
            server_info: Arc::new(server_info),
            rpc_timeout: self.config.rpc_timeout,
        })
    }

    pub(crate) fn emit_state(&self, state: ConnectionState) {
        self.emit(ConnectionEvent::StateChanged(state));
    }

    pub(crate) fn announce_online(&self, connection: &CorbitConnection) {
        self.emit(ConnectionEvent::ServerInfo(
            connection.server_info().clone(),
        ));
        self.emit(ConnectionEvent::StateChanged(ConnectionState::Online));
    }

    pub(crate) fn reconnect_initial_delay(&self) -> Duration {
        self.config.reconnect_initial_delay
    }

    pub(crate) fn reconnect_max_delay(&self) -> Duration {
        self.config.reconnect_max_delay
    }

    pub(crate) fn heartbeat_interval(&self) -> Duration {
        self.config.heartbeat_interval
    }

    fn emit(&self, event: ConnectionEvent) {
        let _ = self.events.send(event);
    }

    fn prepare_legacy_history(&self, server_id: &str) {
        let mut recovery = lock_recovery(&self.recovery);
        let should_reset = recovery.server_id.is_some() || recovery.last_sequence > 0;
        recovery.server_id = Some(server_id.to_owned());
        recovery.last_sequence = 0;
        drop(recovery);
        if should_reset {
            self.emit(ConnectionEvent::HistoryReset);
        }
    }
}

fn parse_server_info(message: ServerMessage) -> Result<(String, ServerInfo), ClientError> {
    match message {
        ServerMessage::ServerInfo {
            session_id,
            server_id,
            version,
            protocol_version,
            features,
            ..
        } => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(ClientError::IncompatibleProtocol {
                    expected: PROTOCOL_VERSION,
                    actual: protocol_version,
                });
            }
            Ok((
                session_id,
                ServerInfo {
                    server_id,
                    version,
                    protocol_version,
                    features,
                },
            ))
        }
        ServerMessage::ProtocolError { error, .. } if error.code == "unauthorized" => {
            Err(ClientError::AuthenticationFailed)
        }
        ServerMessage::ProtocolError { error, .. } if error.code == "unsupported_protocol" => {
            let actual = error
                .details
                .as_ref()
                .and_then(|details| details.get("supported"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default();
            Err(ClientError::IncompatibleProtocol {
                expected: PROTOCOL_VERSION,
                actual,
            })
        }
        ServerMessage::ProtocolError { error, .. } => Err(ClientError::Protocol(error)),
        _ => Err(ClientError::UnexpectedMessage {
            operation: "handshaking",
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn synchronize_event_history(
    socket: &mut Socket,
    duration: Duration,
    requested_after: u64,
    requested_server_id: Option<&str>,
    server_id: &str,
    recovery: &Arc<Mutex<RecoveryState>>,
    events: &broadcast::Sender<ConnectionEvent>,
) -> Result<(), ClientError> {
    let message = next_server_message(socket, duration, "starting event synchronization").await?;
    let (replay_from, latest_sequence, reset) = match message {
        ServerMessage::EventSync {
            phase: EventSyncPhase::Begin,
            requested_after: echoed_after,
            replay_from,
            latest_sequence,
            reset,
            ..
        } => {
            if echoed_after != requested_after {
                return Err(ClientError::EventSynchronization(format!(
                    "Daemon acknowledged event cursor {echoed_after}, expected {requested_after}"
                )));
            }
            (replay_from, latest_sequence, reset)
        }
        ServerMessage::ProtocolError { error, .. } => return Err(ClientError::Protocol(error)),
        _ => {
            return Err(ClientError::UnexpectedMessage {
                operation: "starting event synchronization",
            });
        }
    };

    if replay_from > latest_sequence {
        return Err(ClientError::EventSynchronization(format!(
            "Daemon replay cursor {replay_from} exceeds latest sequence {latest_sequence}"
        )));
    }
    if !reset && replay_from != requested_after {
        return Err(ClientError::EventSynchronization(format!(
            "Daemon changed event cursor from {requested_after} to {replay_from} without a reset"
        )));
    }
    if requested_server_id.is_some_and(|previous| previous != server_id) && !reset {
        return Err(ClientError::EventSynchronization(
            "Daemon identity changed without resetting event history".into(),
        ));
    }

    {
        let mut state = lock_recovery(recovery);
        state.last_sequence = replay_from;
    }
    if reset {
        let _ = events.send(ConnectionEvent::HistoryReset);
    }

    loop {
        let message = next_server_message(socket, duration, "synchronizing event history").await?;
        match message {
            ServerMessage::EventSync {
                phase: EventSyncPhase::Complete,
                requested_after: echoed_after,
                replay_from: echoed_replay_from,
                latest_sequence: echoed_latest,
                reset: echoed_reset,
                ..
            } => {
                if echoed_after != requested_after
                    || echoed_replay_from != replay_from
                    || echoed_latest != latest_sequence
                    || echoed_reset != reset
                {
                    return Err(ClientError::EventSynchronization(
                        "Daemon completed event synchronization with different metadata".into(),
                    ));
                }
                let mut state = lock_recovery(recovery);
                if state.last_sequence != latest_sequence {
                    return Err(ClientError::EventSynchronization(format!(
                        "Daemon completed at sequence {latest_sequence}, but client reached {}",
                        state.last_sequence
                    )));
                }
                state.server_id = Some(server_id.to_owned());
                return Ok(());
            }
            ServerMessage::Event {
                topic,
                sequence,
                payload,
                ..
            } => route_event(&topic, sequence, payload, events, recovery, true)?,
            ServerMessage::ProtocolError { error, .. } => {
                return Err(ClientError::Protocol(error));
            }
            _ => {
                return Err(ClientError::UnexpectedMessage {
                    operation: "synchronizing event history",
                });
            }
        }
    }
}

fn lock_recovery(recovery: &Arc<Mutex<RecoveryState>>) -> MutexGuard<'_, RecoveryState> {
    recovery
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn state_for_error(error: &ClientError) -> ConnectionState {
    match error {
        ClientError::AuthenticationFailed => ConnectionState::AuthenticationFailed,
        ClientError::IncompatibleProtocol { expected, actual } => ConnectionState::Incompatible {
            expected: *expected,
            actual: *actual,
        },
        _ => ConnectionState::Offline,
    }
}

enum SessionCommand {
    Ping {
        id: String,
        response: oneshot::Sender<Result<(), ClientError>>,
    },
    Rpc {
        id: String,
        method: String,
        params: Option<Value>,
        response: oneshot::Sender<Result<Value, ClientError>>,
    },
    Close,
}

#[derive(Clone, Debug)]
enum SessionEnd {
    Graceful,
    AuthenticationFailed,
    Reconnectable(String),
    Fatal(String),
}

impl SessionEnd {
    fn from_result(result: &Result<(), ClientError>) -> Self {
        match result {
            Ok(()) => Self::Graceful,
            Err(ClientError::AuthenticationFailed) => Self::AuthenticationFailed,
            Err(error) if session_error_is_reconnectable(error) => {
                Self::Reconnectable(error.to_string())
            }
            Err(error) => Self::Fatal(error.to_string()),
        }
    }

    fn to_error(&self) -> Option<ClientError> {
        match self {
            Self::Graceful => None,
            Self::AuthenticationFailed => Some(ClientError::AuthenticationFailed),
            Self::Reconnectable(reason) => Some(ClientError::ConnectionLost(reason.clone())),
            Self::Fatal(reason) => Some(ClientError::SessionFailed(reason.clone())),
        }
    }
}

fn session_error_is_reconnectable(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::WebSocket(_)
            | ClientError::ConnectionClosed { .. }
            | ClientError::ConnectionLost(_)
            | ClientError::EventSequenceGap { .. }
            | ClientError::Timeout { .. }
            | ClientError::NotConnected
    )
}

/// A cloneable handle to one WebSocket session driver.
///
/// All clones share one socket, one reader loop, and one request routing table.
#[derive(Clone)]
pub struct CorbitConnection {
    commands: mpsc::Sender<SessionCommand>,
    cancellations: mpsc::Sender<String>,
    session_end: watch::Receiver<Option<SessionEnd>>,
    session_id: Arc<str>,
    server_info: Arc<ServerInfo>,
    rpc_timeout: Duration,
}

impl CorbitConnection {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }

    /// Performs an application-level heartbeat round trip.
    ///
    /// # Errors
    ///
    /// Returns an error if the ping cannot be queued or the matching pong is not
    /// received before the RPC timeout.
    pub async fn ping(&self) -> Result<(), ClientError> {
        let id = format!("ping_{}", uuid::Uuid::new_v4());
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(SessionCommand::Ping {
                id: id.clone(),
                response,
            })
            .await
            .map_err(|_| ClientError::NotConnected)?;

        let mut cancellation = RequestCancellation::new(id, self.cancellations.clone());
        let result = timeout(self.rpc_timeout, receiver)
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "waiting for pong",
            })?
            .map_err(|_| ClientError::NotConnected)?;
        cancellation.disarm();
        result
    }

    /// Calls the diagnostic `system.echo` RPC.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the Daemon reports an RPC error.
    pub async fn echo(&self, value: Value) -> Result<Value, ClientError> {
        self.rpc("system.echo", Some(value)).await
    }

    /// Fetches the versioned Project, Workspace, and Agent state owned by the Daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its result does not match the shared
    /// snapshot protocol model.
    pub async fn snapshot(&self) -> Result<AuthoritativeSnapshot, ClientError> {
        let value = self.rpc("state.snapshot", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Loads the installed Provider catalog, including live model capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or the catalog response is malformed.
    pub async fn provider_catalog(&self) -> Result<ProviderCatalog, ClientError> {
        let value = self.rpc("provider.catalog.list", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists all active and paused scheduled tasks.
    pub async fn scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, ClientError> {
        let value = self.rpc("schedule.list", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Creates one unattended scheduled task.
    pub async fn create_scheduled_task(
        &self,
        input: ScheduledTaskCreateInput,
    ) -> Result<ScheduledTask, ClientError> {
        let value = self
            .rpc("schedule.create", Some(serde_json::to_value(input)?))
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Updates one scheduled task.
    pub async fn update_scheduled_task(
        &self,
        input: ScheduledTaskUpdateInput,
    ) -> Result<ScheduledTask, ClientError> {
        let value = self
            .rpc("schedule.update", Some(serde_json::to_value(input)?))
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Pauses or resumes one scheduled task.
    pub async fn set_scheduled_task_paused(
        &self,
        task_id: &str,
        paused: bool,
    ) -> Result<ScheduledTask, ClientError> {
        let value = self
            .rpc(
                if paused {
                    "schedule.pause"
                } else {
                    "schedule.resume"
                },
                Some(serde_json::json!({ "taskId": task_id })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Deletes one scheduled task.
    pub async fn delete_scheduled_task(
        &self,
        task_id: &str,
    ) -> Result<ScheduledTaskDeleteAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "schedule.delete",
                Some(serde_json::json!({ "taskId": task_id })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Starts one scheduled task immediately.
    pub async fn run_scheduled_task_now(&self, task_id: &str) -> Result<ScheduledRun, ClientError> {
        let value = self
            .rpc(
                "schedule.runNow",
                Some(serde_json::json!({ "taskId": task_id })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists recent scheduled task runs.
    pub async fn scheduled_runs(
        &self,
        task_id: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<ScheduledRun>, ClientError> {
        let mut params = serde_json::Map::new();
        if let Some(task_id) = task_id {
            params.insert("taskId".into(), Value::String(task_id.into()));
        }
        if let Some(limit) = limit {
            params.insert("limit".into(), Value::from(limit));
        }
        let value = self
            .rpc("schedule.run.list", Some(Value::Object(params)))
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists one directory relative to the selected workspace root.
    ///
    /// # Errors
    ///
    /// Returns an error if the Daemon rejects the path, cannot access the
    /// workspace, or returns a malformed directory listing.
    pub async fn list_workspace_files(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<WorkspaceDirectoryListing, ClientError> {
        let value = self
            .rpc(
                "workspace.files.list",
                Some(serde_json::json!({
                    "workspaceId": workspace_id,
                    "path": path,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Reads a UTF-8 text file relative to the selected workspace root.
    ///
    /// # Errors
    ///
    /// Returns an error if the Daemon rejects the path or file type, cannot
    /// access the file, or returns malformed content.
    pub async fn read_workspace_file(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<WorkspaceFileContent, ClientError> {
        let value = self
            .rpc(
                "workspace.file.read",
                Some(serde_json::json!({
                    "workspaceId": workspace_id,
                    "path": path,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Loads read-only Git status scoped to the selected workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if Git cannot inspect the workspace or the Daemon
    /// returns a malformed status result.
    pub async fn workspace_git_status(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceGitStatus, ClientError> {
        let value = self
            .rpc(
                "workspace.git.status",
                Some(serde_json::json!({ "workspaceId": workspace_id })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Loads a unified diff for one changed workspace path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or unchanged, Git cannot produce
    /// the preview, or the response does not match the protocol model.
    pub async fn workspace_git_diff(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<WorkspaceGitDiff, ClientError> {
        let value = self
            .rpc(
                "workspace.git.diff",
                Some(serde_json::json!({
                    "workspaceId": workspace_id,
                    "path": path,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Applies one idempotent Project, Workspace, or Agent mutation.
    ///
    /// The caller owns `clientMutationId` in `params` and must reuse it when
    /// retrying an outcome that may have committed before the connection failed.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn mutate_resource(
        &self,
        method: &str,
        params: Value,
    ) -> Result<ResourceMutationAcknowledgement, ClientError> {
        let value = self.rpc(method, Some(params)).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Submits a prompt to a running Agent and returns the accepted Provider turn.
    ///
    /// The caller must reuse `client_mutation_id` when retrying an ambiguous
    /// result. Output arrives independently through [`ConnectionEvent::AgentTimeline`].
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn prompt(
        &self,
        agent_id: &str,
        text: &str,
        client_mutation_id: &str,
    ) -> Result<AgentPromptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.prompt",
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "text": text,
                    "clientMutationId": client_mutation_id,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Submits a prompt with per-turn Provider configuration and attachments.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn prompt_with_options(
        &self,
        agent_id: &str,
        text: &str,
        client_mutation_id: &str,
        options: AgentPromptOptions,
    ) -> Result<AgentPromptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.prompt",
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "text": text,
                    "clientMutationId": client_mutation_id,
                    "options": options,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Interrupts one active Agent turn.
    ///
    /// The caller must reuse `client_mutation_id` when retrying an ambiguous
    /// result. Completion arrives through [`ConnectionEvent::AgentTimeline`].
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn interrupt(
        &self,
        agent_id: &str,
        turn_id: &str,
        client_mutation_id: &str,
    ) -> Result<AgentInterruptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.interrupt",
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "turnId": turn_id,
                    "clientMutationId": client_mutation_id,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Steers one active Agent turn.
    ///
    /// The caller must reuse `client_mutation_id` when retrying an ambiguous result.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn steer(
        &self,
        agent_id: &str,
        turn_id: &str,
        text: &str,
        client_mutation_id: &str,
    ) -> Result<AgentSteerAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.steer",
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "turnId": turn_id,
                    "text": text,
                    "clientMutationId": client_mutation_id,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Resolves one pending Agent approval request.
    ///
    /// The caller must reuse `client_mutation_id` when retrying an ambiguous
    /// result. Resolution also arrives through [`ConnectionEvent::AgentPermission`].
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC fails or its acknowledgement is malformed.
    pub async fn resolve_approval(
        &self,
        agent_id: &str,
        approval_id: &str,
        decision: AgentApprovalDecision,
        client_mutation_id: &str,
    ) -> Result<AgentApprovalResolveAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.approval.resolve",
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "approvalId": approval_id,
                    "decision": decision,
                    "clientMutationId": client_mutation_id,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Sends an RPC request and waits for the response bearing the same ID.
    ///
    /// Multiple calls may execute concurrently on clones of this handle. Dropping
    /// the returned future cancels its local wait; a late response is discarded.
    ///
    /// # Errors
    ///
    /// Returns an error for transport and protocol failures, Daemon RPC failures,
    /// or a request timeout.
    pub async fn rpc(&self, method: &str, params: Option<Value>) -> Result<Value, ClientError> {
        let id = format!("request_{}", uuid::Uuid::new_v4());
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(SessionCommand::Rpc {
                id: id.clone(),
                method: method.to_owned(),
                params,
                response,
            })
            .await
            .map_err(|_| ClientError::NotConnected)?;

        let mut cancellation = RequestCancellation::new(id, self.cancellations.clone());
        let result = timeout(self.rpc_timeout, receiver)
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "waiting for RPC response",
            })?
            .map_err(|_| ClientError::NotConnected)?;
        cancellation.disarm();
        result
    }

    /// Waits until the session driver detects a close or transport failure.
    ///
    /// # Errors
    ///
    /// Returns the stable reason reported by the session driver.
    pub async fn wait_closed(&self) -> Result<(), ClientError> {
        let mut session_end = self.session_end.clone();
        loop {
            if let Some(end) = session_end.borrow().clone() {
                return end.to_error().map_or(Ok(()), Err);
            }
            session_end
                .changed()
                .await
                .map_err(|_| ClientError::NotConnected)?;
        }
    }

    /// Requests a graceful close and waits for the session driver to finish.
    ///
    /// # Errors
    ///
    /// Returns an error if the session had already failed or the close frame could
    /// not be sent.
    pub async fn close(&self) -> Result<(), ClientError> {
        if self.commands.send(SessionCommand::Close).await.is_err() {
            return self.wait_closed().await;
        }
        self.wait_closed().await
    }
}

struct RequestCancellation {
    id: Option<String>,
    cancellations: mpsc::Sender<String>,
}

impl RequestCancellation {
    fn new(id: String, cancellations: mpsc::Sender<String>) -> Self {
        Self {
            id: Some(id),
            cancellations,
        }
    }

    fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for RequestCancellation {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _ = self.cancellations.try_send(id);
        }
    }
}

async fn run_session_driver(
    mut socket: Socket,
    mut commands: mpsc::Receiver<SessionCommand>,
    mut cancellations: mpsc::Receiver<String>,
    session_end: watch::Sender<Option<SessionEnd>>,
    events: broadcast::Sender<ConnectionEvent>,
    recovery: Arc<Mutex<RecoveryState>>,
    strict_event_sequence: bool,
) {
    let mut pending_pings = HashMap::new();
    let mut pending_rpcs = HashMap::new();
    let result = drive_session(
        &mut socket,
        &mut commands,
        &mut cancellations,
        &mut pending_pings,
        &mut pending_rpcs,
        &events,
        &recovery,
        strict_event_sequence,
    )
    .await;
    let end = SessionEnd::from_result(&result);

    fail_pending(&mut pending_pings, &mut pending_rpcs, &end);
    let _ = events.send(ConnectionEvent::StateChanged(match end {
        SessionEnd::AuthenticationFailed => ConnectionState::AuthenticationFailed,
        SessionEnd::Graceful | SessionEnd::Reconnectable(_) | SessionEnd::Fatal(_) => {
            ConnectionState::Offline
        }
    }));
    let _ = session_end.send(Some(end));
}

#[allow(clippy::too_many_arguments)]
async fn drive_session(
    socket: &mut Socket,
    commands: &mut mpsc::Receiver<SessionCommand>,
    cancellations: &mut mpsc::Receiver<String>,
    pending_pings: &mut HashMap<String, oneshot::Sender<Result<(), ClientError>>>,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, ClientError>>>,
    events: &broadcast::Sender<ConnectionEvent>,
    recovery: &Arc<Mutex<RecoveryState>>,
    strict_event_sequence: bool,
) -> Result<(), ClientError> {
    loop {
        tokio::select! {
            biased;
            cancellation = cancellations.recv() => {
                if let Some(id) = cancellation {
                    pending_pings.remove(&id);
                    pending_rpcs.remove(&id);
                }
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    socket.close(None).await?;
                    return Ok(());
                };
                match command {
                    SessionCommand::Ping { id, response } => {
                        if response.is_closed() {
                            continue;
                        }
                        send_json(socket, &ClientMessage::ping(&id)).await?;
                        if !response.is_closed() {
                            pending_pings.insert(id, response);
                        }
                    }
                    SessionCommand::Rpc { id, method, params, response } => {
                        if response.is_closed() {
                            continue;
                        }
                        send_json(
                            socket,
                            &ClientMessage::rpc_request(&id, method, params),
                        )
                        .await?;
                        if !response.is_closed() {
                            pending_rpcs.insert(id, response);
                        }
                    }
                    SessionCommand::Close => {
                        socket.close(None).await?;
                        return Ok(());
                    }
                }
            }
            frame = socket.next() => {
                let frame = frame.ok_or_else(|| ClientError::ConnectionClosed {
                    code: None,
                    reason: String::new(),
                })??;
                handle_frame(
                    socket,
                    frame,
                    pending_pings,
                    pending_rpcs,
                    events,
                    recovery,
                    strict_event_sequence,
                )
                .await?;
            }
        }
    }
}

async fn handle_frame(
    socket: &mut Socket,
    frame: Message,
    pending_pings: &mut HashMap<String, oneshot::Sender<Result<(), ClientError>>>,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, ClientError>>>,
    events: &broadcast::Sender<ConnectionEvent>,
    recovery: &Arc<Mutex<RecoveryState>>,
    strict_event_sequence: bool,
) -> Result<(), ClientError> {
    match frame {
        Message::Text(text) => {
            let message: ServerMessage = serde_json::from_str(text.as_ref())?;
            route_server_message(
                message,
                pending_pings,
                pending_rpcs,
                events,
                recovery,
                strict_event_sequence,
            )
        }
        Message::Ping(payload) => {
            socket.send(Message::Pong(payload)).await?;
            Ok(())
        }
        Message::Pong(_) | Message::Frame(_) => Ok(()),
        Message::Close(frame) => {
            let (code, reason) = frame.map_or((None, String::new()), |frame| {
                (Some(u16::from(frame.code)), frame.reason.to_string())
            });
            if code == Some(4_408) {
                return Err(ClientError::AuthenticationFailed);
            }
            Err(ClientError::ConnectionClosed { code, reason })
        }
        Message::Binary(_) => Err(ClientError::UnexpectedMessage {
            operation: "reading the session",
        }),
    }
}

fn route_server_message(
    message: ServerMessage,
    pending_pings: &mut HashMap<String, oneshot::Sender<Result<(), ClientError>>>,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, ClientError>>>,
    events: &broadcast::Sender<ConnectionEvent>,
    recovery: &Arc<Mutex<RecoveryState>>,
    strict_event_sequence: bool,
) -> Result<(), ClientError> {
    match message {
        ServerMessage::Pong { ping_id, .. } => {
            if let Some(response) = pending_pings.remove(&ping_id) {
                let _ = response.send(Ok(()));
            }
            Ok(())
        }
        ServerMessage::RpcResponse {
            id,
            ok,
            result,
            error,
            ..
        } => {
            let Some(response) = pending_rpcs.remove(&id) else {
                return Ok(());
            };
            let result = if ok {
                Ok(result.unwrap_or(Value::Null))
            } else {
                Err(ClientError::Rpc(error.unwrap_or_else(|| {
                    corbit_protocol::ProtocolErrorBody {
                        code: "invalid_error_response".into(),
                        message: "Daemon returned an unsuccessful RPC response without an error"
                            .into(),
                        details: None,
                    }
                })))
            };
            let _ = response.send(result);
            Ok(())
        }
        ServerMessage::ProtocolError { error, .. } => Err(ClientError::Protocol(error)),
        ServerMessage::WorkspaceChanged(change) => {
            let _ = events.send(ConnectionEvent::WorkspaceChanged(change));
            Ok(())
        }
        ServerMessage::Event {
            topic,
            sequence,
            payload,
            ..
        } => route_event(
            &topic,
            sequence,
            payload,
            events,
            recovery,
            strict_event_sequence,
        ),
        ServerMessage::ServerInfo { .. } | ServerMessage::EventSync { .. } => {
            Err(ClientError::UnexpectedMessage {
                operation: "reading the session",
            })
        }
    }
}

enum RoutedEvent {
    Timeline(AgentTimelinePayload),
    Permission(AgentPermissionPayload),
    Other,
}

fn route_event(
    topic: &str,
    sequence: u64,
    payload: Value,
    events: &broadcast::Sender<ConnectionEvent>,
    recovery: &Arc<Mutex<RecoveryState>>,
    strict_event_sequence: bool,
) -> Result<(), ClientError> {
    let routed = match topic {
        "agent.timeline" => RoutedEvent::Timeline(serde_json::from_value(payload)?),
        "agent.permission" => RoutedEvent::Permission(serde_json::from_value(payload)?),
        _ => RoutedEvent::Other,
    };

    let mut state = lock_recovery(recovery);
    if sequence <= state.last_sequence {
        return Ok(());
    }
    if strict_event_sequence {
        let expected = state.last_sequence.saturating_add(1);
        if sequence != expected {
            return Err(ClientError::EventSequenceGap {
                expected,
                actual: sequence,
            });
        }
    }
    state.last_sequence = sequence;
    drop(state);

    match routed {
        RoutedEvent::Timeline(payload) => {
            let _ = events.send(ConnectionEvent::AgentTimeline { sequence, payload });
        }
        RoutedEvent::Permission(payload) => {
            let _ = events.send(ConnectionEvent::AgentPermission { sequence, payload });
        }
        RoutedEvent::Other => {}
    }
    Ok(())
}

fn fail_pending(
    pending_pings: &mut HashMap<String, oneshot::Sender<Result<(), ClientError>>>,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, ClientError>>>,
    end: &SessionEnd,
) {
    for (_, response) in pending_pings.drain() {
        let _ = response.send(Err(end.to_error().unwrap_or(ClientError::NotConnected)));
    }
    for (_, response) in pending_rpcs.drain() {
        let _ = response.send(Err(end.to_error().unwrap_or(ClientError::NotConnected)));
    }
}

async fn send_json(socket: &mut Socket, message: &ClientMessage) -> Result<(), ClientError> {
    socket
        .send(Message::text(serde_json::to_string(message)?))
        .await?;
    Ok(())
}

async fn next_server_message(
    socket: &mut Socket,
    duration: Duration,
    operation: &'static str,
) -> Result<ServerMessage, ClientError> {
    loop {
        let frame = timeout(duration, socket.next())
            .await
            .map_err(|_| ClientError::Timeout { operation })?
            .ok_or_else(|| ClientError::ConnectionClosed {
                code: None,
                reason: String::new(),
            })??;
        match frame {
            Message::Text(text) => return serde_json::from_str(text.as_ref()).map_err(Into::into),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
            Message::Pong(_) | Message::Frame(_) => {}
            Message::Close(frame) => {
                let (code, reason) = frame.map_or((None, String::new()), |frame| {
                    (Some(u16::from(frame.code)), frame.reason.to_string())
                });
                if code == Some(4_408) {
                    return Err(ClientError::AuthenticationFailed);
                }
                return Err(ClientError::ConnectionClosed { code, reason });
            }
            Message::Binary(_) => {
                return Err(ClientError::UnexpectedMessage { operation });
            }
        }
    }
}
