//! Stateful per-connection WebSocket sessions: server-side topic
//! subscriptions, a server-assigned client identity, and a registry of live
//! connections for server→client addressing. Enabled by the `runtime`
//! feature.
//!
//! Where [`ws_handler`](super::ws_handler) is a stateless fan-out that
//! pushes *every* event to *every* client, [`session_handler`] gives each
//! connection its own [`Session`] value plus a [`Conn`] handle, only
//! forwards events the connection has subscribed to, and registers the
//! connection in [`Conns`] under a stable [`client_id`](Conn::client_id).
//!
//! ## Identity
//!
//! On connect the server assigns a `client_id` and pushes it to the client
//! as the first frame — `{ "topic": "$draad/welcome", "payload": { "clientId": "…" } }`.
//! The generated client persists it and replays it: as the `x-draad-client`
//! header on every HTTP `call`, and as a `?resume=<id>` query param when the
//! socket reconnects (so the id is stable across reconnects). The
//! [`Caller`] extractor turns that header back into the caller's live
//! [`Conn`] inside HTTP handlers.
//!
//! ## Reserved vs. application frames
//!
//! The client→server wire protocol reserves two frame `type`s, handled by
//! draad itself:
//!
//! ```jsonc
//! { "type": "subscribe",   "topic": "streams/stats" }
//! { "type": "unsubscribe", "topic": "streams/stats" }
//! ```
//!
//! Every other frame is deserialised into your [`Session::Msg`] and handed
//! to [`Session::on_message`].
//!
//! ## Example
//!
//! ```ignore
//! use draad::runtime::{Conn, Conns, EventBus, Session, session_handler};
//! use axum::{Router, routing::get, extract::FromRef};
//! use serde::Deserialize;
//!
//! #[derive(Clone, FromRef)]
//! struct AppState { bus: EventBus, conns: Conns }
//!
//! #[derive(Deserialize)]
//! #[serde(tag = "type", rename_all = "snake_case")]
//! enum Msg { Say { text: String } }
//!
//! struct Chat;
//!
//! #[async_trait::async_trait]
//! impl Session for Chat {
//!     type State = AppState;
//!     type Msg = Msg;
//!     async fn on_connect(_conn: &Conn, _state: &AppState) -> Self { Chat }
//!     async fn on_message(&mut self, msg: Msg, conn: &Conn, _state: &AppState) {
//!         let Msg::Say { text } = msg;
//!         conn.publish("chat", &text);
//!     }
//!     async fn on_disconnect(self, _conn: &Conn, _state: &AppState) {}
//! }
//!
//! let app: Router = Router::new()
//!     .route("/ws", get(session_handler::<Chat>))
//!     .with_state(AppState { bus: EventBus::new(), conns: Conns::new() });
//! ```

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, FromRequestParts, RawQuery, State};
use axum::http::request::Parts;
use axum::response::Response as AxumResponse;
use futures::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, Mutex};

use super::EventBus;

/// Reserved topic prefix for draad's own control frames (e.g. the welcome
/// frame). Apps must not use topics under this prefix.
const WELCOME_TOPIC: &str = "$draad/welcome";

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// Internal, process-monotonic identifier for one WebSocket *socket*. Distinct
/// from the public [`client_id`](Conn::client_id): a reconnect gets a fresh
/// `ConnId` but keeps its `client_id`. Used to make registry eviction
/// resume-safe (a stale socket only unbinds the id if it still owns it).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ConnId(u64);

/// Handle to one live WebSocket connection, handed to every [`Session`]
/// hook and injectable into RPC handlers via a `conn: &Conn` parameter.
/// Cheap to clone.
#[derive(Clone)]
pub struct Conn {
    id: ConnId,
    client_id: Arc<str>,
    bus: EventBus,
    subs: Arc<Mutex<HashSet<String>>>,
    tx: mpsc::UnboundedSender<Message>,
}

impl Conn {
    /// Internal per-socket id (fresh on every reconnect).
    pub fn id(&self) -> ConnId {
        self.id
    }

    /// The server-assigned, reconnect-stable client identity. This is the
    /// value the client replays as `x-draad-client` / `?resume=`, and the key
    /// this connection is registered under in [`Conns`].
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Add `topic` to this connection's subscription set. After this, bus
    /// events on `topic` are forwarded to this socket. Normally driven by
    /// the client's `subscribe` frame, but available for server-initiated
    /// subscriptions too.
    pub async fn subscribe(&self, topic: impl Into<String>) {
        self.subs.lock().await.insert(topic.into());
    }

