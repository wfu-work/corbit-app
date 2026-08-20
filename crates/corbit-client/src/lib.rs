//! Async HTTP and WebSocket client for Corbit Daemon.

mod client;
mod config;
mod error;
mod runtime;

pub use client::{ConnectionEvent, ConnectionState, CorbitClient, CorbitConnection};
pub use config::ClientConfig;
pub use corbit_protocol::{
    AgentApprovalDecision, AgentApprovalResolveAcknowledgement, AgentInterruptAcknowledgement,
    AgentPermissionEvent, AgentPermissionMode, AgentPermissionPayload, AgentPromptAcknowledgement,
    AgentPromptAttachment, AgentPromptOptions, AgentReasoningEffort, AgentResource, AgentStatus,
    AgentTimelineEvent, AgentTimelineFileChange, AgentTimelineFileChangeKind, AgentTimelinePayload,
    AgentTimelinePlanStep, AgentTimelineStepStatus, AgentTurnStatus, AuthoritativeSnapshot,
    DeviceCredentialSummary, DeviceListResponse, PairingOffer, ProjectResource, ProviderCatalog,
    ProviderCatalogEntry, ProviderModelInfo, ProviderReasoningEffortInfo,
    ResourceMutationAcknowledgement, ServerInfo, WorkspaceChanged, WorkspaceDirectoryListing,
    WorkspaceFileContent, WorkspaceFileEntry, WorkspaceFileEntryKind, WorkspaceGitChange,
    WorkspaceGitChangeKind, WorkspaceGitDiff, WorkspaceGitStatus, WorkspaceResource,
    WorkspaceStatus,
};
pub use error::ClientError;
pub use runtime::{DaemonRuntime, DaemonRuntimeClient, RuntimeEvent};
