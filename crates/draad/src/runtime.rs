//! axum-side runtime the codegen targets. Enabled by the `runtime`
//! feature.
//!
//! Wire format (matches the TypeScript `defaultRpc` shipped in the
//! generated `index.ts`):
//!
//!  - Calls: HTTP 200 + JSON body on success; HTTP 500 + plain-text body
//!    on failure.
//!  - Events: WS frames shaped `{ topic: string, payload: T }`, broadcast
//!    to every connected client.
//!
//! If you need a different shape (richer errors, per-client filtering,
//! ack/replay), write your own `Response` / `EventBus` / WS handler and
//! point the codegen at them.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as AxumResponse};
use axum::Json;
use serde::Serialize;
use tokio::sync::broadcast;

// ──────────────────────────────────────────────────────────────────────
// RPC response shim
// ──────────────────────────────────────────────────────────────────────

/// What every generated handler returns. `Ok(T)` → JSON 200; `Err(msg)` →
/// plain-text 500.
pub struct Response<T>(pub Result<T, String>);

/// Lifts a `Result<T, E: Display>` into [`Response<T>`]. The codegen
/// emits a call to this for trait methods whose return type is `Result`.
pub fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> Response<T> {
    Response(r.map_err(|e| e.to_string()))
}

impl<T: Serialize> IntoResponse for Response<T> {
    fn into_response(self) -> AxumResponse {
        match self.0 {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into_response(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Event bus + WebSocket handler
// ──────────────────────────────────────────────────────────────────────

/// Backing channel for the generated `*Emitter` types. Each emit
/// serialises to a `{ topic, payload }` JSON frame and fans out to every
/// subscriber. Cheap to clone, internally a [`tokio::sync::broadcast::Sender`].
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<String>,
}

impl EventBus {
    /// New bus with a default buffer of 256 frames per subscriber. If a
    /// slow client lags behind the buffer it gets a `Lagged` error on
    /// `recv`; [`ws_handler`] silently skips those.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    /// Subscribe a new receiver. Typically called once per WS connection.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// Publish a `{ topic, payload }` frame to all subscribers. No-op if
    /// nobody is currently connected.
    pub fn publish<T: ?Sized + Serialize>(&self, topic: &str, payload: &T) {
        let frame = serde_json::json!({ "topic": topic, "payload": payload });
        let _ = self.sender.send(frame.to_string());
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Default axum WebSocket handler: subscribes to the `EventBus` and
/// forwards every published frame to the client. Mount it like:
///
/// ```ignore
/// use axum::{Router, routing::get};
/// use axum::extract::FromRef;
///
/// #[derive(Clone, FromRef)]
/// struct AppContext { bus: draad::runtime::EventBus, /* ... */ }
///
/// Router::new()
///     .route("/ws", get(draad::runtime::ws_handler))
///     .with_state(AppContext { /* ... */ });
/// ```
///
/// The `FromRef` derive lets axum extract `State<EventBus>` from your
/// composite state. Replace this handler with your own when you need
/// auth, per-client subscriptions, presence, etc.
pub async fn ws_handler(ws: WebSocketUpgrade, State(bus): State<EventBus>) -> AxumResponse {
    ws.on_upgrade(move |socket| ws_session(socket, bus))
}

async fn ws_session(mut socket: WebSocket, bus: EventBus) {
    let mut rx = bus.subscribe();
    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Ok(text) => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        return;
                    }
                }
                // A slow client overflowed the buffer; skip and keep going.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            },
            // Drain client→server frames so we notice closes promptly.
            msg = socket.recv() => match msg {
                Some(Ok(_)) => continue,
                _ => return,
            },
        }
    }
}
