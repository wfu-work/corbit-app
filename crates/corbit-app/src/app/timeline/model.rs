//! Timeline domain state and the lookup index used by virtualized rendering.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(in crate::app) struct ComposerAttachment {
    pub(in crate::app) upload: corbit_client::AgentPromptAttachment,
    pub(in crate::app) size_bytes: usize,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct QueuedPrompt {
    pub(in crate::app) agent_id: String,
    pub(in crate::app) text: String,
    pub(in crate::app) options: corbit_client::AgentPromptOptions,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct RetryPrompt {
    pub(in crate::app) signature: String,
    pub(in crate::app) client_mutation_id: String,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct RetryControl {
    pub(in crate::app) signature: String,
    pub(in crate::app) client_mutation_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum TimelineStatus {
    InProgress,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug)]
pub(in crate::app) enum TimelineStepKind {
    AssistantMessage {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Plan {
        explanation: Option<String>,
        steps: Vec<corbit_client::AgentTimelinePlanStep>,
    },
    Command {
        command: String,
        cwd: Option<String>,
        output: String,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
    },
    FileChange {
        changes: Vec<corbit_client::AgentTimelineFileChange>,
    },
    Diff {
        diff: String,
    },
    Tool {
        tool_name: String,
        title: Option<String>,
        input: Option<String>,
        output: Option<String>,
        error: Option<String>,
        duration_ms: Option<u64>,
    },
}

#[derive(Clone, Debug)]
pub(in crate::app) struct TimelineStep {
    pub(in crate::app) item_id: String,
    pub(in crate::app) status: corbit_client::AgentTimelineStepStatus,
    pub(in crate::app) kind: TimelineStepKind,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct TimelineUsage {
    pub(in crate::app) input_tokens: u64,
    pub(in crate::app) output_tokens: u64,
    pub(in crate::app) total_tokens: u64,
    pub(in crate::app) cached_input_tokens: Option<u64>,
    pub(in crate::app) reasoning_output_tokens: Option<u64>,
    pub(in crate::app) context_window: Option<u64>,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct TimelineTurn {
    pub(in crate::app) agent_id: String,
    pub(in crate::app) turn_id: String,
    pub(in crate::app) provider: Option<String>,
    pub(in crate::app) prompt: String,
    pub(in crate::app) steps: Vec<TimelineStep>,
    pub(in crate::app) diff: Option<String>,
    pub(in crate::app) usage: Option<TimelineUsage>,
    pub(in crate::app) started_at: Option<String>,
    pub(in crate::app) completed_at: Option<String>,
    pub(in crate::app) duration_ms: Option<u64>,
    pub(in crate::app) status: TimelineStatus,
    pub(in crate::app) error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TimelineLocation {
    pub(super) timeline_index: usize,
    pub(super) agent_index: usize,
}

#[derive(Debug, Default)]
pub(in crate::app) struct TimelineIndex {
    by_turn: BTreeMap<String, BTreeMap<String, TimelineLocation>>,
    by_agent: BTreeMap<String, Vec<usize>>,
}

impl TimelineIndex {
    pub(super) fn location(&self, agent_id: &str, turn_id: &str) -> Option<TimelineLocation> {
        self.by_turn
            .get(agent_id)
            .and_then(|turns| turns.get(turn_id))
            .copied()
    }

    pub(super) fn insert(
        &mut self,
        agent_id: &str,
        turn_id: &str,
        timeline_index: usize,
    ) -> TimelineLocation {
        if let Some(location) = self.location(agent_id, turn_id) {
            return location;
        }

        let agent_turns = self.by_agent.entry(agent_id.to_owned()).or_default();
        let location = TimelineLocation {
            timeline_index,
            agent_index: agent_turns.len(),
        };
        agent_turns.push(timeline_index);
        self.by_turn
            .entry(agent_id.to_owned())
            .or_default()
            .insert(turn_id.to_owned(), location);
        location
    }

    pub(super) fn agent_indices(&self, agent_id: &str) -> &[usize] {
        self.by_agent.get(agent_id).map_or(&[], Vec::as_slice)
    }

    pub(super) fn clear(&mut self) {
        self.by_turn.clear();
        self.by_agent.clear();
    }
}

#[derive(Clone, Debug)]
pub(in crate::app) struct PendingPermission {
    pub(in crate::app) agent_id: String,
    pub(in crate::app) approval_id: String,
    pub(in crate::app) turn_id: String,
    pub(in crate::app) permission_kind: String,
    pub(in crate::app) reason: Option<String>,
    pub(in crate::app) command: Option<String>,
    pub(in crate::app) cwd: Option<String>,
    pub(in crate::app) grant_root: Option<String>,
    pub(in crate::app) available_decisions: Vec<corbit_client::AgentApprovalDecision>,
}
