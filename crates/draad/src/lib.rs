//! Trait-driven RPC schema for Rust↔TypeScript.
//!
//! Define your API as Rust traits annotated with [`api`] / [`events`] /
//! [`ty`]; the codegen reads the source and emits the matching Axum
//! router plus a typed TypeScript client. Drop a single
//! `draad::include_generated!(AppContext, EventBus);` line at module
//! scope (typically in `main.rs`) — no `build.rs`, no `cargo test`
//! step, all wiring happens during proc-macro expansion on `cargo
//! build`.
//!
//! ## Example
//!
//! ```ignore
//! use draad::{api, ty, runtime::EventBus};
//!
//! #[ty]
//! pub struct Hit { pub id: i64, pub title: String }
//!
//! #[api(namespace = "search")]
//! pub trait SearchApi {
//!     async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
//! }
//!
//! #[derive(Clone)]
//! pub struct AppContext { /* ... */ }
//!
//! draad::include_generated!(AppContext, EventBus);
//! ```

pub use draad_macros::{api, events, include_generated, raw, ty};

/// The scan → parse → emit pipeline `include_generated!` calls into.
/// Re-exported for the rare consumer that wants to drive the codegen
/// from a hand-written `build.rs` instead of the macro path.
pub use draad_codegen as codegen;

/// The axum-side runtime the generated handlers / emitters call into:
/// `EventBus`, the stateless `ws_handler`, and the stateful
/// `Session`/`Conn`/`Clients` system with per-client subscriptions. Only
/// available with the `runtime` feature enabled.
#[cfg(feature = "runtime")]
pub mod runtime;
