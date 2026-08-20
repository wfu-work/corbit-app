use std::time::Duration;

use corbit_client::{
    ClientConfig, ClientError, ConnectionEvent, ConnectionState, CorbitClient, DaemonRuntime,
    RuntimeEvent,
};
use corbit_protocol::{
    AgentTimelineEvent, AgentTurnStatus, AuthoritativeSnapshot, ClientMessage, PROTOCOL_VERSION,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{
    accept_async, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

const TOKEN: &str = "test-token-that-is-at-least-32-characters";

#[tokio::test]
async fn completes_http_handshake_heartbeat_and_echo_flow() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(serve_complete_flow(listener));

    let client = CorbitClient::new(ClientConfig::desktop(&endpoint, TOKEN).unwrap()).unwrap();

    assert_eq!(client.health().await.unwrap().status, "ok");
    let info = client.info().await.unwrap();
    assert_eq!(info.server_id, "srv_test");
    assert_eq!(info.protocol_version, PROTOCOL_VERSION);

    let mut events = client.subscribe();
    let connection = client.connect().await.unwrap();
    assert_eq!(connection.session_id(), "session_test");
    assert_eq!(connection.server_info(), &info);
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Connecting)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Authenticating)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::ServerInfo(info)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Online)
    );

    connection.ping().await.unwrap();
    assert_eq!(
        connection
            .echo(json!({ "message": "hello" }))
            .await
            .unwrap(),
        json!({ "echo": { "message": "hello" } })
    );
    assert_eq!(connection.snapshot().await.unwrap(), empty_snapshot());
    connection.close().await.unwrap();
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Offline)
    );

    server.await.unwrap();
}