    /// Remove `topic` from this connection's subscription set.
    pub async fn unsubscribe(&self, topic: &str) {
        self.subs.lock().await.remove(topic);
    }

    /// Send a `{ topic, payload }` event to **this socket only**, bypassing
    /// the bus and subscription filter. Use for targeted, per-connection
    /// pushes (an ack, a private snapshot). Received by the client's
    /// `listen(topic, …)` like any other event.
    pub fn send<T: ?Sized + Serialize>(&self, topic: &str, payload: &T) {
        let json = serde_json::json!({ "topic": topic, "payload": payload }).to_string();
        let _ = self.tx.send(Message::Text(json.into()));
    }

    /// Publish a `{ topic, payload }` event onto the shared [`EventBus`], so
    /// **every** connection subscribed to `topic` receives it (the basis for
    /// a client→client proxy). Equivalent to the bus the generated emitters
    /// use, so a published frame is indistinguishable from an emitted one.
    pub fn publish<T: ?Sized + Serialize>(&self, topic: &str, payload: &T) {
        self.bus.publish(topic, payload);
    }
}

/// Registry of currently-connected clients keyed by their server-assigned
/// `client_id`. draad binds/unbinds entries automatically as sockets connect
/// and disconnect; apps use it to address a specific client
/// ([`send_to`](Conns::send_to) / [`get`](Conns::get)) and it backs the
/// [`Caller`] extractor. Put one in your state (and derive `FromRef`) so
/// `session_handler` and generated `conn: &Conn` handlers can reach it.
/// Cheap to clone (shares one `Arc`).
#[derive(Clone, Default)]
pub struct Conns(Arc<Mutex<HashMap<String, Conn>>>);

impl Conns {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a live connection under its `client_id` (resume overwrites).
    async fn bind(&self, conn: Conn) {
        self.0
            .lock()
            .await
            .insert(conn.client_id().to_string(), conn);
    }

    /// Drop `client_id`, but only if the registered connection is still this
    /// socket — so a reconnect that already rebound the id isn't clobbered by
    /// the old socket's late disconnect.
    async fn unbind(&self, client_id: &str, id: ConnId) {
        let mut g = self.0.lock().await;
        if g.get(client_id).map(Conn::id) == Some(id) {
            g.remove(client_id);
        }
    }

    /// The live connection for `client_id`, if currently connected.
    pub async fn get(&self, client_id: &str) -> Option<Conn> {
        self.0.lock().await.get(client_id).cloned()
    }

    /// Send a `{ topic, payload }` event to a specific client's socket.
    /// Returns `false` if that client isn't currently connected.
    pub async fn send_to<T: ?Sized + Serialize>(
        &self,
        client_id: &str,
        topic: &str,
        payload: &T,
    ) -> bool {
        match self.0.lock().await.get(client_id) {
            Some(c) => {
                c.send(topic, payload);
                true
            }
            None => false,
        }
    }

    /// The `client_id`s of every currently-connected client.
    pub async fn ids(&self) -> Vec<String> {
        self.0.lock().await.keys().cloned().collect()
    }

    /// Number of currently-connected clients.
    pub async fn len(&self) -> usize {
        self.0.lock().await.len()
    }

    /// Whether no clients are connected.
    pub async fn is_empty(&self) -> bool {
        self.0.lock().await.is_empty()
    }
}

/// Resolves the calling client's live [`Conn`] from the `x-draad-client`
/// header against the [`Conns`] in state. `Caller(None)` when the header is
/// missing or no live connection matches. This is what backs an injected
/// `conn: &Conn` (required) or `conn: Option<&Conn>` (optional) parameter on
/// a generated RPC handler; it never rejects, so the handler decides what to
/// do when there's no live connection.
pub struct Caller(pub Option<Conn>);

impl<S> FromRequestParts<S> for Caller
where
    S: Send + Sync,
    Conns: FromRef<S>,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let conns = Conns::from_ref(state);
        let id = parts
            .headers
            .get("x-draad-client")
            .and_then(|v| v.to_str().ok());
        let conn = match id {
            Some(id) => conns.get(id).await,
            None => None,
        };
        Ok(Caller(conn))
    }
}

