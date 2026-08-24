//! Shared model-provider metadata and visual identity.
//!
//! Provider labels, capability flags, descriptions, and badge sizes live here
//! so settings, task creation, and the conversation timeline cannot drift.

use gpui::AnyElement;

use super::branding::provider_logo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderInfo {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) feature: &'static str,
    pub(crate) description: &'static str,
    pub(crate) detail: &'static str,
}

pub(crate) const PROVIDERS: [ProviderInfo; 3] = [
    ProviderInfo {
        id: "codex",
        label: "Codex",
        feature: "codexProviderSessions",
        description: "OpenAI Codex 本机 Agent 会话",
        detail: "通过 Codex app-server 创建、恢复并继续任务。",
    },
    ProviderInfo {
        id: "claude",
        label: "Claude",
        feature: "claudeProviderSessions",
        description: "Anthropic Claude Code Agent 会话",
        detail: "通过 Claude Agent SDK 运行，并沿用 Daemon 主机上的登录状态。",
    },
    ProviderInfo {
        id: "acp",
        label: "ACP",
        feature: "acpProviderSessions",
        description: "兼容 Agent Client Protocol 的 Agent 会话",
        detail: "需在 Daemon 中配置 CORBIT_ACP_COMMAND 后启用。",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderBadgeSize {
    Inline,
    Settings,
}

impl ProviderBadgeSize {
    const fn pixels(self) -> f32 {
        match self {
            Self::Inline => 18.,
            Self::Settings => 36.,
        }
    }
}

pub(crate) fn provider_badge(provider_id: &str, size: ProviderBadgeSize) -> AnyElement {
    provider_logo(provider_id, size.pixels())
}

pub(crate) fn provider_label(provider_id: &str) -> &str {
    PROVIDERS
        .iter()
        .find(|provider| provider.id == provider_id)
        .map_or(provider_id, |provider| provider.label)
}

pub(crate) fn provider_supports_turn_options(provider_id: &str) -> bool {
    matches!(provider_id, "codex" | "claude")
}

pub(crate) fn model_display_name(model_id: &str, catalog_display_name: &str) -> String {
    match model_id {
        "gpt-5.6-sol" => "GPT-5.6-sol".into(),
        _ => catalog_display_name.into(),
    }
}

pub(crate) fn reasoning_effort_short_label(
    effort: corbit_client::AgentReasoningEffort,
) -> &'static str {
    match effort {
        corbit_client::AgentReasoningEffort::Low => "低",
        corbit_client::AgentReasoningEffort::Medium => "中",
        corbit_client::AgentReasoningEffort::High => "高",
        corbit_client::AgentReasoningEffort::Xhigh => "极高",
        corbit_client::AgentReasoningEffort::Max => "最高",
        corbit_client::AgentReasoningEffort::Ultra => "超强",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_metadata_has_unique_ids_and_capabilities() {
        for (index, provider) in PROVIDERS.iter().enumerate() {
            assert!(!provider.id.is_empty());
            assert!(!provider.label.is_empty());
            assert!(!provider.feature.is_empty());
            assert!(
                PROVIDERS[index + 1..]
                    .iter()
                    .all(|candidate| candidate.id != provider.id)
            );
        }
    }

    #[test]
    fn provider_label_preserves_unknown_ids() {
        assert_eq!(provider_label("codex"), "Codex");
        assert_eq!(provider_label("custom-provider"), "custom-provider");
    }

    #[test]
    fn model_display_name_uses_canonical_sol_casing() {
        assert_eq!(
            model_display_name("gpt-5.6-sol", "GPT-5.6-Sol"),
            "GPT-5.6-sol"
        );
        assert_eq!(
            model_display_name("custom-model", "Custom Model"),
            "Custom Model"
        );
    }

    #[test]
    fn inline_badge_is_large_enough_for_conversation_metadata() {
        assert!((ProviderBadgeSize::Inline.pixels() - 18.).abs() < f32::EPSILON);
        assert!(ProviderBadgeSize::Settings.pixels() > ProviderBadgeSize::Inline.pixels());
    }
}
