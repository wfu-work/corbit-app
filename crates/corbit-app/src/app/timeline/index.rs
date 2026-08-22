//! Conversation index interaction and animation helpers.

use gpui::{ListState, px};

pub(super) const CONVERSATION_INDEX_MAX_MARKERS: usize = 32;
pub(super) const CONVERSATION_INDEX_ANIMATION_DURATION: std::time::Duration =
    std::time::Duration::from_millis(180);

pub(super) fn conversation_index_entries(turn_count: usize) -> Vec<usize> {
    if turn_count <= CONVERSATION_INDEX_MAX_MARKERS {
        return (0..turn_count).collect();
    }

    let last_turn = turn_count - 1;
    (0..CONVERSATION_INDEX_MAX_MARKERS)
        .map(|slot| slot * last_turn / (CONVERSATION_INDEX_MAX_MARKERS - 1))
        .collect()
}

pub(super) fn closest_conversation_index_entry(
    entries: &[usize],
    active_turn: usize,
) -> Option<usize> {
    entries
        .iter()
        .copied()
        .min_by_key(|entry| entry.abs_diff(active_turn))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::app) struct ConversationIndexInteraction {
    pub(super) hovered: bool,
    pub(super) from_slot: Option<usize>,
    pub(super) to_slot: Option<usize>,
    pub(super) animation_generation: u64,
}

impl ConversationIndexInteraction {
    pub(super) fn set_hovered(&mut self, hovered: bool) -> bool {
        if self.hovered == hovered {
            return false;
        }

        self.hovered = hovered;
        if hovered {
            self.from_slot = None;
            self.to_slot = None;
        } else {
            self.from_slot = self.to_slot;
            self.to_slot = None;
        }
        self.animation_generation = self.animation_generation.wrapping_add(1);
        true
    }

    pub(super) fn focus_slot(&mut self, slot: usize) -> bool {
        if self.hovered && self.to_slot == Some(slot) {
            return false;
        }

        self.from_slot = self.hovered.then_some(self.to_slot).flatten();
        self.to_slot = Some(slot);
        self.hovered = true;
        self.animation_generation = self.animation_generation.wrapping_add(1);
        true
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ConversationIndexMarkerMetrics {
    pub(super) width: f32,
    pub(super) emphasis: f32,
}

pub(super) fn conversation_index_marker_metrics(
    is_active: bool,
    marker_slot: usize,
    focus_slot: Option<usize>,
) -> ConversationIndexMarkerMetrics {
    let base_width = if is_active { 16. } else { 8. };
    let expanded_width = if is_active { 22. } else { 20. };
    let emphasis = focus_slot.map_or(0., |focus_slot| match marker_slot.abs_diff(focus_slot) {
        0 => 1.,
        1 => 0.42,
        2 => 0.16,
        _ => 0.,
    });

    ConversationIndexMarkerMetrics {
        width: base_width + (expanded_width - base_width) * emphasis,
        emphasis,
    }
}

pub(super) fn interpolate_rgba(from: gpui::Rgba, to: gpui::Rgba, delta: f32) -> gpui::Rgba {
    gpui::Rgba {
        r: from.r + (to.r - from.r) * delta,
        g: from.g + (to.g - from.g) * delta,
        b: from.b + (to.b - from.b) * delta,
        a: from.a + (to.a - from.a) * delta,
    }
}

pub(super) fn active_turn_indicator_dot_opacity(delta: f32, dot_index: u8) -> f32 {
    let phase = (delta - f32::from(dot_index) * 0.18).rem_euclid(1.0);
    let direct_distance = (phase - 0.32).abs();
    let wrapped_distance = direct_distance.min(1.0 - direct_distance);
    let pulse = (1.0 - wrapped_distance / 0.2).clamp(0.0, 1.0);
    0.35 + pulse * pulse * 0.65
}

pub(super) fn scroll_timeline_to_latest(list_state: &ListState) {
    list_state.scroll_to(gpui::ListOffset {
        item_ix: list_state.item_count(),
        offset_in_item: px(0.),
    });
}
