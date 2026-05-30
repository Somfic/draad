//! Axum server mounting the draad-generated `rpc_router()` plus the
//! default WebSocket broadcaster shipped in `draad::runtime`.
//!
//! Run with `cargo run -p hello`, then:
//!
//! ```sh
//! curl -X POST http://localhost:3000/api/counter/increment
//! websocat ws://localhost:3000/ws    # in another terminal
//! ```

mod api;

use std::sync::atomic::AtomicI32;
use std::sync::Arc;

use axum::extract::FromRef;
use axum::routing::get;
use axum::Router;
use draad::runtime::{ws_handler, EventBus};
use tower_http::cors::CorsLayer;

/// Application state. `FromRef` lets axum extract `State<EventBus>` for
/// the WS handler without us writing a custom extractor.
#[derive(Clone, FromRef)]
pub struct AppContext {
    pub counter: Arc<AtomicI32>,
    pub events: Events,
    pub bus: EventBus,
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
    };

    let app = Router::new()
        .nest("/api", rpc_router())
        .route("/ws", get(ws_handler))
        .with_state(ctx)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("hello: listening on http://localhost:3000  (ws: ws://localhost:3000/ws)");
    axum::serve(listener, app).await.unwrap();
}
