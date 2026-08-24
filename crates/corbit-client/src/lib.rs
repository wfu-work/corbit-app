//! Async HTTP and WebSocket client for Corbit Daemon.

mod client;
mod config;
mod error;
mod runtime;

pub use client::{ConnectionEvent, ConnectionState, CorbitClient, CorbitConnection};
pub use config::ClientConfig;
pub use corbit_protocol::{
    AgentApprovalDecision, AgentApprovalResolveAcknowledgement, AgentInterruptAcknowledgement,
    AgentPermissionEvent, AgentPermissionMode, AgentPermissionPayload, AgentPersonality,
    AgentPromptAcknowledgement, AgentPromptAttachment, AgentPromptOptions, AgentReasoningEffort,
    AgentReasoningSummary, AgentResource, AgentStatus, AgentSteerAcknowledgement,
    AgentTimelineEvent, AgentTimelineFileChange, AgentTimelineFileChangeKind, AgentTimelinePayload,
    AgentTimelinePlanStep, AgentTimelineStepStatus, AgentTurnStatus, AuthoritativeSnapshot,
    CodexOfficialPluginApp, CodexOfficialPluginCatalog, CodexOfficialPluginInstallResult,
    CodexOfficialPluginInterface, CodexOfficialPluginMarketplace, CodexOfficialPluginSummary,
    DeviceCredentialSummary, DeviceListResponse, PairingOffer, PluginAuthenticationPolicy,
    PluginAuthor, PluginComponents, PluginInspection, PluginInspectionOperation,
    PluginInstallationPolicy, PluginInterface, PluginManifest, PluginMarketplaceEntry,
    PluginMarketplacePolicy, PluginProviderCompatibility, PluginProviderCompatibilityEntry,
    PluginProviderCompatibilityStatus, PluginRecord, PluginSource, ProjectResource,
    ProviderCatalog, ProviderCatalogEntry, ProviderModelInfo, ProviderReasoningEffortInfo,
    ResourceMutationAcknowledgement, ScheduledPromptOptions, ScheduledRun, ScheduledRunStatus,
    ScheduledRunTrigger, ScheduledTask, ScheduledTaskCreateInput,
    ScheduledTaskDeleteAcknowledgement, ScheduledTaskSchedule, ScheduledTaskStatus,
    ScheduledTaskUpdateInput, ServerInfo, WorkspaceChanged, WorkspaceDirectoryListing,
    WorkspaceFileContent, WorkspaceFileEntry, WorkspaceFileEntryKind, WorkspaceGitChange,
    WorkspaceGitChangeKind, WorkspaceGitDiff, WorkspaceGitStatus, WorkspaceResource,
    WorkspaceStatus,
};
pub use error::ClientError;
pub use runtime::{DaemonRuntime, DaemonRuntimeClient, RuntimeEvent};
