//! Code generator for the [`draad`](https://docs.rs/draad) RPC schema.
//!
//! Scans a Rust source tree for `#[api]`, `#[events]`, and `#[ty]` items
//! (also accepted as `#[draad::api]` etc.), then emits:
//!
//!  - A backend `_generated.rs` with Axum handlers + a `rpc_router()` for
//!    every trait method, plus a `*Emitter` struct per events namespace
//!    that publishes onto the user-provided event bus.
//!  - One TypeScript namespace file per Rust module under the frontend
//!    output dir, mirroring the trait shape and inlining ts-rs type
//!    bindings. (Skipped when `rust_only` is set.)
//!  - A frontend `index.ts` assembling every namespace under a single
//!    `api` singleton.
//!
//! Designed to be called from a consumer's `build.rs`:
//!
//! ```ignore
//! // build.rs
//! fn main() {
//!     draad_codegen::Config::new()
//!         .root(env!("CARGO_MANIFEST_DIR"))
//!         .state_type("crate::app::AppContext")
//!         .event_bus_type("crate::app::EventBus")
//!         .rpc_runtime("crate::rpc::{RpcResponse, ok}")
//!         .rust_only()
//!         .generate()
//!         .unwrap();
//! }
//! ```

mod config;
mod emit_rust;
mod emit_ts;
mod model;
mod parse;
mod scan;

pub use config::{generate, Config};
