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
    // Authenticate through the same path every REST request uses, so a
    // deactivated account or a role change takes effect here too. A bare
    // `decode` used to be enough to open this socket, and a socket is opened
    // once and then held, so a revoked user kept streaming for the token's
    // remaining lifetime.
    let Ok((claims, must_change_password)) = crate::api::auth::resolve_bearer(&token, &state).await
    else {
        let _ = socket
            .send(Message::Text(r#"{"error":"unauthorized"}"#.into()))
            .await;
        let _ = socket.close().await;
        return;
    };

    // A first-login account has not finished authenticating yet; it may change
    // its own password and nothing else.
    if must_change_password {
        let _ = socket
            .send(Message::Text(
                r#"{"error":"password change required"}"#.into(),
            ))
            .await;
        let _ = socket.close().await;
        return;
    }

    // The channel carries every user's calls, so each subscriber is filtered to
    // what it is allowed to see. Without this, any authenticated account read
    // the whole deployment's tool names, backends and error messages in real
    // time — data the REST audit endpoint refuses it — and the Usage page
    // counted other people's calls into a graph the server had scoped to one
    // user, so the figure on screen drifted upward until the next refresh.
    let is_owner = claims.roles.iter().any(|r| r == "owner");
    let own_id: Option<uuid::Uuid> = claims.sub.parse().ok();

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
                    Ok(event) => {
                        if !is_owner && event.user_id != own_id {
                            continue;
                        }
                        if socket.send(Message::Text(event.json)).await.is_err() {
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
