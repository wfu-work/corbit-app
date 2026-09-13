use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use corbit_client::{ClientConfig, CorbitClient, RelayConfig};
use futures_util::{SinkExt, StreamExt};
use ring::signature::{KeyPair, UnparsedPublicKey};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use url::Url;

#[tokio::test]
async fn tunnels_corbit_protocol_through_relay_v1_and_ignores_control_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let rng = ring::rand::SystemRandom::new();
    let private = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(private.as_ref()).unwrap();
    let public_key = URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref());
    let private_key = URL_SAFE_NO_PAD.encode(private.as_ref());
    let relay_config = RelayConfig {
        url: Url::parse(&format!("ws://{address}/v1/connect")).unwrap(),
        space_id: "space-integration".into(),
        endpoint_id: "app-integration".into(),
        endpoint_type: "app".into(),
        connect_token: "short-lived-connect-token".into(),
        endpoint_public_key: public_key.clone(),
        endpoint_private_key: private_key,
        endpoint_name: Some("Integration App".into()),
        target_endpoint_id: Some("bridge-integration".into()),
    };

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();

        let hello = read_json(&mut socket).await;
        assert_eq!(hello["type"], "connect.hello");
        assert_eq!(hello["spaceId"], "space-integration");
        assert_eq!(hello["endpointId"], "app-integration");
        assert_eq!(hello["endpointType"], "app");
        assert_eq!(hello["endpointProof"]["publicKey"], public_key);
        let canonical = format!(
            "relay-connect-v1\n1\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            hello["requestId"].as_str().unwrap(),
            hello["spaceId"].as_str().unwrap(),
            hello["endpointId"].as_str().unwrap(),
            hello["endpointType"].as_str().unwrap(),
            hello["token"].as_str().unwrap(),
            hello["endpointProof"]["issuedAt"].as_i64().unwrap(),
            hello["endpointProof"]["nonce"].as_str().unwrap(),
        );
        let public = URL_SAFE_NO_PAD
            .decode(hello["endpointProof"]["publicKey"].as_str().unwrap())
            .unwrap();
        let signature = URL_SAFE_NO_PAD
            .decode(hello["endpointProof"]["signature"].as_str().unwrap())
            .unwrap();
        UnparsedPublicKey::new(&ring::signature::ED25519, public)
            .verify(canonical.as_bytes(), &signature)
            .unwrap();

        socket
            .send(Message::text(
                json!({
                    "version": 1,
                    "type": "connect.welcome",
                    "requestId": hello["requestId"],
                    "connectionId": "connection-integration",
                    "sessionId": "session-integration",
                    "spaceId": "space-integration",
                    "endpointId": "app-integration",
                    "maxFrameSize": 1048576,
                    "features": ["streams", "ack"]
                })
                .to_string(),
            ))
            .await
            .unwrap();

        // Corbit's own hello is the first product payload after the Relay
        // handshake. It must remain opaque to the Relay adapter.
        let corbit_hello = read_json(&mut socket).await;
        assert_eq!(corbit_hello["type"], "stream.message");
        assert_eq!(corbit_hello["protocol"], "corbit.v1");
        assert_eq!(corbit_hello["to"], "bridge-integration");
        assert_eq!(corbit_hello["payload"]["type"], "hello");

        // These transport frames are deliberately sent before the daemon
        // payload. The adapter must consume them without closing the session.
        for control in [
            json!({"version": 1, "type": "stream.open", "streamId": "corbit:bridge-integration", "protocol": "corbit.v1"}),
            json!({"version": 1, "type": "stream.ack", "streamId": "corbit:app-integration", "ack": 1}),
            json!({"version": 1, "type": "pong"}),
            json!({"version": 1, "type": "relay.peer_joined", "spaceId": "space-integration", "endpointId": "bridge-integration"}),
            json!({"version": 1, "type": "stream.message", "streamId": "relay-control", "protocol": "relay.v1", "payload": {"type": "relay.ready"}}),
        ] {
            socket
                .send(Message::text(control.to_string()))
                .await
                .unwrap();
        }
        socket
            .send(Message::text(
                json!({
                    "version": 1,
                    "type": "stream.message",
                    "streamId": "corbit:bridge-integration",
                    "protocol": "corbit.v1",
                    "from": "bridge-integration",
                    "payload": {
                        "type": "server_info",
                        "sessionId": "corbit-session",
                        "serverId": "daemon-integration",
                        "version": "0.1.0",
                        "protocolVersion": 1,
                        "features": {"heartbeat": true}
                    }
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let ping = read_json(&mut socket).await;
        assert_eq!(ping["type"], "stream.message");
        assert_eq!(ping["protocol"], "corbit.v1");
        assert_eq!(ping["payload"]["type"], "ping");
        socket
            .send(Message::text(
                json!({"version": 1, "type": "stream.ack", "streamId": ping["streamId"], "ack": 0})
                    .to_string(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::text(
                json!({"version": 1, "type": "pong"}).to_string(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::text(
                json!({
                    "version": 1,
                    "type": "stream.message",
                    "streamId": "corbit:bridge-integration",
                    "protocol": "corbit.v1",
                    "payload": {"type": "pong", "pingId": ping["payload"]["pingId"], "serverTime": "2026-08-30T00:00:00.000Z"}
                })
                .to_string(),
            ))
            .await
            .unwrap();

        let _ = tokio::time::timeout(Duration::from_secs(2), socket.next()).await;
    });

    let client = CorbitClient::new(
        ClientConfig::desktop(
            "http://127.0.0.1:1",
            "daemon-token-that-is-at-least-32-characters",
        )
        .unwrap()
        .with_relay(relay_config),
    )
    .unwrap();
    let connection = client.connect().await.unwrap();
    assert_eq!(connection.session_id(), "corbit-session");
    connection.ping().await.unwrap();
    connection.close().await.unwrap();
    server.await.unwrap();
}

async fn read_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let frame = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = frame else {
        panic!("expected a text frame");
    };
    serde_json::from_str(text.as_ref()).unwrap()
}
