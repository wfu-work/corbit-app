//! Rust models for the versioned Corbit daemon wire protocol.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Desktop,
    Mobile,
    Cli,
    Test,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIdentity {
    pub id: String,
    pub kind: ClientKind,
    pub version: String,
    pub platform: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeCursor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u64>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "hello", rename_all = "camelCase")]
    Hello {
        protocol_version: u32,
        client: ClientIdentity,
        capabilities: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resume: Option<ResumeCursor>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "ping", rename_all = "camelCase")]
    Ping {
        ping_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        sent_at: Option<String>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "rpc.request", rename_all = "camelCase")]
    RpcRequest {
        id: String,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
}

impl ClientMessage {
    pub fn hello(client: ClientIdentity, capabilities: Vec<String>) -> Self {
        Self::hello_with_resume(client, capabilities, None)
    }

    pub fn hello_with_resume(
        client: ClientIdentity,
        capabilities: Vec<String>,
        resume: Option<ResumeCursor>,
    ) -> Self {
        Self::Hello {
            protocol_version: PROTOCOL_VERSION,
            client,
            capabilities,
            resume,
            extensions: BTreeMap::new(),
        }
    }

    pub fn ping(ping_id: impl Into<String>) -> Self {
        Self::Ping {
            ping_id: ping_id.into(),
            sent_at: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn rpc_request(
        id: impl Into<String>,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Self {
        Self::RpcRequest {
            id: id.into(),
            method: method.into(),
            params,
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSyncPhase {
    Begin,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentTurnStatus {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AgentTimelineStepStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "inProgress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "declined")]
    Declined,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTimelinePlanStep {
    pub text: String,
    pub status: AgentTimelineStepStatus,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentTimelineFileChangeKind {
    Added,
    Modified,
    Deleted,
    Moved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTimelineFileChange {
    pub path: String,
    pub change_kind: AgentTimelineFileChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum AgentTimelineEvent {
    #[serde(rename = "turn.started", rename_all = "camelCase")]
    TurnStarted {
        turn_id: String,
        prompt: String,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "assistant.delta", rename_all = "camelCase")]
    AssistantDelta {
        turn_id: String,
        item_id: String,
        delta: String,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "reasoning.delta", rename_all = "camelCase")]
    ReasoningDelta {
        turn_id: String,
        item_id: String,
        delta: String,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "plan.updated", rename_all = "camelCase")]
    PlanUpdated {
        turn_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        explanation: Option<String>,
        steps: Vec<AgentTimelinePlanStep>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "command.updated", rename_all = "camelCase")]
    CommandUpdated {
        turn_id: String,
        item_id: String,
        command: String,
        status: AgentTimelineStepStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "command.output.delta", rename_all = "camelCase")]
    CommandOutputDelta {
        turn_id: String,
        item_id: String,
        delta: String,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "file.change.updated", rename_all = "camelCase")]
    FileChangeUpdated {
        turn_id: String,
        item_id: String,
        status: AgentTimelineStepStatus,
        changes: Vec<AgentTimelineFileChange>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "tool.updated", rename_all = "camelCase")]
    ToolUpdated {
        turn_id: String,
        item_id: String,
        tool_name: String,
        status: AgentTimelineStepStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "turn.diff.updated", rename_all = "camelCase")]
    TurnDiffUpdated {
        turn_id: String,
        diff: String,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "turn.usage.updated", rename_all = "camelCase")]
    TurnUsageUpdated {
        turn_id: String,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cached_input_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_output_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_window: Option<u64>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "turn.completed", rename_all = "camelCase")]
    TurnCompleted {
        turn_id: String,
        status: AgentTurnStatus,
        occurred_at: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTimelinePayload {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub event: AgentTimelineEvent,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AgentApprovalDecision {
    #[serde(rename = "accept")]
    Accept,
    #[serde(rename = "acceptForSession")]
    AcceptForSession,
    #[serde(rename = "decline")]
    Decline,
    #[serde(rename = "cancel")]
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum AgentPermissionEvent {
    #[serde(rename = "permission.requested", rename_all = "camelCase")]
    PermissionRequested {
        approval_id: String,
        turn_id: String,
        item_id: String,
        permission_kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        grant_root: Option<String>,
        available_decisions: Vec<AgentApprovalDecision>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "permission.resolved", rename_all = "camelCase")]
    PermissionResolved {
        approval_id: String,
        turn_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        decision: Option<AgentApprovalDecision>,
        occurred_at: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionPayload {
    pub agent_id: String,
    pub event: AgentPermissionEvent,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChanged {
    pub workspace_id: String,
    pub paths: Vec<String>,
    pub occurred_at: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "server_info", rename_all = "camelCase")]
    ServerInfo {
        session_id: String,
        server_id: String,
        version: String,
        protocol_version: u32,
        features: BTreeMap<String, bool>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "event_sync", rename_all = "camelCase")]
    EventSync {
        phase: EventSyncPhase,
        requested_after: u64,
        replay_from: u64,
        latest_sequence: u64,
        reset: bool,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "pong", rename_all = "camelCase")]
    Pong {
        ping_id: String,
        server_time: String,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "rpc.response", rename_all = "camelCase")]
    RpcResponse {
        id: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ProtocolErrorBody>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "workspace.changed")]
    WorkspaceChanged(WorkspaceChanged),
    #[serde(rename = "event", rename_all = "camelCase")]
    Event {
        topic: String,
        sequence: u64,
        payload: Value,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
    #[serde(rename = "protocol.error", rename_all = "camelCase")]
    ProtocolError {
        error: ProtocolErrorBody,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub server_id: String,
    pub version: String,
    pub protocol_version: u32,
    pub features: BTreeMap<String, bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalog {
    pub providers: Vec<ProviderCatalogEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub provider_id: String,
    pub available: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
    pub models: Vec<ProviderModelInfo>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub default_reasoning_effort: Option<AgentReasoningEffort>,
    pub supported_reasoning_efforts: Vec<ProviderReasoningEffortInfo>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReasoningEffortInfo {
    pub reasoning_effort: AgentReasoningEffort,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredentialSummary {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceCredentialSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingOffer {
    pub pairing_id: String,
    pub pairing_uri: String,
    pub endpoint: String,
    pub server_id: String,
    pub host_name: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResource {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Active,
    Archived,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceResource {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub working_directory: String,
    pub status: WorkspaceStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceFileEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileEntry {
    pub name: String,
    pub path: String,
    pub kind: WorkspaceFileEntryKind,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectoryListing {
    pub workspace_id: String,
    pub path: String,
    pub entries: Vec<WorkspaceFileEntry>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileContent {
    pub workspace_id: String,
    pub path: String,
    pub content: String,
    pub byte_length: u64,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceGitChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflicted,
    Untracked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitChange {
    pub path: String,
    pub index_status: Option<WorkspaceGitChangeKind>,
    pub worktree_status: Option<WorkspaceGitChangeKind>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitStatus {
    pub workspace_id: String,
    pub is_repository: bool,
    pub branch: Option<String>,
    pub changes: Vec<WorkspaceGitChange>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitDiff {
    pub workspace_id: String,
    pub path: String,
    pub unified_diff: String,
    pub byte_length: u64,
    pub is_binary: bool,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Initializing,
    Idle,
    Running,
    Error,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResource {
    pub id: String,
    pub workspace_id: String,
    pub provider: String,
    pub title: String,
    pub status: AgentStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoritativeSnapshot {
    pub schema_version: u32,
    pub generated_at: String,
    pub revision: u64,
    pub projects: Vec<ProjectResource>,
    pub workspaces: Vec<WorkspaceResource>,
    pub agents: Vec<AgentResource>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMutationAcknowledgement {
    pub resource_id: String,
    pub revision: u64,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

/// Access policy applied to one Provider turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionMode {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

/// Requested reasoning effort for one Provider turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReasoningEffort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

/// One attachment uploaded with an Agent prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAttachment {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

/// Optional per-turn Provider configuration supplied with an Agent prompt.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<AgentPermissionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<AgentReasoningEffort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AgentPromptAttachment>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAcknowledgement {
    pub agent_id: String,
    pub turn_id: String,
    pub client_mutation_id: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterruptAcknowledgement {
    pub agent_id: String,
    pub turn_id: String,
    pub client_mutation_id: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalResolveAcknowledgement {
    pub agent_id: String,
    pub approval_id: String,
    pub decision: AgentApprovalDecision,
    pub client_mutation_id: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::{ClientMessage, ServerMessage};

    const FIXTURE_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../corbit-daemon/protocol/fixtures"
    );

    #[test]
    fn daemon_client_fixtures_parse_and_round_trip() {
        for relative_path in [
            "handshake/client-hello.json",
            "heartbeat/ping.json",
            "rpc/echo-request.json",
            "state/snapshot-request.json",
            "resources/project-create-request.json",
            "resources/workspace-create-request.json",
            "resources/agent-create-request.json",
            "resources/agent-start-request.json",
            "resources/agent-stop-request.json",
            "agents/prompt-request.json",
            "agents/prompt-options-request.json",
            "agents/permission-resolve-request.json",
            "agents/interrupt-request.json",
            "workspace/files-list-request.json",
            "workspace/file-read-request.json",
            "workspace/git-status-request.json",
            "workspace/git-diff-request.json",
        ] {
            let text = std::fs::read_to_string(format!("{FIXTURE_ROOT}/{relative_path}"))
                .expect("client fixture should be readable");
            let message: ClientMessage =
                serde_json::from_str(&text).expect("client fixture should match Rust protocol");
            let encoded = serde_json::to_value(message).expect("client message should serialize");
            let expected: serde_json::Value =
                serde_json::from_str(&text).expect("fixture should contain JSON");
            assert_eq!(encoded, expected, "fixture {relative_path}");
        }
    }

    #[test]
    fn daemon_server_fixtures_parse_and_round_trip() {
        for relative_path in [
            "handshake/server-info.json",
            "heartbeat/pong.json",
            "rpc/echo-response.json",
            "state/snapshot-response.json",
            "resources/project-create-response.json",
            "resources/workspace-create-response.json",
            "resources/agent-create-response.json",
            "resources/agent-start-response.json",
            "resources/agent-stop-response.json",
            "agents/prompt-response.json",
            "agents/turn-started.json",
            "agents/assistant-delta.json",
            "agents/reasoning-delta.json",
            "agents/plan-updated.json",
            "agents/command-updated.json",
            "agents/command-output-delta.json",
            "agents/file-change-updated.json",
            "agents/tool-updated.json",
            "agents/turn-diff-updated.json",
            "agents/turn-usage-updated.json",
            "agents/turn-completed.json",
            "agents/permission-requested.json",
            "agents/permission-resolve-response.json",
            "agents/permission-resolved.json",
            "agents/interrupt-response.json",
            "workspace/files-list-response.json",
            "workspace/file-read-response.json",
            "workspace/git-status-response.json",
            "workspace/git-diff-response.json",
            "workspace/workspace-changed.json",
        ] {
            let text = std::fs::read_to_string(format!("{FIXTURE_ROOT}/{relative_path}"))
                .expect("server fixture should be readable");
            let message: ServerMessage =
                serde_json::from_str(&text).expect("server fixture should match Rust protocol");
            let encoded = serde_json::to_value(message).expect("server message should serialize");
            let expected: serde_json::Value =
                serde_json::from_str(&text).expect("fixture should contain JSON");
            assert_eq!(encoded, expected, "fixture {relative_path}");
        }
    }

    #[test]
    fn additive_fields_are_preserved_for_compatibility() {
        let message: ServerMessage = serde_json::from_str(
            r#"{"type":"server_info","sessionId":"session","serverId":"server","version":"0.2.0","protocolVersion":1,"features":{},"futureField":{"enabled":true}}"#,
        )
        .expect("additive fields should remain compatible");

        let encoded = serde_json::to_value(message).expect("message should serialize");
        assert_eq!(encoded["futureField"]["enabled"], true);
    }

    #[test]
    fn unknown_event_payloads_remain_forward_compatible() {
        let message: ServerMessage = serde_json::from_str(
            r#"{"type":"event","topic":"permission.requested","sequence":1,"payload":{"permissionId":"permission_future"}}"#,
        )
        .expect("unknown event payloads should remain parseable");

        let encoded = serde_json::to_value(message).expect("message should serialize");
        assert_eq!(encoded["topic"], "permission.requested");
        assert_eq!(encoded["payload"]["permissionId"], "permission_future");
    }

    #[test]
    fn state_snapshot_fixture_has_strongly_typed_resources() {
        let text = std::fs::read_to_string(format!("{FIXTURE_ROOT}/state/snapshot-response.json"))
            .expect("snapshot fixture should be readable");
        let message: ServerMessage =
            serde_json::from_str(&text).expect("snapshot fixture should be a server message");
        let ServerMessage::RpcResponse {
            result: Some(result),
            ..
        } = message
        else {
            panic!("snapshot fixture should contain an RPC result");
        };
        let snapshot: super::AuthoritativeSnapshot =
            serde_json::from_value(result).expect("snapshot should match the typed model");

        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.revision, 7);
        assert_eq!(snapshot.projects[0].id, "project_fixture");
        assert_eq!(snapshot.workspaces[0].project_id, "project_fixture");
        assert_eq!(snapshot.agents[0].workspace_id, "workspace_fixture");
        assert_eq!(snapshot.agents[0].status, super::AgentStatus::Running);
    }

    #[test]
    fn provider_catalog_decodes_live_models_and_extended_efforts() {
        let catalog: super::ProviderCatalog = serde_json::from_value(serde_json::json!({
            "providers": [{
                "providerId": "codex",
                "available": true,
                "version": "codex-cli 0.146.0",
                "models": [{
                    "id": "gpt-5.6-sol",
                    "displayName": "GPT-5.6-Sol",
                    "description": "Latest frontier agentic coding model.",
                    "isDefault": true,
                    "defaultReasoningEffort": "low",
                    "supportedReasoningEfforts": [
                        { "reasoningEffort": "max" },
                        { "reasoningEffort": "ultra", "description": "Deepest reasoning" }
                    ]
                }]
            }]
        }))
        .expect("provider catalog should match the daemon response");

        assert_eq!(catalog.providers[0].models[0].id, "gpt-5.6-sol");
        assert_eq!(
            catalog.providers[0].models[0].supported_reasoning_efforts[1].reasoning_effort,
            super::AgentReasoningEffort::Ultra
        );
    }

    #[test]
    fn resource_mutation_fixtures_have_strongly_typed_acknowledgements() {
        for relative_path in [
            "resources/project-create-response.json",
            "resources/workspace-create-response.json",
            "resources/agent-create-response.json",
            "resources/agent-start-response.json",
            "resources/agent-stop-response.json",
        ] {
            let text = std::fs::read_to_string(format!("{FIXTURE_ROOT}/{relative_path}"))
                .expect("mutation fixture should be readable");
            let message: ServerMessage =
                serde_json::from_str(&text).expect("mutation fixture should be a server message");
            let ServerMessage::RpcResponse {
                result: Some(result),
                ..
            } = message
            else {
                panic!("mutation fixture should contain an RPC result");
            };
            let acknowledgement: super::ResourceMutationAcknowledgement =
                serde_json::from_value(result).expect("result should match the typed model");

            assert!(acknowledgement.resource_id.contains("fixture"));
            assert!(acknowledgement.revision > 0);
        }
    }

    #[test]
    fn workspace_file_fixtures_have_strongly_typed_results() {
        let listing = fixture_result("workspace/files-list-response.json");
        let listing: super::WorkspaceDirectoryListing =
            serde_json::from_value(listing).expect("listing should match the typed model");
        assert_eq!(listing.workspace_id, "workspace_fixture");
        assert_eq!(listing.path, "src");
        assert_eq!(listing.entries.len(), 2);
        assert_eq!(
            listing.entries[0].kind,
            super::WorkspaceFileEntryKind::Directory
        );

        let content = fixture_result("workspace/file-read-response.json");
        let content: super::WorkspaceFileContent =
            serde_json::from_value(content).expect("content should match the typed model");
        assert_eq!(content.path, "src/main.rs");
        assert_eq!(content.content, "fn main() {}\n");
        assert_eq!(content.byte_length, 13);
    }

    #[test]
    fn workspace_git_fixtures_have_strongly_typed_results() {
        let status = fixture_result("workspace/git-status-response.json");
        let status: super::WorkspaceGitStatus =
            serde_json::from_value(status).expect("status should match the typed model");
        assert!(status.is_repository);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.changes.len(), 2);
        assert_eq!(
            status.changes[0].worktree_status,
            Some(super::WorkspaceGitChangeKind::Modified)
        );

        let diff = fixture_result("workspace/git-diff-response.json");
        let diff: super::WorkspaceGitDiff =
            serde_json::from_value(diff).expect("diff should match the typed model");
        assert_eq!(diff.path, "src/main.rs");
        assert!(diff.unified_diff.contains("println!"));
        assert!(!diff.is_binary);
    }

    #[test]
    fn device_pairing_http_models_use_daemon_camel_case_fields() {
        let response: super::DeviceListResponse = serde_json::from_value(serde_json::json!({
            "devices": [{
                "id": "device_1",
                "clientId": "mobile_1",
                "name": "iPhone",
                "createdAt": "2026-08-16T00:00:00.000Z"
            }]
        }))
        .expect("device response should match the daemon model");
        assert_eq!(response.devices[0].client_id, "mobile_1");

        let offer: super::PairingOffer = serde_json::from_value(serde_json::json!({
            "pairingId": "pair_1",
            "pairingUri": "corbit://pair/example",
            "endpoint": "https://corbit.example.test",
            "serverId": "server_1",
            "hostName": "Development Mac",
            "expiresAt": "2026-08-16T00:05:00.000Z"
        }))
        .expect("pairing offer should match the daemon model");
        assert_eq!(offer.server_id, "server_1");
        assert_eq!(offer.host_name, "Development Mac");
    }

    fn fixture_result(relative_path: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(format!("{FIXTURE_ROOT}/{relative_path}"))
            .expect("server fixture should be readable");
        let message: ServerMessage =
            serde_json::from_str(&text).expect("fixture should be a server message");
        let ServerMessage::RpcResponse {
            result: Some(result),
            ..
        } = message
        else {
            panic!("fixture should contain an RPC result");
        };
        result
    }
}
