use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use async_channel::{Receiver, Sender};
use corbit_protocol::{
    AgentApprovalDecision, AgentApprovalResolveAcknowledgement, AgentInterruptAcknowledgement,
    AgentPromptAcknowledgement, AgentPromptOptions, AuthoritativeSnapshot, DeviceCredentialSummary,
    PairingOffer, PluginAuditEntry, PluginCommandResult, PluginInspection, PluginMarketplaceEntry,
    PluginRecord, ProviderCatalog, ResourceMutationAcknowledgement, WorkspaceDirectoryListing,
    WorkspaceFileContent, WorkspaceGitDiff, WorkspaceGitStatus,
};
use serde_json::Value;
use tokio::{
    sync::{Semaphore, broadcast, oneshot},
    time::{MissedTickBehavior, interval, sleep},
};

use crate::{ClientConfig, ClientError, ConnectionEvent, ConnectionState, CorbitClient};

const EVENT_CAPACITY: usize = 512;

/// Events emitted by the dedicated Tokio runtime for consumption by a UI runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeEvent {
    HealthChecked,
    Connection(ConnectionEvent),
    Snapshot(AuthoritativeSnapshot),
    Error(String),
}

/// Owns a dedicated Tokio runtime thread and its bounded event channel.
///
/// Dropping this handle requests a graceful WebSocket shutdown. It never joins the
/// network thread on the caller because UI teardown must remain non-blocking.
pub struct DaemonRuntime {
    events: Receiver<RuntimeEvent>,
    commands: Sender<RuntimeCommand>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// Cloneable command handle for UI tasks that must not own the runtime thread.
#[derive(Clone)]
pub struct DaemonRuntimeClient {
    commands: Sender<RuntimeCommand>,
}

impl DaemonRuntime {
    /// Starts a dedicated, automatically reconnecting network thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the Tokio runtime or its operating-system thread cannot
    /// be created.
    pub fn spawn(config: ClientConfig) -> Result<Self, ClientError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| ClientError::RuntimeStart(error.to_string()))?;
        let (event_sender, events) = async_channel::bounded(EVENT_CAPACITY);
        let (command_sender, commands) = async_channel::bounded(EVENT_CAPACITY);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("corbit-daemon-client".into())
            .spawn(move || {
                runtime.block_on(run(config, event_sender, commands, shutdown_receiver));
            })
            .map_err(|error| ClientError::RuntimeStart(error.to_string()))?;

