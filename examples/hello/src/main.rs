//! Axum server mounting the draad-generated `rpc_router()` plus a stateful
//! WebSocket [`session_handler`](session_handler) — each
//! connection gets its own session, its own topic subscriptions, and a
//! shared presence roster (see [`api::presence`]).
//!
//! Run with `cargo run -p hello`, then in another terminal:
//!
//! ```sh
//! websocat ws://localhost:3000/ws
//! # subscribe to the counter, then bump it from a third terminal:
//! {"type":"subscribe","topic":"counter/changed"}
//! #   curl -X POST http://localhost:3000/api/counter/increment
//! # join the roster + chat:
//! {"type":"subscribe","topic":"presence/roster"}
//! {"type":"join","name":"ada"}
//! ```
//!
//! The simpler broadcast-everything [`ws_handler`](draad::runtime::ws_handler)
//! is still available if you don't need per-client subscriptions.

mod api;

use std::sync::atomic::AtomicI32;
use std::sync::Arc;

use axum::extract::FromRef;
use axum::routing::get;
use axum::Router;
use draad::runtime::{session_handler, Conns, EventBus};
use tower_http::cors::CorsLayer;

use api::presence::{HelloSession, Members};

/// Application state. The `FromRef` derive lets `session_handler` (and the
/// generated `conn: &Conn` handlers) pull `State<EventBus>` and `State<Conns>`
/// out of this composite state without a custom extractor. `Conns` is draad's
/// registry of live sockets; `Members` is the app's own presence payload.
#[derive(Clone, FromRef)]
pub struct AppContext {
    pub counter: Arc<AtomicI32>,
    pub events: Events,
    pub bus: EventBus,
    pub members: Members,
    pub conns: Conns,
}

draad::include_generated!(crate::AppContext, draad::runtime::EventBus);

#[tokio::main]
async fn main() {
    let bus = EventBus::new();
    let events = Events::new(bus.clone());
    let ctx = AppContext {
        counter: Arc::new(AtomicI32::new(0)),
        events,
        bus,
        members: Members::new(),
        conns: Conns::new(),
    };

    let app = Router::new()
        .nest("/api", rpc_router())
        .route("/ws", get(session_handler::<HelloSession>))
        .with_state(ctx)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("hello: listening on http://localhost:3000  (ws: ws://localhost:3000/ws)");
    axum::serve(listener, app).await.unwrap();
}
