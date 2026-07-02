use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub struct LiveWsQuery {
    pub token: Option<String>,
}

pub async fn live_ws_handler(
    State(state): State<AppState>,
    Query(query): Query<LiveWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let token = query.token.or_else(|| {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    match token {
        Some(t) => ws
            .on_upgrade(move |socket| handle_live_connection(state, socket, t))
            .into_response(),
        None => ws
            .on_upgrade(|socket| async {
                let mut socket = socket;
                let _ = socket
                    .send(Message::Text(
                        r#"{"error":"Missing authentication token"}"#.to_string(),
                    ))
                    .await;
                let _ = socket.close().await;
            })
            .into_response(),
    }
}

async fn handle_live_connection(state: AppState, mut socket: WebSocket, token: String) {
    // Authenticate: try API key first, then JWT
    let auth_result = if token.starts_with("mcpgw_") {
        crate::api::auth::resolve_api_key(&token, &state).await
    } else {
        // JWT path
        jsonwebtoken::decode::<crate::api::auth::Claims>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(state.jwt_secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|_| crate::AppError::Unauthorized("Invalid or expired token".into()))
    };

    if let Err(_) = auth_result {
        let _ = socket
            .send(Message::Text(r#"{"error":"unauthorized"}"#.into()))
            .await;
        let _ = socket.close().await;
        return;
    }

    let mut rx = state.event_tx.subscribe();

    // Send a connected acknowledgement
    let _ = socket
        .send(Message::Text(r#"{"type":"connected"}"#.into()))
        .await;

    loop {
        tokio::select! {
            // Incoming broadcast event → forward to client
            result = rx.recv() => {
                match result {
                    Ok(json) => {
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Live WS client lagged, skipping events");
                        // Continue — don't disconnect, just skip
                    }
                    Err(_) => break,
                }
            }

            // Client closed or sent a message (we only expect pings/close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
