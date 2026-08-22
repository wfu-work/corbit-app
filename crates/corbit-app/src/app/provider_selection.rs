//! Per-project draft and per-Agent Provider controls backed by the live Daemon catalog.
//!
//! The global Provider preference only seeds new tasks. Model and reasoning
//! choices are kept separate for each project draft, Agent conversation, and
//! Provider so switching one context cannot silently change another one.

use std::collections::{BTreeMap, BTreeSet};

use corbit_client::{
    AgentReasoningEffort, AgentResource, ProjectResource, ProviderCatalog, ProviderCatalogEntry,
    ProviderModelInfo,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ComposerSelections {
    agents: BTreeMap<String, ScopedSelections>,
    projects: BTreeMap<String, ScopedSelections>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ScopedSelections {
    providers: BTreeMap<String, ProviderSelection>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ProviderSelection {
    model: Option<String>,
    reasoning_effort: Option<AgentReasoningEffort>,
}

impl ComposerSelections {
    pub(super) fn model<'a>(
        &self,
        agent_id: &str,
        provider: &'a ProviderCatalogEntry,
    ) -> Option<&'a ProviderModelInfo> {
        let selected = self
            .agents
            .get(agent_id)
            .and_then(|agent| agent.providers.get(&provider.provider_id))
            .and_then(|selection| selection.model.as_deref());
        resolve_model(provider, selected)
    }

    pub(super) fn reasoning_effort(
        &self,
        agent_id: &str,
        provider: &ProviderCatalogEntry,
    ) -> Option<AgentReasoningEffort> {
        let model = self.model(agent_id, provider)?;
        let selected = self
            .agents
            .get(agent_id)
            .and_then(|agent| agent.providers.get(&provider.provider_id))
            .and_then(|selection| selection.reasoning_effort);
        resolve_reasoning_effort(model, selected)
    }

    pub(super) fn project_model<'a>(
        &self,
        project_id: &str,
        provider: &'a ProviderCatalogEntry,
    ) -> Option<&'a ProviderModelInfo> {
        let selected = self
            .projects
            .get(project_id)
            .and_then(|project| project.providers.get(&provider.provider_id))
            .and_then(|selection| selection.model.as_deref());
        resolve_model(provider, selected)
    }

    pub(super) fn project_reasoning_effort(
        &self,
        project_id: &str,
        provider: &ProviderCatalogEntry,
    ) -> Option<AgentReasoningEffort> {
        let model = self.project_model(project_id, provider)?;
        let selected = self
            .projects
            .get(project_id)
            .and_then(|project| project.providers.get(&provider.provider_id))
            .and_then(|selection| selection.reasoning_effort);
        resolve_reasoning_effort(model, selected)
    }

    pub(super) fn choose_model(
        &mut self,
        agent_id: &str,
        provider: &ProviderCatalogEntry,
        model_id: &str,
    ) -> bool {
        if !provider.models.iter().any(|model| model.id == model_id) {
            return false;
        }
        let selection = Self::selection_mut(&mut self.agents, agent_id, &provider.provider_id);
        let before = selection.clone();
        selection.model = Some(model_id.to_owned());
        reconcile_selection(selection, provider);
        *selection != before
    }

    pub(super) fn choose_reasoning_effort(
        &mut self,
        agent_id: &str,
        provider: &ProviderCatalogEntry,
        effort: AgentReasoningEffort,
    ) -> bool {
        let selection = Self::selection_mut(&mut self.agents, agent_id, &provider.provider_id);
        reconcile_selection(selection, provider);
        let Some(model) = resolve_model(provider, selection.model.as_deref()) else {
            return false;
        };
        if !model_supports_effort(model, effort) {
            return false;
        }
        if selection.reasoning_effort == Some(effort) {
            return false;
        }
        selection.reasoning_effort = Some(effort);
        true
    }

    pub(super) fn choose_project_model(
        &mut self,
        project_id: &str,
        provider: &ProviderCatalogEntry,
        model_id: &str,
    ) -> bool {
        if !provider.models.iter().any(|model| model.id == model_id) {
            return false;
        }
        let selection = Self::selection_mut(&mut self.projects, project_id, &provider.provider_id);
        let before = selection.clone();
        selection.model = Some(model_id.to_owned());
        reconcile_selection(selection, provider);
        *selection != before
    }

    pub(super) fn choose_project_reasoning_effort(
        &mut self,
        project_id: &str,
        provider: &ProviderCatalogEntry,
        effort: AgentReasoningEffort,
    ) -> bool {
        let selection = Self::selection_mut(&mut self.projects, project_id, &provider.provider_id);
        reconcile_selection(selection, provider);
        let Some(model) = resolve_model(provider, selection.model.as_deref()) else {
            return false;
        };
        if !model_supports_effort(model, effort) || selection.reasoning_effort == Some(effort) {
            return false;
        }
        selection.reasoning_effort = Some(effort);
        true
    }

    /// Records the frozen new-task selection on the newly created Agent. The
    /// catalog reconciliation below repairs a model that disappeared while the
    /// create/start RPC sequence was in flight.
    pub(super) fn set_agent_selection(
        &mut self,
        agent_id: &str,
        provider: &ProviderCatalogEntry,
        model: Option<&str>,
        reasoning_effort: Option<AgentReasoningEffort>,
    ) -> bool {
        let selection = Self::selection_mut(&mut self.agents, agent_id, &provider.provider_id);
        let before = selection.clone();
        selection.model = model.map(str::to_owned);
        selection.reasoning_effort = reasoning_effort;
        reconcile_selection(selection, provider);
        *selection != before
    }

    /// Removes deleted Agents and unavailable Providers, then repairs the
    /// current Provider selection for every authoritative Agent.
    pub(super) fn reconcile_catalog(
        &mut self,
        catalog: &ProviderCatalog,
        agents: &[AgentResource],
        projects: &[ProjectResource],
    ) -> bool {
        let before = self.clone();
        let agent_ids = agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<BTreeSet<_>>();
        let project_ids = projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<BTreeSet<_>>();
        let available_providers = catalog
            .providers
            .iter()
            .filter(|provider| provider.available)
            .map(|provider| (provider.provider_id.as_str(), provider))
            .collect::<BTreeMap<_, _>>();

        reconcile_scoped_selections(&mut self.agents, &agent_ids, &available_providers);
        reconcile_scoped_selections(&mut self.projects, &project_ids, &available_providers);

        for agent in agents {
            let Some(provider) = available_providers.get(agent.provider.as_str()) else {
                continue;
            };
            if provider.models.is_empty() {
                if let Some(selections) = self.agents.get_mut(&agent.id) {
                    selections.providers.remove(&provider.provider_id);
                }
                continue;
            }
            let selection = Self::selection_mut(&mut self.agents, &agent.id, &provider.provider_id);
            reconcile_selection(selection, provider);
        }

        self.agents
            .retain(|_, selections| !selections.providers.is_empty());
        self.projects
            .retain(|_, selections| !selections.providers.is_empty());
        *self != before
    }

    fn selection_mut<'a>(
        scopes: &'a mut BTreeMap<String, ScopedSelections>,
        scope_id: &str,
        provider_id: &str,
    ) -> &'a mut ProviderSelection {
        scopes
            .entry(scope_id.to_owned())
            .or_default()
            .providers
            .entry(provider_id.to_owned())
            .or_default()
    }
}

