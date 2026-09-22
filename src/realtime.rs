use axum::{extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State}, http::HeaderMap, response::{IntoResponse, Response}, routing::get, Router};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;

use crate::{api::{authenticate, AppState}, protocol::{generated::{Envelope, Error as ProtoError, Heartbeat, HeartbeatAck, Hello, HelloAck}, HEARTBEAT_INTERVAL_SECONDS, MAX_MESSAGE_BYTES, PROTOCOL_V1}};

pub fn router(state: AppState) -> Router {
    Router::new().route("/v1/realtime", get(upgrade)).with_state(state)
}

async fn upgrade(State(state): State<AppState>, headers: HeaderMap, websocket: WebSocketUpgrade) -> Result<Response, CloudError> {
    authenticate(&state, &headers)?;
    Ok(websocket
        .read_buffer_size(state.config.max_message_bytes)
        .max_write_buffer_size(state.config.max_message_bytes * 2)
        .on_upgrade(move |socket| serve(socket, state))
        .into_response())
}

async fn serve(mut socket: WebSocket, state: AppState) {
    while let Some(result) = socket.next().await {
        let Ok(message) = result else { break; };
        let Message::Binary(bytes) = message else { continue; };
        if bytes.len() > MAX_MESSAGE_BYTES { let _ = send_error(&mut socket, "", "MESSAGE_TOO_LARGE", false).await; break; }
        let envelope = match Envelope::decode(bytes) {
            Ok(envelope) => envelope,
            Err(_) => { let _ = send_error(&mut socket, "", "MALFORMED_MESSAGE", false).await; continue; }
        };
        if envelope.protocol_version != PROTOCOL_V1 { let _ = send_error(&mut socket, &envelope.request_id, "PROTOCOL_UNSUPPORTED", false).await; continue; }
        let send_result = match envelope.kind.as_str() {
            "Hello" => handle_hello(&mut socket, &state, &envelope).await,
            "Heartbeat" => handle_heartbeat(&mut socket, &envelope).await,
            _ => send_error(&mut socket, &envelope.request_id, "MALFORMED_MESSAGE", false).await,
        };
        if send_result.is_err() { break; }
    }
}

async fn handle_hello(socket: &mut WebSocket, state: &AppState, envelope: &Envelope) -> Result<(), ()> {
    let hello = Hello::decode(envelope.payload.as_ref()).map_err(|_| ())?;
    if hello.instance_id != state.config.instance_id || !hello.protocol_versions.iter().any(|p| p == PROTOCOL_V1) { return send_error(socket, &envelope.request_id, "INSTANCE_MISMATCH", false).await; }
    let payload = HelloAck { instance_id: state.config.instance_id.clone(), protocol_version: PROTOCOL_V1.to_owned(), max_message_bytes: state.config.max_message_bytes as u64, heartbeat_interval_seconds: HEARTBEAT_INTERVAL_SECONDS, heartbeat_ttl_seconds: HEARTBEAT_TTL_SECONDS };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "HelloAck".to_owned(), request_id: envelope.request_id.clone(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn handle_heartbeat(socket: &mut WebSocket, envelope: &Envelope) -> Result<(), ()> {
    let _ = Heartbeat::decode(envelope.payload.as_ref()).map_err(|_| ())?;
    let payload = HeartbeatAck { server_time_unix_ms: crate::api::now_unix_ms() };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "HeartbeatAck".to_owned(), request_id: envelope.request_id.clone(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn send_error(socket: &mut WebSocket, request_id: &str, code: &str, retryable: bool) -> Result<(), ()> {
    let payload = ProtoError { code: code.to_owned(), retryable, message_key: format!("cloud.error.{}", code.to_lowercase()), details: String::new() };
    send_envelope(socket, Envelope { protocol_version: PROTOCOL_V1.to_owned(), kind: "Error".to_owned(), request_id: request_id.to_owned(), event_id: String::new(), scope_id: String::new(), payload: payload.encode_to_vec().into() }).await
}

async fn send_envelope(socket: &mut WebSocket, envelope: Envelope) -> Result<(), ()> {
    let bytes = envelope.encode_to_vec();
    if bytes.len() > MAX_MESSAGE_BYTES { return Err(()); }
    socket.send(Message::Binary(bytes.into())).await.map_err(|_| ())
}
