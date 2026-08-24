//! Replay-safe timeline turn lifecycle transitions.

use super::{TimelineStatus, TimelineTurn};

pub(super) fn remember_turn_provider(turn: &mut TimelineTurn, provider: Option<&str>) {
    if turn.provider.is_none() {
        turn.provider = provider
            .filter(|provider| !provider.is_empty())
            .map(str::to_owned);
    }
}

pub(super) fn apply_turn_started_event(
    turn: &mut TimelineTurn,
    prompt: String,
    occurred_at: String,
) {
    // A replayed or delayed start event must not reopen a terminal turn.
    if turn.status != TimelineStatus::InProgress {
        return;
    }

    turn.prompt = prompt;
    turn.error = None;
    turn.started_at = Some(occurred_at);
    turn.completed_at = None;
    turn.duration_ms = None;
}

pub(super) fn apply_turn_completed_event(
    turn: &mut TimelineTurn,
    status: &corbit_client::AgentTurnStatus,
    error: Option<String>,
    occurred_at: String,
    duration_ms: Option<u64>,
) {
    turn.status = match status {
        corbit_client::AgentTurnStatus::Completed => TimelineStatus::Completed,
        corbit_client::AgentTurnStatus::Interrupted => TimelineStatus::Interrupted,
        corbit_client::AgentTurnStatus::Failed => TimelineStatus::Failed,
    };
    turn.error = error;
    turn.completed_at = Some(occurred_at);
    turn.duration_ms = duration_ms;

    let terminal_step_status = if turn.status == TimelineStatus::Completed {
        corbit_client::AgentTimelineStepStatus::Completed
    } else {
        corbit_client::AgentTimelineStepStatus::Failed
    };
    for step in &mut turn.steps {
        if matches!(
            step.status,
            corbit_client::AgentTimelineStepStatus::Pending
                | corbit_client::AgentTimelineStepStatus::InProgress
        ) {
            step.status = terminal_step_status;
        }
    }
}
