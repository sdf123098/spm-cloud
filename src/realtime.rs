use axum::{extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State}, http::HeaderMap, response::{IntoResponse, Response}, routing::get, Router};
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use prost::Message as ProstMessage;

use crate::{api::{authenticate, AppState, CloudEvent}, error::CloudError, protocol::{generated::{AppearanceState, Envelope, Error as ProtoError, Heartbeat, HeartbeatAck, Hello, HelloAck, JoinScope, TargetEntry, TargetSnapshot}, HEARTBEAT_INTERVAL_SECONDS, MAX_MESSAGE_BYTES, PROTOCOL_V1}};

pub fn router(state: AppState) -> Router {
    Router::new().route("/v1/realtime", get(upgrade)).with_state(state)
}

type SocketSink = SplitSink<WebSocket, Message>;

async fn upgrade(State(state): State<AppState>, headers: HeaderMap, websocket: WebSocketUpgrade) -> Result<Response, CloudError> {
    let account_id = authenticate(&state, &headers)?;
    Ok(websocket
        .read_buffer_size(state.config.max_message_bytes)
        .max_write_buffer_size(state.config.max_message_bytes * 2)
        .on_upgrade(move |socket| serve(socket, state, account_id))
        .into_response())
}

async fn serve(mut socket: WebSocket, state: AppState, account_id: String) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.events.subscribe();
    let mut joined_scope: Option<String> = None;
    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(result) = incoming else { break; };
                let Ok(message) = result else { break; };
                let Message::Binary(bytes) = message else { continue; };
                if bytes.len() > MAX_MESSAGE_BYTES { let _ = send_error(&mut sender, "", "MESSAGE_TOO_LARGE", false).await; break; }
                let envelope = match Envelope::decode(bytes) {
                    Ok(envelope) => envelope,
                    Err(_) => { let _ = send_error(&mut sender, "", "MALFORMED_MESSAGE", false).await; continue; }
                };
                if envelope.protocol_version != PROTOCOL_V1 { let _ = send_error(&mut sender, &envelope.request_id, "PROTOCOL_UNSUPPORTED", false).await; continue; }
                let join_result = match envelope.kind.as_str() {
                    "Hello" => handle_hello(&mut sender, &state, &envelope).await.map(|_| None),
                    "Heartbeat" => handle_heartbeat(&mut sender, &envelope).await.map(|_| None),
                    "JoinScope" => handle_join_scope(&mut sender, &state, &account_id, &envelope).await.map(Some),
                    _ => send_error(&mut sender, &envelope.request_id, "MALFORMED_MESSAGE", false).await.map(|_| None),
                };
                if let Ok(scope) = join_result { if scope.is_some() { joined_scope = scope; } }
                else { break; }
            }
            event = events.recv() => {
                match event {
                    Ok(event) if joined_scope.as_deref() == Some(event.scope_id.as_str()) => {
                        if send_appearance_event(&mut sender, &event).await.is_err() { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if send_error(&mut sender, "", "CATALOG_SNAPSHOT_REQUIRED", true).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn handle_join_scope(socket: &mut SocketSink, state: &AppState, account_id: &str, envelope: &Envelope) -> Result<String, ()> {
    let join = JoinScope::decode(envelope.payload.as_ref()).map_err(|_| ())?;
    let scope_id = join.scope_id.clone();
    let targets = match state.store.join_scope(account_id, &join.scope_id, &join.world_epoch) {
        Ok(targets) => targets,
        Err(error) => {
            let _ = send_error(socket, &envelope.request_id, error.code(), false).await;
            return Err(());
        }
    };
    let snapshot = TargetSnapshot {
        snapshot_id: format!("snapshot_{}", uuid::Uuid::new_v4().simple()),
        targets: targets.into_iter().map(|target| TargetEntry {
            target_id: target.target_id,
            kind: serde_json::to_string(&target.kind).unwrap_or_default().trim_matches('"').to_owned(),
            display_name: target.display_name,
            revision: target.revision,
        }).collect(),
    };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "TargetSnapshot".to_owned(), request_id: envelope.request_id.clone(), event_id: String::new(), scope_id: scope_id.clone(), payload: snapshot.encode_to_vec().into() }).await?;
    Ok(scope_id)
}

async fn send_appearance_event(socket: &mut SocketSink, event: &CloudEvent) -> Result<(), ()> {
    let state = &event.appearance;
    let payload = AppearanceState {
        target_id: state.target_id.clone(),
        revision: state.revision,
        asset_id: state.asset_id.clone().unwrap_or_default(),
        asset_revision: state.asset_revision.unwrap_or_default(),
        raw_sha256: state.raw_sha256.clone().unwrap_or_default(),
        texture_id: state.texture_id.clone().unwrap_or_default(),
        scale: state.scale.unwrap_or_default(),
        disabled: state.disabled,
    };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "AppearanceState".to_owned(), request_id: String::new(), event_id: event.event_id.clone(), scope_id: event.scope_id.clone(), payload: payload.encode_to_vec().into() }).await
}

async fn handle_hello(socket: &mut SocketSink, state: &AppState, envelope: &Envelope) -> Result<(), ()> {
    let hello = Hello::decode(envelope.payload.as_ref()).map_err(|_| ())?;
    if hello.instance_id != state.config.instance_id || !hello.protocol_versions.iter().any(|p| p == PROTOCOL_V1) { return send_error(socket, &envelope.request_id, "INSTANCE_MISMATCH", false).await; }
    let payload = HelloAck { instance_id: state.config.instance_id.clone(), protocol_version: PROTOCOL_V1.to_owned(), max_message_bytes: state.config.max_message_bytes as u64, heartbeat_interval_seconds: HEARTBEAT_INTERVAL_SECONDS, heartbeat_ttl_seconds: HEARTBEAT_TTL_SECONDS };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "HelloAck".to_owned(), request_id: envelope.request_id.clone(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn handle_heartbeat(socket: &mut SocketSink, envelope: &Envelope) -> Result<(), ()> {
    let _ = Heartbeat::decode(envelope.payload.as_ref()).map_err(|_| ())?;
    let payload = HeartbeatAck { server_time_unix_ms: crate::api::now_unix_ms() };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "HeartbeatAck".to_owned(), request_id: envelope.request_id.clone(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn send_error(socket: &mut SocketSink, request_id: &str, code: &str, retryable: bool) -> Result<(), ()> {
    let payload = ProtoError { code: code.to_owned(), retryable, message_key: format!("cloud.error.{}", code.to_lowercase()), details: String::new() };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "Error".to_owned(), request_id: request_id.to_owned(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn send_envelope(socket: &mut SocketSink, envelope: Envelope) -> Result<(), ()> {
    let bytes = envelope.encode_to_vec();
    if bytes.len() > MAX_MESSAGE_BYTES { return Err(()); }
    socket.send(Message::Binary(bytes.into())).await.map_err(|_| ())
}
