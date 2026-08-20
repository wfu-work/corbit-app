use std::time::Duration;

use corbit_client::{AgentTimelineEvent, ConnectionEvent, RuntimeEvent};

// Text does not benefit from 60 FPS updates. A 50 ms batch keeps streaming
// responsive while leaving enough main-thread time for layout and input.
pub(super) const STREAMING_BATCH_INTERVAL: Duration = Duration::from_millis(50);
const MAX_STREAMING_EVENTS_PER_BATCH: usize = 256;

pub(super) fn is_streaming_timeline_delta(event: &RuntimeEvent) -> bool {
    matches!(
        event,
        RuntimeEvent::Connection(ConnectionEvent::AgentTimeline { payload, .. })
            if matches!(
                &payload.event,
                AgentTimelineEvent::AssistantDelta { .. }
                    | AgentTimelineEvent::ReasoningDelta { .. }
                    | AgentTimelineEvent::CommandOutputDelta { .. }
            )
    )
}

pub(super) fn collect_runtime_event_batch(
    first: RuntimeEvent,
    mut try_next: impl FnMut() -> Option<RuntimeEvent>,
) -> (Vec<RuntimeEvent>, Option<RuntimeEvent>) {
    if !is_streaming_timeline_delta(&first) {
        return (vec![first], None);
    }

    let mut batch = Vec::with_capacity(16);
    batch.push(first);
    while batch.len() < MAX_STREAMING_EVENTS_PER_BATCH {
        let Some(next) = try_next() else {
            break;
        };
        if is_streaming_timeline_delta(&next) {
            batch.push(next);
        } else {
            return (batch, Some(next));
        }
    }
    (batch, None)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use corbit_client::{AgentTimelinePayload, AgentTurnStatus, ConnectionState};

    use super::*;

    fn runtime_timeline_event(sequence: u64, event: AgentTimelineEvent) -> RuntimeEvent {
        RuntimeEvent::Connection(ConnectionEvent::AgentTimeline {
            sequence,
            payload: AgentTimelinePayload {
                agent_id: "agent-1".into(),
                provider: None,
                event,
                extensions: BTreeMap::new(),
            },
        })
    }

    fn assistant_delta(sequence: u64) -> RuntimeEvent {
        runtime_timeline_event(
            sequence,
            AgentTimelineEvent::AssistantDelta {
                turn_id: "turn-1".into(),
                item_id: "message-1".into(),
                delta: sequence.to_string(),
                occurred_at: "2026-08-16T00:00:00Z".into(),
                extensions: BTreeMap::new(),
            },
        )
    }

    fn reasoning_delta(sequence: u64) -> RuntimeEvent {
        runtime_timeline_event(
            sequence,
            AgentTimelineEvent::ReasoningDelta {
                turn_id: "turn-1".into(),
                item_id: "reasoning-1".into(),
                delta: sequence.to_string(),
                occurred_at: "2026-08-16T00:00:00Z".into(),
                extensions: BTreeMap::new(),
            },
        )
    }

    fn command_output_delta(sequence: u64) -> RuntimeEvent {
        runtime_timeline_event(
            sequence,
            AgentTimelineEvent::CommandOutputDelta {
                turn_id: "turn-1".into(),
                item_id: "command-1".into(),
                delta: sequence.to_string(),
                occurred_at: "2026-08-16T00:00:00Z".into(),
                extensions: BTreeMap::new(),
            },
        )
    }

    fn turn_completed(sequence: u64) -> RuntimeEvent {
        runtime_timeline_event(
            sequence,
            AgentTimelineEvent::TurnCompleted {
                turn_id: "turn-1".into(),
                status: AgentTurnStatus::Completed,
                occurred_at: "2026-08-16T00:00:01Z".into(),
                error: None,
                duration_ms: Some(1_000),
                extensions: BTreeMap::new(),
            },
        )
    }

    #[test]
    fn classifies_only_high_frequency_timeline_deltas_as_streaming() {
        assert!(is_streaming_timeline_delta(&assistant_delta(1)));
        assert!(is_streaming_timeline_delta(&reasoning_delta(2)));
        assert!(is_streaming_timeline_delta(&command_output_delta(3)));
        assert!(!is_streaming_timeline_delta(&turn_completed(4)));
        assert!(!is_streaming_timeline_delta(&RuntimeEvent::Connection(
            ConnectionEvent::StateChanged(ConnectionState::Online),
        )));
    }

    #[test]
    fn stops_before_a_lifecycle_boundary_and_preserves_the_next_event() {
        let mut queued = VecDeque::from([
            reasoning_delta(2),
            command_output_delta(3),
            turn_completed(4),
            assistant_delta(5),
        ]);

        let (batch, pending) =
            collect_runtime_event_batch(assistant_delta(1), || queued.pop_front());

        assert_eq!(batch.len(), 3);
        assert!(matches!(
            pending,
            Some(RuntimeEvent::Connection(ConnectionEvent::AgentTimeline {
                payload: AgentTimelinePayload {
                    event: AgentTimelineEvent::TurnCompleted { .. },
                    ..
                },
                ..
            }))
        ));
        assert_eq!(queued.len(), 1);
    }

    #[test]
    fn does_not_poll_the_queue_for_an_immediate_event() {
        let mut polled = false;
        let (batch, pending) = collect_runtime_event_batch(turn_completed(1), || {
            polled = true;
            None
        });

        assert_eq!(batch.len(), 1);
        assert!(pending.is_none());
        assert!(!polled);
    }

    #[test]
    fn caps_each_streaming_batch_to_bound_ui_update_work() {
        let mut queued = (2..=301).map(assistant_delta).collect::<VecDeque<_>>();

        let (batch, pending) =
            collect_runtime_event_batch(assistant_delta(1), || queued.pop_front());

        assert_eq!(batch.len(), MAX_STREAMING_EVENTS_PER_BATCH);
        assert!(pending.is_none());
        assert_eq!(queued.len(), 301 - MAX_STREAMING_EVENTS_PER_BATCH);
    }
}
