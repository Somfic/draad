//! Code generator for the [`draad`](https://docs.rs/draad) RPC schema.
//!
//! Scans a Rust source tree for `#[api]`, `#[events]`, and `#[ty]` items
//! (also accepted as `#[draad::api]` etc.), then emits:
//!
//!  - A backend `_generated.rs` with Axum handlers + a `rpc_router()` for
//!    every trait method, plus a `*Emitter` struct per events namespace
//!    that publishes onto the user-provided event bus.
//!  - A single frontend `index.ts` containing the transport runtime,
//!    every inlined `#[ty]` binding, one class per namespace, and an
//!    aggregator `Api`. Skipped when `rust_only` is set.
//!
//! Internal layout:
//!
//!  - [`config`] — public `Config` builder.
//!  - [`pipeline`] — drives scan → parse → emit.
//!  - [`scan`] — walks the source tree, matches attributes.
//!  - [`parse`] — `syn` AST → internal model.
//!  - [`types`] — Rust ⇄ TypeScript type-shape mapping.
//!  - [`model`] — plain data structs the parser hands to the emitters.
//!  - [`emit_rust`] / [`emit_ts`] — write the two output files.
//!  - [`util`] — small string helpers shared by parser + emitters.
//!
//! Designed to be called from a consumer's `build.rs`:
//!
//! ```ignore
//! // build.rs
//! draad::codegen::Config::new()
//!     .root(env!("CARGO_MANIFEST_DIR"))
//!     .rpc_runtime("crate::rpc::{RpcResponse, ok}")
//!     .rust_only()
//!     .generate()
//!     .unwrap();
//! ```

mod config;
mod emit_rust;
mod emit_ts;
mod model;
mod parse;
mod pipeline;
mod scan;
mod ty_decl;
mod types;
mod util;
mod writer;

pub use config::Config;
pub use pipeline::{generate, run, Artifacts};