fn reconcile_scoped_selections(
    selections: &mut BTreeMap<String, ScopedSelections>,
    scope_ids: &BTreeSet<&str>,
    available_providers: &BTreeMap<&str, &ProviderCatalogEntry>,
) {
    selections.retain(|scope_id, _| scope_ids.contains(scope_id.as_str()));
    for scope in selections.values_mut() {
        scope.providers.retain(|provider_id, selection| {
            let Some(provider) = available_providers.get(provider_id.as_str()) else {
                return false;
            };
            if provider.models.is_empty() {
                return false;
            }
            reconcile_selection(selection, provider);
            true
        });
    }
}

fn reconcile_selection(selection: &mut ProviderSelection, provider: &ProviderCatalogEntry) {
    let Some(model) = resolve_model(provider, selection.model.as_deref()) else {
        selection.model = None;
        selection.reasoning_effort = None;
        return;
    };
    selection.model = Some(model.id.clone());
    selection.reasoning_effort = resolve_reasoning_effort(model, selection.reasoning_effort);
}

fn resolve_model<'a>(
    provider: &'a ProviderCatalogEntry,
    selected: Option<&str>,
) -> Option<&'a ProviderModelInfo> {
    selected
        .and_then(|selected| provider.models.iter().find(|model| model.id == selected))
        .or_else(|| provider.models.iter().find(|model| model.is_default))
        .or_else(|| provider.models.first())
}

