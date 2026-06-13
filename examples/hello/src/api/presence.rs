//! Presence + chat over a stateful [`draad::runtime::Session`].
//!
//! Demonstrates the per-connection session API and the split of concerns
//! introduced with the connected-client reshape:
//!
//!  - draad owns the transport: each socket gets a [`Conn`] with a
//!    server-assigned [`client_id`](Conn::client_id), and a `Conns` registry
//!    of live sockets (held in `AppContext`).
//!  - the app owns the *payload*: who each client is. Here that's a tiny
//!    [`Members`] roster keyed by `client_id`, broadcast on `presence/roster`.
//!
//! Try it (after `cargo run -p hello`):
//!
//! ```sh
//! websocat ws://localhost:3000/ws
//! # the server immediately sends {"topic":"$draad/welcome","payload":{"clientId":"…"}}
//! {"type":"subscribe","topic":"presence/roster"}
//! {"type":"join","name":"ada"}
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use crate::AppContext;

use draad::runtime::{Conn, Session};
use draad::{events, ty};
use serde::Deserialize;
use tokio::sync::Mutex;

/// One connected participant, as seen by everyone else.
#[ty]
pub struct Member {
    pub name: String,
}

/// A chat line relayed to every subscriber of `presence/said`.
#[ty]
pub struct ChatLine {
    pub from: String,
    pub text: String,
}

/// App-owned roster of connected members, keyed by draad's `client_id`.
/// draad's `Conns` tracks the live *sockets*; this tracks who's behind them —
/// the part draad deliberately doesn't model. Cheap to clone (shares one `Arc`).
#[derive(Clone, Default)]
pub struct Members(Arc<Mutex<HashMap<String, Member>>>);

impl Members {
    pub fn new() -> Self {
        Self::default()
    }
    async fn insert(&self, client_id: &str, member: Member) {
        self.0.lock().await.insert(client_id.to_string(), member);
    }
    async fn remove(&self, client_id: &str) {
        self.0.lock().await.remove(client_id);
    }
    async fn snapshot(&self) -> Vec<Member> {
        self.0.lock().await.values().cloned().collect()
    }
}

/// Server-pushed events for the presence namespace. These generate typed
/// `*Emitter` methods on the Rust side and `on{Event}` listeners on the TS
/// client.
#[events(namespace = "presence")]
pub trait PresenceEvents {
    /// Full roster, re-broadcast whenever someone joins or leaves.
    /// Topic: `presence/roster`.
    fn roster(payload: Vec<Member>);

    /// A chat line from one participant. Topic: `presence/said`.
    fn said(payload: ChatLine);
}

/// Application client→server frames. draad consumes the reserved
/// `{type:"subscribe"|"unsubscribe"}` frames itself; everything else is
/// deserialised into this enum and handed to [`Session::on_message`].
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HelloMsg {
    /// Announce (or rename) this client in the roster.
    Join { name: String },
    /// Say something to everyone subscribed to `presence/said`.
    Say { text: String },
}

/// Per-connection state — constructed on connect, consumed on disconnect.
pub struct HelloSession {
    name: Option<String>,
}

#[async_trait::async_trait]
impl Session for HelloSession {
    type State = AppContext;
    type Msg = HelloMsg;

    async fn on_connect(_conn: &Conn, _state: &AppContext) -> Self {
        HelloSession { name: None }
    }

    async fn on_message(&mut self, msg: HelloMsg, conn: &Conn, state: &AppContext) {
        match msg {
            HelloMsg::Join { name } => {
                self.name = Some(name.clone());
                // Key the roster by the server-assigned client id, so a
                // reconnect (resume) replaces its own entry.
                state
                    .members
                    .insert(conn.client_id(), Member { name })
                    .await;
                let roster = state.members.snapshot().await;
                state.events.presence.emit_roster(&roster);
            }
            HelloMsg::Say { text } => {
                let from = self.name.clone().unwrap_or_else(|| "anon".to_string());
                state.events.presence.emit_said(&ChatLine { from, text });
            }
        }
    }

    async fn on_disconnect(self, conn: &Conn, state: &AppContext) {
        state.members.remove(conn.client_id()).await;
        let roster = state.members.snapshot().await;
        state.events.presence.emit_roster(&roster);
    }
}
