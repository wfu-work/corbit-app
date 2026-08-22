//! Provider switch transaction used by the conversation composer.

use serde_json::json;

pub(super) struct ProviderSwitchFailure {
    pub(super) message: String,
    pub(super) snapshot: Option<corbit_client::AuthoritativeSnapshot>,
    pub(super) provider_updated: bool,
}

pub(super) async fn execute_provider_switch(
    client: corbit_client::DaemonRuntimeClient,
    agent: corbit_client::AgentResource,
    provider: String,
) -> Result<corbit_client::AuthoritativeSnapshot, ProviderSwitchFailure> {
    let mut snapshot = None;
    if agent.status == corbit_client::AgentStatus::Running {
        match client
            .mutate_and_snapshot(
                "agent.stop",
                json!({
                    "agentId": agent.id.clone(),
                    "clientMutationId": format!("provider_switch_stop_{}", uuid::Uuid::new_v4()),
                }),
            )
            .await
        {
            Ok((_, stopped_snapshot)) => snapshot = Some(stopped_snapshot),
            Err(error) => {
                return Err(ProviderSwitchFailure {
                    message: format!("停止原 Provider 会话失败：{error}"),
                    snapshot: None,
                    provider_updated: false,
                });
            }
        }
    }

    if agent.provider != provider {
        match client
            .mutate_and_snapshot(
                "agent.update",
                json!({
                    "agentId": agent.id.clone(),
                    "provider": provider.clone(),
                    "clientMutationId": format!("provider_switch_update_{}", uuid::Uuid::new_v4()),
                }),
            )
            .await
        {
            Ok((_, updated_snapshot)) => snapshot = Some(updated_snapshot),
            Err(error) => {
                return Err(ProviderSwitchFailure {
                    message: format!("更新 Agent Provider 失败：{error}"),
                    snapshot,
                    provider_updated: false,
                });
            }
        }
    }

    match client
        .mutate_and_snapshot(
            "agent.start",
            json!({
                "agentId": agent.id.clone(),
                "clientMutationId": format!("provider_switch_start_{}", uuid::Uuid::new_v4()),
            }),
        )
        .await
    {
        Ok((_, running_snapshot)) => Ok(running_snapshot),
        Err(error) => Err(ProviderSwitchFailure {
            message: format!("新 Provider 会话启动失败：{error}"),
            snapshot,
            provider_updated: agent.provider != provider,
        }),
    }
}