fn resolve_reasoning_effort(
    model: &ProviderModelInfo,
    selected: Option<AgentReasoningEffort>,
) -> Option<AgentReasoningEffort> {
    selected
        .filter(|effort| model_supports_effort(model, *effort))
        .or(model
            .default_reasoning_effort
            .filter(|effort| model_supports_effort(model, *effort)))
        .or_else(|| {
            model_supports_effort(model, AgentReasoningEffort::Medium)
                .then_some(AgentReasoningEffort::Medium)
        })
        .or_else(|| {
            model
                .supported_reasoning_efforts
                .first()
                .map(|effort| effort.reasoning_effort)
        })
}

fn model_supports_effort(model: &ProviderModelInfo, effort: AgentReasoningEffort) -> bool {
    model
        .supported_reasoning_efforts
        .iter()
        .any(|candidate| candidate.reasoning_effort == effort)
}

#[cfg(test)]
mod tests {
    use super::*;
    use corbit_client::ProviderReasoningEffortInfo;

    fn model(
        id: &str,
        is_default: bool,
        default_reasoning_effort: AgentReasoningEffort,
        efforts: &[AgentReasoningEffort],
    ) -> ProviderModelInfo {
        ProviderModelInfo {
            id: id.into(),
            display_name: id.into(),
            description: String::new(),
            is_default,
            default_reasoning_effort: Some(default_reasoning_effort),
            supported_reasoning_efforts: efforts
                .iter()
                .copied()
                .map(|reasoning_effort| ProviderReasoningEffortInfo {
                    reasoning_effort,
                    description: None,
                })
                .collect(),
        }
    }

    fn provider(models: Vec<ProviderModelInfo>) -> ProviderCatalogEntry {
        ProviderCatalogEntry {
            provider_id: "codex".into(),
            available: true,
            version: Some("test".into()),
            reason: None,
            models,
        }
    }

    fn agent(id: &str) -> AgentResource {
        AgentResource {
            id: id.into(),
            workspace_id: "workspace-1".into(),
            provider: "codex".into(),
            title: id.into(),
            status: corbit_client::AgentStatus::Running,
            created_at: "2026-08-21T00:00:00Z".into(),
            updated_at: "2026-08-21T00:00:00Z".into(),
            extensions: BTreeMap::new(),
        }
    }