#[tokio::test]
async fn maps_daemon_authentication_close_to_a_stable_state() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        socket
            .close(Some(CloseFrame {
                code: CloseCode::from(4_408),
                reason: "Unauthorized".into(),
            }))
            .await
            .unwrap();
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let mut events = client.subscribe();
    assert!(matches!(
        client.connect().await,
        Err(ClientError::AuthenticationFailed)
    ));
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Connecting)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Authenticating)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::AuthenticationFailed)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn reports_the_protocol_version_advertised_by_an_incompatible_daemon() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let _: ClientMessage = read_json(&mut socket).await;
        socket
            .send(Message::text(
                json!({
                    "type": "protocol.error",
                    "error": {
                        "code": "unsupported_protocol",
                        "message": "The protocol version is not supported",
                        "details": { "requested": 1, "supported": 2 }
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let mut events = client.subscribe();
    assert!(matches!(
        client.connect().await,
        Err(ClientError::IncompatibleProtocol {
            expected: PROTOCOL_VERSION,
            actual: 2
        })
    ));
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Connecting)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Authenticating)
    );
    assert_eq!(
        events.recv().await.unwrap(),
        ConnectionEvent::StateChanged(ConnectionState::Incompatible {
            expected: PROTOCOL_VERSION,
            actual: 2,
        })
    );
    server.await.unwrap();
}

#[tokio::test]
async fn routes_concurrent_rpc_responses_by_request_id() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_concurrent").await;

        let first: ClientMessage = read_json(&mut socket).await;
        let second: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id: first_id,
            params: first_params,
            ..
        } = first
        else {
            panic!("expected first RPC request");
        };
        let ClientMessage::RpcRequest {
            id: second_id,
            params: second_params,
            ..
        } = second
        else {
            panic!("expected second RPC request");
        };

        for (id, params) in [(second_id, second_params), (first_id, first_params)] {
            socket
                .send(Message::text(
                    json!({
                        "type": "rpc.response",
                        "id": id,
                        "ok": true,
                        "result": { "echo": params }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
        }

        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let connection = client.connect().await.unwrap();
    let first = {
        let connection = connection.clone();
        tokio::spawn(async move { connection.echo(json!({ "request": 1 })).await })
    };
    let second = {
        let connection = connection.clone();
        tokio::spawn(async move { connection.echo(json!({ "request": 2 })).await })
    };

    assert_eq!(
        first.await.unwrap().unwrap(),
        json!({ "echo": { "request": 1 } })
    );
    assert_eq!(
        second.await.unwrap().unwrap(),
        json!({ "echo": { "request": 2 } })
    );
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn routes_transient_workspace_changes_without_consuming_event_sequences() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_workspace_change").await;
        socket
            .send(Message::text(
                json!({
                    "type": "workspace.changed",
                    "workspaceId": "workspace_test",
                    "paths": ["README.md", "src/main.rs"],
                    "occurredAt": "2026-08-14T00:00:03.000Z"
                })
                .to_string(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::text(
                json!({
                    "type": "event",
                    "topic": "agent.timeline",
                    "sequence": 1,
                    "payload": {
                        "agentId": "agent_test",
                        "event": {
                            "kind": "turn.started",
                            "turnId": "turn_test",
                            "prompt": "Verify the cursor",
                            "occurredAt": "2026-08-14T00:00:04.000Z"
                        }
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let mut events = client.subscribe();
    let connection = client.connect().await.unwrap();
    let mut workspace_change = None;
    let mut timeline_sequence = None;
    while workspace_change.is_none() || timeline_sequence.is_none() {
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            ConnectionEvent::WorkspaceChanged(change) => workspace_change = Some(change),
            ConnectionEvent::AgentTimeline { sequence, .. } => timeline_sequence = Some(sequence),
            _ => {}
        }
    }

    let change = workspace_change.unwrap();
    assert_eq!(change.workspace_id, "workspace_test");
    assert_eq!(change.paths, ["README.md", "src/main.rs"]);
    assert_eq!(timeline_sequence, Some(1));
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn decodes_typed_workspace_file_and_git_results() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_workspace_files").await;

        let listing: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id, method, params, ..
        } = listing
        else {
            panic!("expected workspace listing request");
        };
        assert_eq!(method, "workspace.files.list");
        assert_eq!(
            params,
            Some(json!({ "workspaceId": "workspace_test", "path": "src" }))
        );
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": id,
                    "ok": true,
                    "result": {
                        "workspaceId": "workspace_test",
                        "path": "src",
                        "entries": [
                            { "name": "components", "path": "src/components", "kind": "directory" },
                            { "name": "main.rs", "path": "src/main.rs", "kind": "file" }
                        ]
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let file: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id, method, params, ..
        } = file
        else {
            panic!("expected workspace file request");
        };
        assert_eq!(method, "workspace.file.read");
        assert_eq!(
            params,
            Some(json!({
                "workspaceId": "workspace_test",
                "path": "src/main.rs"
            }))
        );
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": id,
                    "ok": true,
                    "result": {
                        "workspaceId": "workspace_test",
                        "path": "src/main.rs",
                        "content": "fn main() {}\n",
                        "byteLength": 13
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let status: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id, method, params, ..
        } = status
        else {
            panic!("expected workspace Git status request");
        };
        assert_eq!(method, "workspace.git.status");
        assert_eq!(params, Some(json!({ "workspaceId": "workspace_test" })));
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": id,
                    "ok": true,
                    "result": {
                        "workspaceId": "workspace_test",
                        "isRepository": true,
                        "branch": "main",
                        "changes": [{
                            "path": "src/main.rs",
                            "indexStatus": null,
                            "worktreeStatus": "modified"
                        }]
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let diff: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id, method, params, ..
        } = diff
        else {
            panic!("expected workspace Git diff request");
        };
        assert_eq!(method, "workspace.git.diff");
        assert_eq!(
            params,
            Some(json!({
                "workspaceId": "workspace_test",
                "path": "src/main.rs"
            }))
        );
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": id,
                    "ok": true,
                    "result": {
                        "workspaceId": "workspace_test",
                        "path": "src/main.rs",
                        "unifiedDiff": "@@ -1 +1 @@\n-fn main() {}\n+fn main() { println!(\"Corbit\"); }\n",
                        "byteLength": 61,
                        "isBinary": false
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let connection = client.connect().await.unwrap();
    let listing = connection
        .list_workspace_files("workspace_test", "src")
        .await
        .unwrap();
    assert_eq!(listing.entries.len(), 2);
    assert_eq!(listing.entries[0].name, "components");

    let content = connection
        .read_workspace_file("workspace_test", "src/main.rs")
        .await
        .unwrap();
    assert_eq!(content.content, "fn main() {}\n");
    assert_eq!(content.byte_length, 13);

    let status = connection
        .workspace_git_status("workspace_test")
        .await
        .unwrap();
    assert!(status.is_repository);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.changes.len(), 1);
    assert_eq!(
        status.changes[0].worktree_status,
        Some(corbit_protocol::WorkspaceGitChangeKind::Modified)
    );

    let diff = connection
        .workspace_git_diff("workspace_test", "src/main.rs")
        .await
        .unwrap();
    assert!(diff.unified_diff.contains("println!"));
    assert_eq!(diff.byte_length, 61);
    assert!(!diff.is_binary);
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn submits_a_prompt_and_routes_timeline_events_independently_of_the_rpc_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_prompt").await;

        let request: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id, method, params, ..
        } = request
        else {
            panic!("expected prompt RPC request");
        };
        assert_eq!(method, "agent.prompt");
        assert_eq!(
            params,
            Some(json!({
                "agentId": "agent_test",
                "text": "Summarize the workspace",
                "clientMutationId": "prompt_test"
            }))
        );

        for message in [
            json!({
                "type": "event",
                "topic": "agent.timeline",
                "sequence": 1,
                "payload": {
                    "agentId": "agent_test",
                    "event": {
                        "kind": "turn.started",
                        "turnId": "turn_test",
                        "prompt": "Summarize the workspace",
                        "occurredAt": "2026-08-14T00:00:00.000Z"
                    }
                }
            }),
            json!({
                "type": "event",
                "topic": "agent.timeline",
                "sequence": 2,
                "payload": {
                    "agentId": "agent_test",
                    "event": {
                        "kind": "assistant.delta",
                        "turnId": "turn_test",
                        "itemId": "item_test",
                        "delta": "Corbit",
                        "occurredAt": "2026-08-14T00:00:01.000Z"
                    }
                }
            }),
            json!({
                "type": "event",
                "topic": "agent.timeline",
                "sequence": 3,
                "payload": {
                    "agentId": "agent_test",
                    "event": {
                        "kind": "turn.completed",
                        "turnId": "turn_test",
                        "status": "completed",
                        "occurredAt": "2026-08-14T00:00:02.000Z"
                    }
                }
            }),
            json!({
                "type": "rpc.response",
                "id": id,
                "ok": true,
                "result": {
                    "agentId": "agent_test",
                    "turnId": "turn_test",
                    "clientMutationId": "prompt_test"
                }
            }),
        ] {
            socket
                .send(Message::text(message.to_string()))
                .await
                .unwrap();
        }

        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let mut events = client.subscribe();
    let connection = client.connect().await.unwrap();
    for _ in 0..4 {
        let _ = events.recv().await.unwrap();
    }

    let acknowledgement = connection
        .prompt("agent_test", "Summarize the workspace", "prompt_test")
        .await
        .unwrap();
    assert_eq!(acknowledgement.agent_id, "agent_test");
    assert_eq!(acknowledgement.turn_id, "turn_test");

    let mut timeline = Vec::new();
    while timeline.len() < 3 {
        if let ConnectionEvent::AgentTimeline { sequence, payload } = events.recv().await.unwrap() {
            timeline.push((sequence, payload));
        }
    }
    assert_eq!(timeline[0].0, 1);
    assert!(matches!(
        timeline[0].1.event,
        AgentTimelineEvent::TurnStarted { ref prompt, .. }
            if prompt == "Summarize the workspace"
    ));
    assert!(matches!(
        timeline[1].1.event,
        AgentTimelineEvent::AssistantDelta { ref delta, .. } if delta == "Corbit"
    ));
    assert!(matches!(
        timeline[2].1.event,
        AgentTimelineEvent::TurnCompleted {
            status: AgentTurnStatus::Completed,
            ..
        }
    ));

    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn resumes_the_authoritative_timeline_from_the_last_committed_sequence() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for (attempt, latest_sequence) in [(1_u32, 1_u64), (2, 2)] {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let hello: ClientMessage = read_json(&mut socket).await;
            let ClientMessage::Hello { resume, .. } = hello else {
                panic!("expected hello");
            };
            let resume = resume.expect("desktop client should request event recovery");
            assert_eq!(resume.last_sequence, Some(u64::from(attempt - 1)));
            assert_eq!(
                resume.server_id.as_deref(),
                (attempt == 2).then_some("srv_recovery")
            );

            socket
                .send(Message::text(
                    json!({
                        "type": "server_info",
                        "sessionId": format!("session_{attempt}"),
                        "serverId": "srv_recovery",
                        "version": "0.1.0",
                        "protocolVersion": PROTOCOL_VERSION,
                        "features": { "eventCursorRecovery": true }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::text(
                    json!({
                        "type": "event_sync",
                        "phase": "begin",
                        "requestedAfter": attempt - 1,
                        "replayFrom": attempt - 1,
                        "latestSequence": latest_sequence,
                        "reset": false
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::text(
                    json!({
                        "type": "event",
                        "topic": "agent.timeline",
                        "sequence": latest_sequence,
                        "payload": {
                            "agentId": "agent_recovery",
                            "event": if attempt == 1 {
                                json!({
                                    "kind": "turn.started",
                                    "turnId": "turn_recovery",
                                    "prompt": "Recover this turn",
                                    "occurredAt": "2026-08-14T00:00:00.000Z"
                                })
                            } else {
                                json!({
                                    "kind": "turn.completed",
                                    "turnId": "turn_recovery",
                                    "status": "completed",
                                    "occurredAt": "2026-08-14T00:00:01.000Z"
                                })
                            }
                        }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::text(
                    json!({
                        "type": "event_sync",
                        "phase": "complete",
                        "requestedAfter": attempt - 1,
                        "replayFrom": attempt - 1,
                        "latestSequence": latest_sequence,
                        "reset": false
                    })
                    .to_string(),
                ))
                .await
                .unwrap();

            let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
        }
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let mut events = client.subscribe();

    let first = client.connect().await.unwrap();
    assert_eq!(next_timeline_sequence(&mut events).await, 1);
    first.close().await.unwrap();

    let second = client.connect().await.unwrap();
    assert_eq!(next_timeline_sequence(&mut events).await, 2);
    second.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn times_out_one_rpc_and_ignores_its_late_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_timeout").await;

        let first: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest { id: first_id, .. } = first else {
            panic!("expected first RPC request");
        };
        tokio::time::sleep(Duration::from_millis(80)).await;
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": first_id,
                    "ok": true,
                    "result": "late"
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let second: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest { id: second_id, .. } = second else {
            panic!("expected second RPC request");
        };
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": second_id,
                    "ok": true,
                    "result": "current"
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let mut config = ClientConfig::desktop(endpoint, TOKEN).unwrap();
    config.rpc_timeout = Duration::from_millis(30);
    let client = CorbitClient::new(config).unwrap();
    let connection = client.connect().await.unwrap();

    assert!(matches!(
        connection.echo(json!({ "request": "timeout" })).await,
        Err(ClientError::Timeout {
            operation: "waiting for RPC response"
        })
    ));
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        connection
            .echo(json!({ "request": "current" }))
            .await
            .unwrap(),
        json!("current")
    );
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn dropping_an_rpc_future_cancels_only_its_local_wait() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        complete_handshake(&mut socket, "session_cancel").await;

        let abandoned: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest {
            id: abandoned_id, ..
        } = abandoned
        else {
            panic!("expected abandoned RPC request");
        };
        tokio::time::sleep(Duration::from_millis(40)).await;
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": abandoned_id,
                    "ok": true,
                    "result": "abandoned"
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let live: ClientMessage = read_json(&mut socket).await;
        let ClientMessage::RpcRequest { id: live_id, .. } = live else {
            panic!("expected live RPC request");
        };
        socket
            .send(Message::text(
                json!({
                    "type": "rpc.response",
                    "id": live_id,
                    "ok": true,
                    "result": "live"
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    });

    let client = CorbitClient::new(ClientConfig::desktop(endpoint, TOKEN).unwrap()).unwrap();
    let connection = client.connect().await.unwrap();
    let abandoned = {
        let connection = connection.clone();
        tokio::spawn(async move { connection.echo(json!({ "request": "abandoned" })).await })
    };
    tokio::time::sleep(Duration::from_millis(10)).await;
    abandoned.abort();
    assert!(abandoned.await.unwrap_err().is_cancelled());
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        connection.echo(json!({ "request": "live" })).await.unwrap(),
        json!("live")
    );
    connection.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn runtime_reconnects_and_routes_rpc_to_the_replacement_session() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for attempt in 1_u32..=2 {
            let (health_stream, _) = listener.accept().await.unwrap();
            serve_http(health_stream, "GET /health ", None, r#"{"status":"ok"}"#).await;

            let (websocket_stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(websocket_stream).await.unwrap();
            complete_handshake(&mut socket, &format!("session_{attempt}")).await;

            serve_snapshot(&mut socket, u64::from(attempt)).await;

            if attempt == 1 {
                socket
                    .close(Some(CloseFrame {
                        code: CloseCode::Away,
                        reason: "test reconnect".into(),
                    }))
                    .await
                    .unwrap();
                continue;
            }

            let request: ClientMessage = read_json(&mut socket).await;
            let ClientMessage::RpcRequest { id, params, .. } = request else {
                panic!("expected RPC on replacement session");
            };
            socket
                .send(Message::text(
                    json!({
                        "type": "rpc.response",
                        "id": id,
                        "ok": true,
                        "result": { "echo": params }
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
        }
    });

    let mut config = ClientConfig::desktop(endpoint, TOKEN).unwrap();
    config.reconnect_initial_delay = Duration::from_millis(10);
    config.reconnect_max_delay = Duration::from_millis(20);
    config.heartbeat_interval = Duration::from_mins(1);
    let runtime = DaemonRuntime::spawn(config).unwrap();
    let events = runtime.events();

    let mut snapshot_revisions = Vec::new();
    let mut saw_reconnecting = false;
    tokio::time::timeout(Duration::from_secs(2), async {
        while snapshot_revisions.len() < 2 {
            match events.recv().await.unwrap() {
                RuntimeEvent::Snapshot(snapshot) => snapshot_revisions.push(snapshot.revision),
                RuntimeEvent::Connection(ConnectionEvent::StateChanged(
                    ConnectionState::Reconnecting { attempt, .. },
                )) => {
                    assert_eq!(attempt, 1);
                    saw_reconnecting = true;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("runtime should reconnect");
    assert!(saw_reconnecting);
    assert_eq!(snapshot_revisions, [1, 2]);
    assert_eq!(
        runtime
            .rpc("system.echo", Some(json!({ "after": "reconnect" })))
            .await
            .unwrap(),
        json!({ "echo": { "after": "reconnect" } })
    );

    drop(runtime);
    server.await.unwrap();
}

async fn serve_complete_flow(listener: TcpListener) {
    let (health_stream, _) = listener.accept().await.unwrap();
    serve_http(health_stream, "GET /health ", None, r#"{"status":"ok"}"#).await;

    let (info_stream, _) = listener.accept().await.unwrap();
    let info_body = json!({
        "serverId": "srv_test",
        "version": "0.1.0",
        "protocolVersion": PROTOCOL_VERSION,
        "features": { "heartbeat": true, "systemEcho": true }
    })
    .to_string();
    serve_http(info_stream, "GET /info ", Some(TOKEN), &info_body).await;

    let (websocket_stream, _) = listener.accept().await.unwrap();
    #[allow(clippy::result_large_err)]
    let mut socket = accept_hdr_async(websocket_stream, |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/ws");
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            &format!("Bearer {TOKEN}")
        );
        Ok(response)
    })
    .await
    .unwrap();

    let hello: ClientMessage = read_json(&mut socket).await;
    assert!(matches!(
        hello,
        ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            ..
        }
    ));
    socket
        .send(Message::text(
            json!({
                "type": "server_info",
                "sessionId": "session_test",
                "serverId": "srv_test",
                "version": "0.1.0",
                "protocolVersion": PROTOCOL_VERSION,
                "features": { "heartbeat": true, "systemEcho": true }
            })
            .to_string(),
        ))
        .await
        .unwrap();

    let ping: ClientMessage = read_json(&mut socket).await;
    let ClientMessage::Ping { ping_id, .. } = ping else {
        panic!("expected ping");
    };
    socket
        .send(Message::text(
            json!({
                "type": "pong",
                "pingId": ping_id,
                "serverTime": "2026-08-14T00:00:00.000Z"
            })
            .to_string(),
        ))
        .await
        .unwrap();

    let request: ClientMessage = read_json(&mut socket).await;
    let ClientMessage::RpcRequest { id, params, .. } = request else {
        panic!("expected RPC request");
    };
    socket
        .send(Message::text(
            json!({
                "type": "rpc.response",
                "id": id,
                "ok": true,
                "result": { "echo": params }
            })
            .to_string(),
        ))
        .await
        .unwrap();

    serve_snapshot(&mut socket, 0).await;

    let _ = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
}

async fn complete_handshake<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, session_id: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let hello: ClientMessage = read_json(socket).await;
    assert!(matches!(hello, ClientMessage::Hello { .. }));
    socket
        .send(Message::text(
            json!({
                "type": "server_info",
                "sessionId": session_id,
                "serverId": "srv_test",
                "version": "0.1.0",
                "protocolVersion": PROTOCOL_VERSION,
                "features": { "heartbeat": true, "systemEcho": true }
            })
            .to_string(),
        ))
        .await
        .unwrap();
}

async fn serve_snapshot<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, revision: u64)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: ClientMessage = read_json(socket).await;
    let ClientMessage::RpcRequest { id, method, .. } = request else {
        panic!("expected state snapshot request");
    };
    assert_eq!(method, "state.snapshot");
    socket
        .send(Message::text(
            json!({
                "type": "rpc.response",
                "id": id,
                "ok": true,
                "result": {
                    "schemaVersion": 1,
                    "generatedAt": "2026-08-14T00:00:00.000Z",
                    "revision": revision,
                    "projects": [],
                    "workspaces": [],
                    "agents": []
                }
            })
            .to_string(),
        ))
        .await
        .unwrap();
}

fn empty_snapshot() -> AuthoritativeSnapshot {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "generatedAt": "2026-08-14T00:00:00.000Z",
        "revision": 0,
        "projects": [],
        "workspaces": [],
        "agents": []
    }))
    .unwrap()
}

async fn serve_http(mut stream: TcpStream, route: &str, token: Option<&str>, body: &str) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0, "HTTP request ended before its headers");
        request.extend_from_slice(&buffer[..count]);
        assert!(
            request.len() < 16 * 1_024,
            "HTTP request headers are too large"
        );
    }

    let request = String::from_utf8(request).unwrap();
    assert!(
        request.starts_with(route),
        "unexpected HTTP request: {request}"
    );
    if let Some(token) = token {
        assert!(request.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            token.to_ascii_lowercase()
        )));
    }

    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
}

async fn read_json<S, T>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> T
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    T: serde::de::DeserializeOwned,
{
    let frame = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = frame else {
        panic!("expected a text frame");
    };
    serde_json::from_str(text.as_ref()).unwrap()
}

async fn next_timeline_sequence(
    events: &mut tokio::sync::broadcast::Receiver<ConnectionEvent>,
) -> u64 {
    loop {
        if let ConnectionEvent::AgentTimeline { sequence, .. } = events.recv().await.unwrap() {
            return sequence;
        }
    }
}