        Ok(Self {
            events,
            commands: command_sender,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn events(&self) -> Receiver<RuntimeEvent> {
        self.events.clone()
    }

    pub fn client(&self) -> DaemonRuntimeClient {
        DaemonRuntimeClient {
            commands: self.commands.clone(),
        }
    }

    /// Executes an RPC on the currently active session without requiring a Tokio
    /// runtime on the caller thread.
    ///
    /// Commands received while reconnecting fail with [`ClientError::NotConnected`]
    /// so mutations are never replayed implicitly.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active, the request times out, or the
    /// Daemon rejects the RPC.
    pub async fn rpc(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<Value, ClientError> {
        self.client().rpc(method, params).await
    }
}

impl DaemonRuntimeClient {
    /// Executes an RPC on the currently active runtime session.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline or the request fails.
    pub async fn rpc(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<Value, ClientError> {
        let (response, result) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::Rpc {
                method: method.into(),
                params,
                response,
            })
            .await
            .map_err(|_| ClientError::NotConnected)?;
        result.recv().await.map_err(|_| ClientError::NotConnected)?
    }

    /// Loads the installed Provider catalog and its live model capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the Daemon rejects the request, or
    /// when the response cannot be decoded.
    pub async fn provider_catalog(&self) -> Result<ProviderCatalog, ClientError> {
        let value = self.rpc("provider.catalog.list", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists installed built-in and local plugins.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline or the response is invalid.
    pub async fn plugins(&self) -> Result<Vec<PluginRecord>, ClientError> {
        let value = self.rpc("plugin.list", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists the Daemon-provided plugin marketplace entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline or the response is invalid.
    pub async fn plugin_marketplace(&self) -> Result<Vec<PluginMarketplaceEntry>, ClientError> {
        let value = self.rpc("plugin.marketplace.list", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists recent redacted plugin command audit entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline, the credential is not the
    /// local root credential, or the response is invalid.
    pub async fn plugin_audit(
        &self,
        limit: Option<u32>,
    ) -> Result<Vec<PluginAuditEntry>, ClientError> {
        let params = limit.map(|limit| serde_json::json!({ "limit": limit }));
        let value = self.rpc("plugin.audit.list", params).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Installs a local plugin directory containing `manifest.json`.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline, the package is invalid, or
    /// the Daemon rejects the operation.
    pub async fn install_plugin(
        &self,
        path: impl Into<String>,
    ) -> Result<PluginRecord, ClientError> {
        let value = self
            .rpc(
                "plugin.install",
                Some(serde_json::json!({ "path": path.into() })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Performs a complete local plugin validation and returns a short-lived,
    /// daemon-owned confirmation token without exposing the local path.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the package is invalid, or when the
    /// connected Daemon does not support plugin inspection.
    pub async fn inspect_plugin(
        &self,
        path: impl Into<String>,
    ) -> Result<PluginInspection, ClientError> {
        let value = self
            .rpc(
                "plugin.inspect",
                Some(serde_json::json!({ "path": path.into() })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Installs the exact plugin source represented by a short-lived inspection token.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the token expired, or when the local
    /// source changed after inspection.
    pub async fn install_inspected_plugin(
        &self,
        inspection_id: impl Into<String>,
    ) -> Result<PluginRecord, ClientError> {
        let value = self
            .rpc(
                "plugin.install",
                Some(serde_json::json!({
                    "inspectionId": inspection_id.into()
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Installs a verified plugin from the Daemon's signed marketplace index.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline, the marketplace is unavailable,
    /// or the Daemon rejects the signature, integrity, or package validation.
    pub async fn install_marketplace_plugin(
        &self,
        plugin_id: impl Into<String>,
        version: Option<String>,
    ) -> Result<PluginRecord, ClientError> {
        let mut params = serde_json::json!({ "pluginId": plugin_id.into() });
        if let Some(version) = version {
            params["version"] = serde_json::Value::String(version);
        }
        let value = self.rpc("plugin.marketplace.install", Some(params)).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Enables or disables one installed plugin.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline or the Daemon rejects the operation.
    pub async fn set_plugin_enabled(
        &self,
        plugin_id: impl Into<String>,
        enabled: bool,
    ) -> Result<PluginRecord, ClientError> {
        let method = if enabled {
            "plugin.enable"
        } else {
            "plugin.disable"
        };
        let value = self
            .rpc(
                method,
                Some(serde_json::json!({ "pluginId": plugin_id.into() })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Uninstalls one local plugin.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline or the Daemon rejects the operation.
    pub async fn uninstall_plugin(&self, plugin_id: impl Into<String>) -> Result<(), ClientError> {
        self.rpc(
            "plugin.uninstall",
            Some(serde_json::json!({ "pluginId": plugin_id.into() })),
        )
        .await
        .map(|_| ())
    }

    /// Invokes a command exposed by an enabled plugin.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is offline, the command is unavailable,
    /// or the Daemon rejects the plugin process execution.
    pub async fn execute_plugin_command(
        &self,
        plugin_id: impl Into<String>,
        command_id: impl Into<String>,
        workspace_id: Option<String>,
        allow_workspace_write: bool,
    ) -> Result<PluginCommandResult, ClientError> {
        let mut params = serde_json::json!({
            "pluginId": plugin_id.into(),
            "commandId": command_id.into(),
        });
        if let Some(workspace_id) = workspace_id {
            params["workspaceId"] = serde_json::Value::String(workspace_id);
        }
        if allow_workspace_write {
            params["approvedPermissions"] = serde_json::json!(["workspace.write"]);
        }
        let value = self.rpc("plugin.invoke", Some(params)).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Lists credentials paired through the Daemon root token.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the runtime has stopped.
    pub async fn devices(&self) -> Result<Vec<DeviceCredentialSummary>, ClientError> {
        let (response, result) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::ListDevices { response })
            .await
            .map_err(|_| ClientError::NotConnected)?;
        result.recv().await.map_err(|_| ClientError::NotConnected)?
    }

    /// Creates a single-use device pairing offer through the Daemon root token.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, HTTP transport, or runtime dispatch fails.
    pub async fn create_pairing(
        &self,
        endpoint: impl Into<String>,
        host_name: impl Into<String>,
    ) -> Result<PairingOffer, ClientError> {
        let (response, result) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::CreatePairing {
                endpoint: endpoint.into(),
                host_name: host_name.into(),
                response,
            })
            .await
            .map_err(|_| ClientError::NotConnected)?;
        result.recv().await.map_err(|_| ClientError::NotConnected)?
    }

    /// Revokes a paired device credential through the Daemon root token.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request or runtime dispatch fails.
    pub async fn revoke_device(&self, device_id: impl Into<String>) -> Result<(), ClientError> {
        let (response, result) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::RevokeDevice {
                device_id: device_id.into(),
                response,
            })
            .await
            .map_err(|_| ClientError::NotConnected)?;
        result.recv().await.map_err(|_| ClientError::NotConnected)?
    }

    /// Applies one idempotent mutation and then fetches the authoritative state.
    ///
    /// A mutation is never replayed automatically. If the result is ambiguous,
    /// callers can safely retry with the same `clientMutationId`.
    ///
    /// # Errors
    ///
    /// Returns an error when either the mutation or follow-up snapshot fails, or
    /// when either result does not match the protocol model.
    pub async fn mutate_and_snapshot(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<(ResourceMutationAcknowledgement, AuthoritativeSnapshot), ClientError> {
        let acknowledgement = self.rpc(method, Some(params)).await?;
        let acknowledgement = serde_json::from_value(acknowledgement)?;
        let snapshot = self.rpc("state.snapshot", None).await?;
        let snapshot = serde_json::from_value(snapshot)?;
        Ok((acknowledgement, snapshot))
    }

    /// Lists one directory relative to a workspace root through the active session.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the Daemon rejects the path, or when
    /// the result does not match the shared protocol model.
    pub async fn list_workspace_files(
        &self,
        workspace_id: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<WorkspaceDirectoryListing, ClientError> {
        let value = self
            .rpc(
                "workspace.files.list",
                Some(serde_json::json!({
                    "workspaceId": workspace_id.into(),
                    "path": path.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Reads one UTF-8 text file relative to a workspace root.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the Daemon rejects the file, or when
    /// the result does not match the shared protocol model.
    pub async fn read_workspace_file(
        &self,
        workspace_id: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<WorkspaceFileContent, ClientError> {
        let value = self
            .rpc(
                "workspace.file.read",
                Some(serde_json::json!({
                    "workspaceId": workspace_id.into(),
                    "path": path.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Loads read-only Git status for one workspace through the active session.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when Git inspection fails, or when the
    /// result does not match the shared protocol model.
    pub async fn workspace_git_status(
        &self,
        workspace_id: impl Into<String>,
    ) -> Result<WorkspaceGitStatus, ClientError> {
        let value = self
            .rpc(
                "workspace.git.status",
                Some(serde_json::json!({ "workspaceId": workspace_id.into() })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Loads a unified diff for one changed workspace path.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, when the Daemon rejects the path or Git
    /// operation, or when the result cannot be decoded.
    pub async fn workspace_git_diff(
        &self,
        workspace_id: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<WorkspaceGitDiff, ClientError> {
        let value = self
            .rpc(
                "workspace.git.diff",
                Some(serde_json::json!({
                    "workspaceId": workspace_id.into(),
                    "path": path.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Submits a prompt to a running Agent through the active runtime session.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, rejected by the Daemon, or when the
    /// acknowledgement does not match the shared protocol model.
    pub async fn prompt(
        &self,
        agent_id: impl Into<String>,
        text: impl Into<String>,
        client_mutation_id: impl Into<String>,
    ) -> Result<AgentPromptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.prompt",
                Some(serde_json::json!({
                    "agentId": agent_id.into(),
                    "text": text.into(),
                    "clientMutationId": client_mutation_id.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Submits a prompt with per-turn Provider configuration and attachments.
    ///
    /// # Errors
    ///
    /// Returns an error when offline, rejected by the Daemon, or when the
    /// acknowledgement does not match the shared protocol model.
    pub async fn prompt_with_options(
        &self,
        agent_id: impl Into<String>,
        text: impl Into<String>,
        client_mutation_id: impl Into<String>,
        options: AgentPromptOptions,
    ) -> Result<AgentPromptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.prompt",
                Some(serde_json::json!({
                    "agentId": agent_id.into(),
                    "text": text.into(),
                    "clientMutationId": client_mutation_id.into(),
                    "options": options,
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Interrupts one active Agent turn through the active runtime session.
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime is offline, the Daemon rejects the
    /// request, or the acknowledgement cannot be decoded.
    pub async fn interrupt(
        &self,
        agent_id: impl Into<String>,
        turn_id: impl Into<String>,
        client_mutation_id: impl Into<String>,
    ) -> Result<AgentInterruptAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.interrupt",
                Some(serde_json::json!({
                    "agentId": agent_id.into(),
                    "turnId": turn_id.into(),
                    "clientMutationId": client_mutation_id.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Resolves one pending Agent approval through the active runtime session.
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime is offline, the Daemon rejects the
    /// request, or the acknowledgement cannot be decoded.
    pub async fn resolve_approval(
        &self,
        agent_id: impl Into<String>,
        approval_id: impl Into<String>,
        decision: AgentApprovalDecision,
        client_mutation_id: impl Into<String>,
    ) -> Result<AgentApprovalResolveAcknowledgement, ClientError> {
        let value = self
            .rpc(
                "agent.approval.resolve",
                Some(serde_json::json!({
                    "agentId": agent_id.into(),
                    "approvalId": approval_id.into(),
                    "decision": decision,
                    "clientMutationId": client_mutation_id.into(),
                })),
            )
            .await?;
        serde_json::from_value(value).map_err(Into::into)
    }
}

impl Drop for DaemonRuntime {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            drop(thread);
        }
    }
}

async fn run(
    config: ClientConfig,
    events: Sender<RuntimeEvent>,
    commands: Receiver<RuntimeCommand>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let client = match CorbitClient::new(config) {
        Ok(client) => client,
        Err(error) => {
            send_error(&events, error).await;
            return;
        }
    };
    let mut client_events = client.subscribe();
    let (active_session, active_session_receiver) = tokio::sync::watch::channel(None);
    let dispatcher = tokio::spawn(dispatch_commands(
        commands,
        active_session_receiver,
        client.clone(),
    ));
    let forwarded_events = events.clone();
    let forwarder = tokio::spawn(async move {
        loop {
            match client_events.recv().await {
                Ok(event) => {
                    if forwarded_events
                        .send(RuntimeEvent::Connection(event))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let message = format!("daemon client event channel skipped {skipped} events");
                    if forwarded_events
                        .send(RuntimeEvent::Error(message))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let result = supervise(&client, &events, &active_session, &mut shutdown).await;
    let _ = active_session.send(None);
    dispatcher.abort();
    let _ = dispatcher.await;
    drop(client);
    let _ = forwarder.await;

    if let Err(error) = result {
        send_error(&events, error).await;
    }
}

enum AttemptOutcome {
    Shutdown,
    ConnectFailed(ClientError),
    SessionFailed(ClientError),
}

enum RuntimeCommand {
    Rpc {
        method: String,
        params: Option<Value>,
        response: Sender<Result<Value, ClientError>>,
    },
    ListDevices {
        response: Sender<Result<Vec<DeviceCredentialSummary>, ClientError>>,
    },
    CreatePairing {
        endpoint: String,
        host_name: String,
        response: Sender<Result<PairingOffer, ClientError>>,
    },
    RevokeDevice {
        device_id: String,
        response: Sender<Result<(), ClientError>>,
    },
}

async fn dispatch_commands(
    commands: Receiver<RuntimeCommand>,
    active_session: tokio::sync::watch::Receiver<Option<crate::CorbitConnection>>,
    client: CorbitClient,
) {
    let slots = Arc::new(Semaphore::new(EVENT_CAPACITY));
    loop {
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            break;
        };
        let Ok(command) = commands.recv().await else {
            break;
        };
        match command {
            RuntimeCommand::Rpc {
                method,
                params,
                response,
            } => {
                let connection = active_session.borrow().clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    if let Some(connection) = connection {
                        let result = tokio::select! {
                            result = connection.rpc(&method, params) => Some(result),
                            () = response.closed() => None,
                        };
                        if let Some(result) = result {
                            let _ = response.send(result).await;
                        }
                    } else {
                        let _ = response.send(Err(ClientError::NotConnected)).await;
                    }
                });
            }
            RuntimeCommand::ListDevices { response } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    let result = tokio::select! {
                        result = client.devices() => Some(result),
                        () = response.closed() => None,
                    };
                    if let Some(result) = result {
                        let _ = response.send(result).await;
                    }
                });
            }
            RuntimeCommand::CreatePairing {
                endpoint,
                host_name,
                response,
            } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    let result = tokio::select! {
                        result = client.create_pairing(endpoint, host_name) => Some(result),
                        () = response.closed() => None,
                    };
                    if let Some(result) = result {
                        let _ = response.send(result).await;
                    }
                });
            }
            RuntimeCommand::RevokeDevice {
                device_id,
                response,
            } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    let result = tokio::select! {
                        result = client.revoke_device(&device_id) => Some(result),
                        () = response.closed() => None,
                    };
                    if let Some(result) = result {
                        let _ = response.send(result).await;
                    }
                });
            }
        }
    }
}

async fn supervise(
    client: &CorbitClient,
    events: &Sender<RuntimeEvent>,
    active_session: &tokio::sync::watch::Sender<Option<crate::CorbitConnection>>,
    shutdown: &mut oneshot::Receiver<()>,
) -> Result<(), ClientError> {
    let mut reconnect_attempt = 1_u32;

    loop {
        match run_attempt(client, events, active_session, shutdown).await {
            AttemptOutcome::Shutdown => return Ok(()),
            AttemptOutcome::ConnectFailed(error) | AttemptOutcome::SessionFailed(error)
                if !is_reconnectable(&error) =>
            {
                return Err(error);
            }
            AttemptOutcome::SessionFailed(error) => {
                reconnect_attempt = 1;
                let delay = reconnect_delay(
                    client.reconnect_initial_delay(),
                    client.reconnect_max_delay(),
                    reconnect_attempt,
                );
                client.emit_state(ConnectionState::Reconnecting {
                    attempt: reconnect_attempt,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    reason: error.to_string(),
                });
                reconnect_attempt = reconnect_attempt.saturating_add(1);

                tokio::select! {
                    _ = &mut *shutdown => return Ok(()),
                    () = sleep(delay) => {}
                }
            }
            AttemptOutcome::ConnectFailed(error) => {
                let delay = reconnect_delay(
                    client.reconnect_initial_delay(),
                    client.reconnect_max_delay(),
                    reconnect_attempt,
                );
                client.emit_state(ConnectionState::Reconnecting {
                    attempt: reconnect_attempt,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    reason: error.to_string(),
                });
                reconnect_attempt = reconnect_attempt.saturating_add(1);

                tokio::select! {
                    _ = &mut *shutdown => return Ok(()),
                    () = sleep(delay) => {}
                }
            }
        }
    }
}

async fn run_attempt(
    client: &CorbitClient,
    events: &Sender<RuntimeEvent>,
    active_session: &tokio::sync::watch::Sender<Option<crate::CorbitConnection>>,
    shutdown: &mut oneshot::Receiver<()>,
) -> AttemptOutcome {
    let _ = active_session.send(None);
    let health = tokio::select! {
        _ = &mut *shutdown => return AttemptOutcome::Shutdown,
        result = client.health() => match result {
            Ok(health) => health,
            Err(error) => return AttemptOutcome::ConnectFailed(error),
        },
    };
    if health.status != "ok" {
        return AttemptOutcome::ConnectFailed(ClientError::Unhealthy(health.status));
    }
    if events.send(RuntimeEvent::HealthChecked).await.is_err() {
        return AttemptOutcome::Shutdown;
    }

    let connection = tokio::select! {
        _ = &mut *shutdown => return AttemptOutcome::Shutdown,
        result = client.connect_without_online_event() => match result {
            Ok(connection) => connection,
            Err(error) => return AttemptOutcome::ConnectFailed(error),
        },
    };
    client.announce_online(&connection);

    let snapshot = tokio::select! {
        _ = &mut *shutdown => {
            let _ = connection.close().await;
            return AttemptOutcome::Shutdown;
        },
        result = connection.snapshot() => match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = connection.close().await;
                return AttemptOutcome::SessionFailed(error);
            }
        },
    };
    // Publish the session before the snapshot event reaches the UI. This
    // keeps RPCs triggered by snapshot reconciliation (for example the live
    // Provider catalog request) from observing a transient `NotConnected`
    // state while the event is still being delivered.
    let _ = active_session.send(Some(connection.clone()));
    if events.send(RuntimeEvent::Snapshot(snapshot)).await.is_err() {
        let _ = active_session.send(None);
        let _ = connection.close().await;
        return AttemptOutcome::Shutdown;
    }

    let mut heartbeat = interval(client.heartbeat_interval());
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    heartbeat.tick().await;

    loop {
        tokio::select! {
            _ = &mut *shutdown => {
                let _ = active_session.send(None);
                let _ = connection.close().await;
                return AttemptOutcome::Shutdown;
            }
            result = connection.wait_closed() => {
                let _ = active_session.send(None);
                return AttemptOutcome::SessionFailed(
                    result.err().unwrap_or(ClientError::NotConnected),
                );
            }
            _ = heartbeat.tick() => {
                if let Err(error) = connection.ping().await {
                    let _ = active_session.send(None);
                    let _ = connection.close().await;
                    return AttemptOutcome::SessionFailed(error);
                }
            }
        }
    }
}

fn reconnect_delay(initial: Duration, maximum: Duration, attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(31);
    initial.saturating_mul(1_u32 << exponent).min(maximum)
}

fn is_reconnectable(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Http(_)
            | ClientError::WebSocket(_)
            | ClientError::ConnectionClosed { .. }
            | ClientError::ConnectionLost(_)
            | ClientError::EventSequenceGap { .. }
            | ClientError::Timeout { .. }
            | ClientError::Unhealthy(_)
            | ClientError::NotConnected
    )
}

async fn send_error(events: &Sender<RuntimeEvent>, error: ClientError) {
    let _ = events.send(RuntimeEvent::Error(error.to_string())).await;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::reconnect_delay;

    #[test]
    fn reconnect_delay_uses_capped_exponential_backoff() {
        let initial = Duration::from_millis(100);
        let maximum = Duration::from_millis(350);

        assert_eq!(reconnect_delay(initial, maximum, 1), initial);
        assert_eq!(
            reconnect_delay(initial, maximum, 2),
            Duration::from_millis(200)
        );
        assert_eq!(reconnect_delay(initial, maximum, 3), maximum);
        assert_eq!(reconnect_delay(initial, maximum, u32::MAX), maximum);
    }
}