    fn project(id: &str) -> ProjectResource {
        ProjectResource {
            id: id.into(),
            name: id.into(),
            root_path: format!("/work/{id}"),
            created_at: "2026-08-21T00:00:00Z".into(),
            updated_at: "2026-08-21T00:00:00Z".into(),
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn selections_are_isolated_per_agent() {
        let provider = provider(vec![
            model(
                "gpt-default",
                true,
                AgentReasoningEffort::Medium,
                &[AgentReasoningEffort::Low, AgentReasoningEffort::Medium],
            ),
            model(
                "gpt-deep",
                false,
                AgentReasoningEffort::High,
                &[AgentReasoningEffort::High],
            ),
        ]);
        let mut selections = ComposerSelections::default();

        assert!(selections.choose_model("agent-1", &provider, "gpt-deep"));
        assert_eq!(
            selections
                .model("agent-1", &provider)
                .map(|model| model.id.as_str()),
            Some("gpt-deep")
        );
        assert_eq!(
            selections
                .model("agent-2", &provider)
                .map(|model| model.id.as_str()),
            Some("gpt-default")
        );
    }

    #[test]
    fn changing_model_repairs_an_unsupported_reasoning_effort() {
        let provider = provider(vec![
            model(
                "gpt-fast",
                true,
                AgentReasoningEffort::Low,
                &[AgentReasoningEffort::Low],
            ),
            model(
                "gpt-deep",
                false,
                AgentReasoningEffort::High,
                &[AgentReasoningEffort::High],
            ),
        ]);
        let mut selections = ComposerSelections::default();

        assert!(selections.choose_model("agent-1", &provider, "gpt-fast"));
        assert!(selections.choose_model("agent-1", &provider, "gpt-deep"));
        assert_eq!(
            selections.reasoning_effort("agent-1", &provider),
            Some(AgentReasoningEffort::High)
        );
    }

    #[test]
    fn project_selection_is_copied_to_the_created_agent() {
        let provider = provider(vec![
            model(
                "gpt-default",
                true,
                AgentReasoningEffort::Medium,
                &[AgentReasoningEffort::Medium],
            ),
            model(
                "gpt-deep",
                false,
                AgentReasoningEffort::High,
                &[AgentReasoningEffort::High, AgentReasoningEffort::Xhigh],
            ),
        ]);
        let mut selections = ComposerSelections::default();

        assert!(selections.choose_project_model("project-1", &provider, "gpt-deep"));
        assert!(selections.choose_project_reasoning_effort(
            "project-1",
            &provider,
            AgentReasoningEffort::Xhigh,
        ));
        let model = selections
            .project_model("project-1", &provider)
            .map(|model| model.id.clone());
        let reasoning = selections.project_reasoning_effort("project-1", &provider);

        assert!(selections.set_agent_selection("agent-1", &provider, model.as_deref(), reasoning,));
        assert_eq!(
            selections
                .model("agent-1", &provider)
                .map(|model| model.id.as_str()),
            Some("gpt-deep")
        );
        assert_eq!(
            selections.reasoning_effort("agent-1", &provider),
            Some(AgentReasoningEffort::Xhigh)
        );
        assert_eq!(
            selections
                .project_model("project-2", &provider)
                .map(|model| model.id.as_str()),
            Some("gpt-default")
        );
    }

    #[test]
    fn catalog_reconciliation_repairs_models_and_removes_deleted_scopes() {
        let original = provider(vec![
            model(
                "gpt-old",
                true,
                AgentReasoningEffort::Low,
                &[AgentReasoningEffort::Low],
            ),
            model(
                "gpt-new",
                false,
                AgentReasoningEffort::High,
                &[AgentReasoningEffort::High],
            ),
        ]);
        let replacement = provider(vec![model(
            "gpt-current",
            true,
            AgentReasoningEffort::Medium,
            &[AgentReasoningEffort::Medium],
        )]);
        let mut selections = ComposerSelections::default();
        selections.choose_model("agent-1", &original, "gpt-old");
        selections.choose_model("deleted-agent", &original, "gpt-new");
        selections.choose_project_model("project-1", &original, "gpt-old");
        selections.choose_project_model("deleted-project", &original, "gpt-new");

        assert!(selections.reconcile_catalog(
            &ProviderCatalog {
                providers: vec![replacement.clone()],
            },
            &[agent("agent-1")],
            &[project("project-1")],
        ));
        assert_eq!(
            selections
                .model("agent-1", &replacement)
                .map(|model| model.id.as_str()),
            Some("gpt-current")
        );
        assert!(!selections.agents.contains_key("deleted-agent"));
        assert_eq!(
            selections
                .project_model("project-1", &replacement)
                .map(|model| model.id.as_str()),
            Some("gpt-current")
        );
        assert!(!selections.projects.contains_key("deleted-project"));
    }

    #[test]
    fn selections_round_trip_for_application_restart() {
        let provider = provider(vec![model(
            "gpt-current",
            true,
            AgentReasoningEffort::High,
            &[AgentReasoningEffort::High],
        )]);
        let mut selections = ComposerSelections::default();
        selections.choose_model("agent-1", &provider, "gpt-current");
        selections.choose_project_model("project-1", &provider, "gpt-current");

        let json = serde_json::to_string(&selections).expect("selections should serialize");
        let restored: ComposerSelections =
            serde_json::from_str(&json).expect("selections should deserialize");

        assert_eq!(restored, selections);
        assert_eq!(
            restored.reasoning_effort("agent-1", &provider),
            Some(AgentReasoningEffort::High)
        );
        assert_eq!(
            restored.project_reasoning_effort("project-1", &provider),
            Some(AgentReasoningEffort::High)
        );
    }
}
