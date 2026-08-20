use corbit_client::{
    AgentApprovalDecision, AgentPermissionEvent, AgentTimelineEvent, AgentTurnStatus, ClientConfig,
    ConnectionEvent, CorbitClient,
};
use corbit_protocol::PROTOCOL_VERSION;
use serde_json::json;
use std::process::Command;

fn git(root: &str, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .status()
        .expect("Git should be available for the Daemon E2E test");
    assert!(status.success(), "Git command failed: {arguments:?}");
}

#[tokio::test]
#[ignore = "requires a separately running corbit-daemon"]
#[allow(clippy::too_many_lines)]
async fn talks_to_a_real_corbit_daemon() {
    let endpoint = std::env::var("CORBIT_DAEMON_URL")
        .expect("CORBIT_DAEMON_URL must identify the running test daemon");
    let token = std::env::var("CORBIT_AUTH_TOKEN")
        .expect("CORBIT_AUTH_TOKEN must match the running test daemon");
    let client = CorbitClient::new(ClientConfig::desktop(endpoint, token).unwrap()).unwrap();

    assert_eq!(client.health().await.unwrap().status, "ok");
    let info = client.info().await.unwrap();
    assert_eq!(info.protocol_version, PROTOCOL_VERSION);
    assert_eq!(info.features.get("workspaceGit"), Some(&true));
    assert_eq!(info.features.get("workspaceWatch"), Some(&true));

    let connection = client.connect().await.unwrap();
    let mut events = client.subscribe();
    assert_eq!(connection.server_info().server_id, info.server_id);
    connection.ping().await.unwrap();
    let initial_snapshot = connection.snapshot().await.unwrap();
    assert_eq!(initial_snapshot.schema_version, 1);

    let test_id = uuid::Uuid::new_v4();
    let root_path = format!("/private/tmp/corbit-e2e-{test_id}");
    std::fs::create_dir_all(&root_path).unwrap();
    let source_path = format!("{root_path}/src");
    let source_file_path = format!("{source_path}/main.rs");
    let readme_path = format!("{root_path}/README.md");
    std::fs::create_dir(&source_path).unwrap();
    std::fs::write(&source_file_path, "fn main() {}\n").unwrap();
    std::fs::write(&readme_path, "# E2E Workspace\n").unwrap();
    git(&root_path, &["init", "-b", "main"]);
    git(&root_path, &["config", "user.name", "Corbit E2E"]);
    git(
        &root_path,
        &["config", "user.email", "corbit-e2e@example.test"],
    );
    git(&root_path, &["add", "."]);
    git(&root_path, &["commit", "-m", "initial"]);
    let project = connection
        .mutate_resource(
            "project.create",
            json!({
                "clientMutationId": format!("mutation_project_create_{test_id}"),
                "name": "E2E Project",
                "rootPath": root_path,
            }),
        )
        .await
        .unwrap();
    let project_retry = connection
        .mutate_resource(
            "project.create",
            json!({
                "clientMutationId": format!("mutation_project_create_{test_id}"),
                "name": "E2E Project",
                "rootPath": root_path,
            }),
        )
        .await
        .unwrap();
    assert_eq!(project, project_retry);

    let workspace = connection
        .mutate_resource(
            "workspace.create",
            json!({
                "clientMutationId": format!("mutation_workspace_create_{test_id}"),
                "projectId": project.resource_id,
                "name": "E2E Workspace",
                "workingDirectory": root_path,
            }),
        )
        .await
        .unwrap();
    let snapshot = connection.snapshot().await.unwrap();
    assert!(
        snapshot
            .projects
            .iter()
            .any(|item| item.id == project.resource_id)
    );
    assert!(
        snapshot
            .workspaces
            .iter()
            .any(|item| item.id == workspace.resource_id && item.project_id == project.resource_id)
    );
    let root_listing = connection
        .list_workspace_files(&workspace.resource_id, "")
        .await
        .unwrap();
    assert_eq!(root_listing.workspace_id, workspace.resource_id);
    assert!(root_listing.entries.iter().any(|entry| {
        entry.path == "src" && entry.kind == corbit_protocol::WorkspaceFileEntryKind::Directory
    }));
    let source = connection
        .read_workspace_file(&workspace.resource_id, "src/main.rs")
        .await
        .unwrap();
    assert_eq!(source.content, "fn main() {}\n");
    assert_eq!(source.byte_length, 13);
    std::fs::write(
        &source_file_path,
        "fn main() { println!(\"Corbit E2E\"); }\n",
    )
    .unwrap();
    std::fs::write(format!("{root_path}/untracked.txt"), "new file\n").unwrap();
    let workspace_change = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let ConnectionEvent::WorkspaceChanged(change) = events.recv().await.unwrap()
                && change.workspace_id == workspace.resource_id
            {
                break change;
            }
        }
    })
    .await
    .expect("workspace file changes should be broadcast");
    assert!(
        workspace_change.paths.is_empty()
            || workspace_change
                .paths
                .iter()
                .any(|path| path == "src/main.rs" || path == "untracked.txt")
    );
    let git_status = connection
        .workspace_git_status(&workspace.resource_id)
        .await
        .unwrap();
    assert!(git_status.is_repository);
    assert_eq!(git_status.branch.as_deref(), Some("main"));
    assert!(git_status.changes.iter().any(|change| {
        change.path == "src/main.rs"
            && change.worktree_status == Some(corbit_protocol::WorkspaceGitChangeKind::Modified)
    }));
    assert!(git_status.changes.iter().any(|change| {
        change.path == "untracked.txt"
            && change.worktree_status == Some(corbit_protocol::WorkspaceGitChangeKind::Untracked)
    }));
    let git_diff = connection
        .workspace_git_diff(&workspace.resource_id, "src/main.rs")
        .await
        .unwrap();
    assert!(git_diff.unified_diff.contains("Corbit E2E"));
    assert!(!git_diff.is_binary);

    let agent = connection
        .mutate_resource(
            "agent.create",
            json!({
                "clientMutationId": format!("mutation_agent_create_{test_id}"),
                "workspaceId": workspace.resource_id,
                "provider": "codex",
                "title": "E2E Agent",
            }),
        )
        .await
        .unwrap();
    let agent_retry = connection
        .mutate_resource(
            "agent.create",
            json!({
                "clientMutationId": format!("mutation_agent_create_{test_id}"),
                "workspaceId": workspace.resource_id,
                "provider": "codex",
                "title": "E2E Agent",
            }),
        )
        .await
        .unwrap();
    assert_eq!(agent, agent_retry);
    connection
        .mutate_resource(
            "agent.update",
            json!({
                "clientMutationId": format!("mutation_agent_update_{test_id}"),
                "agentId": agent.resource_id,
                "title": "E2E Agent Renamed",
            }),
        )
        .await
        .unwrap();
    let snapshot = connection.snapshot().await.unwrap();
    assert!(snapshot.agents.iter().any(|item| {
        item.id == agent.resource_id
            && item.workspace_id == workspace.resource_id
            && item.provider == "codex"
            && item.title == "E2E Agent Renamed"
            && item.status == corbit_protocol::AgentStatus::Idle
    }));
    connection
        .mutate_resource(
            "agent.start",
            json!({
                "clientMutationId": format!("mutation_agent_start_{test_id}"),
                "agentId": agent.resource_id,
            }),
        )
        .await
        .unwrap();
    let running_snapshot = connection.snapshot().await.unwrap();
    assert!(running_snapshot.agents.iter().any(|item| {
        item.id == agent.resource_id && item.status == corbit_protocol::AgentStatus::Running
    }));

    let prompt = connection
        .prompt(
            &agent.resource_id,
            "Summarize the workspace",
            &format!("mutation_agent_prompt_{test_id}"),
        )
        .await
        .unwrap();
    assert_eq!(prompt.agent_id, agent.resource_id);
    assert_eq!(prompt.turn_id, "turn_test");
    let mut timeline = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while timeline.len() < 3 {
            if let ConnectionEvent::AgentTimeline { payload, .. } = events.recv().await.unwrap()
                && payload.agent_id == agent.resource_id
            {
                timeline.push(payload.event);
            }
        }
    })
    .await
    .expect("fake Codex timeline should complete");
    assert!(matches!(
        timeline[0],
        AgentTimelineEvent::TurnStarted { ref prompt, .. }
            if prompt == "Summarize the workspace"
    ));
    assert!(matches!(
        timeline[1],
        AgentTimelineEvent::AssistantDelta { ref delta, .. } if delta == "Corbit"
    ));
    assert!(matches!(
        timeline[2],
        AgentTimelineEvent::TurnCompleted {
            status: AgentTurnStatus::Completed,
            ..
        }
    ));

    let approval_prompt = connection
        .prompt(
            &agent.resource_id,
            "Request approval",
            &format!("mutation_agent_approval_prompt_{test_id}"),
        )
        .await
        .unwrap();
    assert_eq!(approval_prompt.turn_id, "turn_approval");
    let requested_approval = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let ConnectionEvent::AgentPermission { payload, .. } = events.recv().await.unwrap()
                && payload.agent_id == agent.resource_id
                && let AgentPermissionEvent::PermissionRequested {
                    approval_id,
                    command,
                    ..
                } = payload.event
            {
                assert_eq!(command.as_deref(), Some("git status --short"));
                break approval_id;
            }
        }
    })
    .await
    .expect("fake Codex approval should arrive");
    connection
        .resolve_approval(
            &agent.resource_id,
            &requested_approval,
            AgentApprovalDecision::Accept,
            &format!("mutation_agent_approval_{test_id}"),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut resolved = false;
        let mut completed = false;
        while !resolved || !completed {
            match events.recv().await.unwrap() {
                ConnectionEvent::AgentPermission { payload, .. }
                    if payload.agent_id == agent.resource_id =>
                {
                    resolved = matches!(
                        payload.event,
                        AgentPermissionEvent::PermissionResolved {
                            decision: Some(AgentApprovalDecision::Accept),
                            ..
                        }
                    );
                }
                ConnectionEvent::AgentTimeline { payload, .. }
                    if payload.agent_id == agent.resource_id =>
                {
                    completed = matches!(
                        payload.event,
                        AgentTimelineEvent::TurnCompleted {
                            status: AgentTurnStatus::Completed,
                            ..
                        }
                    );
                }
                _ => {}
            }
        }
    })
    .await
    .expect("approved fake Codex turn should complete");

    let interrupt_prompt = connection
        .prompt(
            &agent.resource_id,
            "Keep working",
            &format!("mutation_agent_interrupt_prompt_{test_id}"),
        )
        .await
        .unwrap();
    connection
        .interrupt(
            &agent.resource_id,
            &interrupt_prompt.turn_id,
            &format!("mutation_agent_interrupt_{test_id}"),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let ConnectionEvent::AgentTimeline { payload, .. } = events.recv().await.unwrap()
                && payload.agent_id == agent.resource_id
                && matches!(
                    payload.event,
                    AgentTimelineEvent::TurnCompleted {
                        status: AgentTurnStatus::Interrupted,
                        ..
                    }
                )
            {
                break;
            }
        }
    })
    .await
    .expect("interrupted fake Codex turn should complete");
    connection
        .mutate_resource(
            "agent.stop",
            json!({
                "clientMutationId": format!("mutation_agent_stop_{test_id}"),
                "agentId": agent.resource_id,
            }),
        )
        .await
        .unwrap();
    connection
        .mutate_resource(
            "agent.delete",
            json!({
                "clientMutationId": format!("mutation_agent_delete_{test_id}"),
                "agentId": agent.resource_id,
            }),
        )
        .await
        .unwrap();

    connection
        .mutate_resource(
            "workspace.update",
            json!({
                "clientMutationId": format!("mutation_workspace_archive_{test_id}"),
                "workspaceId": workspace.resource_id,
                "status": "archived",
            }),
        )
        .await
        .unwrap();
    connection
        .mutate_resource(
            "workspace.delete",
            json!({
                "clientMutationId": format!("mutation_workspace_delete_{test_id}"),
                "workspaceId": workspace.resource_id,
            }),
        )
        .await
        .unwrap();
    connection
        .mutate_resource(
            "project.delete",
            json!({
                "clientMutationId": format!("mutation_project_delete_{test_id}"),
                "projectId": project.resource_id,
            }),
        )
        .await
        .unwrap();
    let final_snapshot = connection.snapshot().await.unwrap();
    assert_eq!(final_snapshot.revision, initial_snapshot.revision + 10);
    assert!(
        final_snapshot
            .projects
            .iter()
            .all(|item| item.id != project.resource_id)
    );
    assert!(
        final_snapshot
            .agents
            .iter()
            .all(|item| item.id != agent.resource_id)
    );
    assert_eq!(
        connection
            .echo(json!({ "source": "corbit-app-e2e" }))
            .await
            .unwrap(),
        json!({ "echo": { "source": "corbit-app-e2e" } })
    );
    connection.close().await.unwrap();
    std::fs::remove_dir_all(&root_path).unwrap();
}