/// Application-defined per-connection handler. draad constructs one value
/// per WebSocket connection via [`on_connect`](Session::on_connect), feeds
/// it application frames through [`on_message`](Session::on_message), and
/// consumes it in [`on_disconnect`](Session::on_disconnect).
///
/// `subscribe`/`unsubscribe` frames are handled by draad and never reach
/// `on_message`.
#[async_trait::async_trait]
pub trait Session: Sized + Send + 'static {
    /// Shared application state, pulled from the router's `State`. Must yield
    /// an [`EventBus`] and a [`Conns`] via [`FromRef`].
    type State: Clone + Send + Sync + 'static;

    /// Application client→server message. Deserialised from every inbound
    /// frame whose `type` is not a reserved `subscribe`/`unsubscribe`.
    /// A typical shape is a `#[serde(tag = "type", rename_all = "snake_case")]`
    /// enum. Frames that fail to deserialise are ignored.
    type Msg: DeserializeOwned + Send;

    /// Called once when the socket connects, after the `client_id` is
    /// assigned and the welcome frame is queued, before any message is read.
    async fn on_connect(conn: &Conn, state: &Self::State) -> Self;

    /// Called for each application frame the client sends.
    async fn on_message(&mut self, msg: Self::Msg, conn: &Conn, state: &Self::State);

    /// Called once when the socket closes (cleanly or otherwise).
    async fn on_disconnect(self, conn: &Conn, state: &Self::State);
}

/// axum WebSocket handler that drives a [`Session`]. Mount it with a
/// turbofish for your session type:
///
/// ```ignore
/// Router::new().route("/ws", axum::routing::get(session_handler::<MySession>))
/// ```
///
/// Requires the router's state to yield an [`EventBus`] and a [`Conns`] via
/// [`FromRef`].
pub async fn session_handler<S: Session>(
    ws: WebSocketUpgrade,
    RawQuery(query): RawQuery,
    State(state): State<S::State>,
) -> AxumResponse
where
    EventBus: FromRef<S::State>,
    Conns: FromRef<S::State>,
{
    let bus = EventBus::from_ref(&state);
    let conns = Conns::from_ref(&state);
    let resume = query
        .as_deref()
        .and_then(resume_param)
        .filter(|s| !s.is_empty());
    ws.on_upgrade(move |socket| session_loop::<S>(socket, state, bus, conns, resume))
}

/// Pull the `resume` value out of a raw query string. client ids are UUIDs
/// (no percent-encoding needed), so the raw value is taken verbatim.
fn resume_param(query: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("resume="))
        .map(str::to_string)
}

async fn session_loop<S: Session>(
    socket: WebSocket,
    state: S::State,
    bus: EventBus,
    conns: Conns,
    resume: Option<String>,
) {
    let id = ConnId(NEXT_CONN.fetch_add(1, Ordering::Relaxed));
    let client_id: Arc<str> = resume
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        .into();
    let subs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let (tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let conn = Conn {
        id,
        client_id: client_id.clone(),
        bus: bus.clone(),
        subs: subs.clone(),
        tx,
    };

    let (mut sink, mut stream) = socket.split();

    // Writer: the single owner of the sink. Everything that wants to push
    // to the socket (the event pump, `Conn::send`) funnels through `tx`.
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Pump: forward bus frames this connection is subscribed to.
    let pump = {
        let subs = subs.clone();
        let tx = conn.tx.clone();
        let mut bus_rx = bus.subscribe();
        tokio::spawn(async move {
            loop {
                match bus_rx.recv().await {
                    Ok(frame) => {
                        if subs.lock().await.contains(&frame.topic)
                            && tx.send(Message::Text(frame.json.clone().into())).is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    };

    conns.bind(conn.clone()).await;
    // Tell the client its (re)assigned id before anything else goes out.
    conn.send(
        WELCOME_TOPIC,
        &serde_json::json!({ "clientId": &*client_id }),
    );

    let mut session = S::on_connect(&conn, &state).await;

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        match value.get("type").and_then(|t| t.as_str()) {
            Some("subscribe") => {
                if let Some(topic) = value.get("topic").and_then(|t| t.as_str()) {
                    conn.subscribe(topic.to_string()).await;
                }
            }
            Some("unsubscribe") => {
                if let Some(topic) = value.get("topic").and_then(|t| t.as_str()) {
                    conn.unsubscribe(topic).await;
                }
            }
            // Anything else is an application frame.
            _ => {
                if let Ok(msg) = serde_json::from_value::<S::Msg>(value) {
                    session.on_message(msg, &conn, &state).await;
                }
            }
        }
    }

    session.on_disconnect(&conn, &state).await;
    conns.unbind(&client_id, id).await;
    pump.abort();
    writer.abort();
}
