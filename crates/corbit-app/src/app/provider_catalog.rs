//! Live Provider catalog refresh lifecycle.
//!
//! Initial synchronization may clear stale connection state. Later automatic
//! and user-initiated refreshes keep the last non-empty catalog on transient
//! failures so an otherwise healthy conversation is not disabled by one RPC.

use super::*;

pub(super) const PROVIDER_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_mins(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogRefreshSource {
    Initial,
    Automatic,
    Manual,
}

impl ConnectionView {
    pub(super) fn start_provider_catalog_if_ready(&mut self, cx: &mut Context<Self>) {
        if !provider_catalog_bootstrap_ready(
            &self.state,
            self.snapshot.is_some(),
            self.provider_catalog.is_some(),
            self.provider_catalog_task.is_some(),
        ) {
            return;
        }

        self.load_provider_catalog(cx);
        self.start_provider_catalog_refresh_loop(cx);
    }

    pub(super) fn load_provider_catalog(&mut self, cx: &mut Context<Self>) {
        self.request_provider_catalog(CatalogRefreshSource::Initial, cx);
    }

    pub(super) fn refresh_provider_catalog(&mut self, cx: &mut Context<Self>) {
        self.request_provider_catalog(CatalogRefreshSource::Manual, cx);
    }

    pub(super) fn start_provider_catalog_refresh_loop(&mut self, cx: &mut Context<Self>) {
        if self.provider_catalog_refresh_task.is_some() {
            return;
        }
        let generation = self.connection_generation;
        self.provider_catalog_refresh_task = Some(cx.spawn(async move |view, cx| {
            loop {
                Timer::after(PROVIDER_CATALOG_REFRESH_INTERVAL).await;
                let Some(view) = view.upgrade() else {
                    break;
                };
                let should_continue = view
                    .update(cx, |view, cx| {
                        if view.connection_generation != generation
                            || !matches!(view.state, corbit_client::ConnectionState::Online)
                        {
                            return false;
                        }
                        view.request_provider_catalog(CatalogRefreshSource::Automatic, cx);
                        true
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        }));
    }

    fn request_provider_catalog(&mut self, source: CatalogRefreshSource, cx: &mut Context<Self>) {
        if self.provider_catalog_task.is_some() {
            return;
        }
        let Some(client) = self
            .runtime
            .as_ref()
            .map(corbit_client::DaemonRuntime::client)
        else {
            return;
        };
        if !matches!(self.state, corbit_client::ConnectionState::Online) {
            return;
        }
        if source == CatalogRefreshSource::Initial {
            self.provider_catalog = None;
        }
        self.provider_catalog_error = None;
        let generation = self.connection_generation;
        self.provider_catalog_request_id = self.provider_catalog_request_id.wrapping_add(1);
        let request_id = self.provider_catalog_request_id;
        self.provider_catalog_task = Some(cx.spawn(async move |view, cx| {
            let result = client.provider_catalog().await;
            let Some(view) = view.upgrade() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if view.connection_generation != generation
                    || view.provider_catalog_request_id != request_id
                {
                    return;
                }
                view.provider_catalog_task = None;
                if !matches!(view.state, corbit_client::ConnectionState::Online) {
                    return;
                }
                match result {
                    Ok(catalog) => {
                        view.provider_catalog = Some(catalog);
                        view.provider_catalog_error = None;
                        view.ensure_selected_provider();
                        view.reconcile_project_providers();
                        view.reconcile_composer_catalog();
                        if source == CatalogRefreshSource::Manual {
                            view.show_success("模型目录已刷新", cx);
                        }
                    }
                    Err(error) => {
                        let retained = catalog_has_entries(view.provider_catalog.as_ref());
                        view.provider_catalog_error = Some(error.to_string());
                        if !retained {
                            view.provider_catalog = Some(corbit_client::ProviderCatalog {
                                providers: Vec::new(),
                            });
                        }
                        match source {
                            CatalogRefreshSource::Initial => {
                                view.show_error(format!("无法读取本机模型目录：{error}"), cx);
                            }
                            CatalogRefreshSource::Manual => {
                                let message = if retained {
                                    format!("模型目录刷新失败，继续使用上次同步结果：{error}")
                                } else {
                                    format!("模型目录刷新失败：{error}")
                                };
                                view.show_warning(message, cx);
                            }
                            CatalogRefreshSource::Automatic => {}
                        }
                    }
                }
                view.schedule_ui_state_save(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn catalog_has_entries(catalog: Option<&corbit_client::ProviderCatalog>) -> bool {
    catalog.is_some_and(|catalog| !catalog.providers.is_empty())
}

fn provider_catalog_bootstrap_ready(
    state: &corbit_client::ConnectionState,
    has_snapshot: bool,
    has_catalog: bool,
    request_active: bool,
) -> bool {
    matches!(state, corbit_client::ConnectionState::Online)
        && has_snapshot
        && !has_catalog
        && !request_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_catalog_bootstrap_waits_for_snapshot_after_online() {
        let state = corbit_client::ConnectionState::Online;

        assert!(!provider_catalog_bootstrap_ready(
            &state, false, false, false
        ));
        assert!(provider_catalog_bootstrap_ready(&state, true, false, false));
    }

    #[test]
    fn provider_catalog_bootstrap_waits_for_online_after_snapshot() {
        let mut state = corbit_client::ConnectionState::Connecting;

        assert!(!provider_catalog_bootstrap_ready(
            &state, true, false, false
        ));
        state = corbit_client::ConnectionState::Online;
        assert!(provider_catalog_bootstrap_ready(&state, true, false, false));
    }

    #[test]
    fn provider_catalog_bootstrap_is_idempotent() {
        let state = corbit_client::ConnectionState::Online;

        assert!(!provider_catalog_bootstrap_ready(&state, true, true, false));
        assert!(!provider_catalog_bootstrap_ready(&state, true, false, true));
    }

    #[test]
    fn refresh_failure_retains_only_a_real_previous_catalog() {
        assert!(!catalog_has_entries(None));
        assert!(!catalog_has_entries(Some(
            &corbit_client::ProviderCatalog {
                providers: Vec::new(),
            }
        )));
        assert!(catalog_has_entries(Some(&corbit_client::ProviderCatalog {
            providers: vec![corbit_client::ProviderCatalogEntry {
                provider_id: "codex".into(),
                available: false,
                version: None,
                reason: Some("temporarily unavailable".into()),
                models: Vec::new(),
            }],
        })));
    }
}
