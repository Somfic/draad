//! Trait-driven RPC schema for Rust↔TypeScript.
//!
//! Define your API as Rust traits annotated with [`api`] / [`events`] /
//! [`ty`]; the codegen (under the `codegen` feature) reads the source and
//! emits the matching Axum router plus a typed TypeScript client.
//!
//! See the crate README for a complete walkthrough.
//!
//! ## Setup
//!
//! ```toml
//! # runtime, just the proc-macros
//! [dependencies]
//! draad = "0.1"
//!
//! # build.rs, codegen module
//! [build-dependencies]
//! draad = { version = "0.1", features = ["codegen"] }
//! ```
//!
//! ## Example
//!
//! ```ignore
//! use draad::{api, ty};
//!
//! #[ty]
//! pub struct Hit { pub id: i64, pub title: String }
//!
//! #[api(namespace = "search")]
//! pub trait SearchApi {
//!     async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
//! }
//! ```

pub use draad_macros::{api, events, ty};

/// Includes the Rust file emitted by [`codegen`] from your `build.rs`.
///
/// Expand once at module scope (typically in `lib.rs` or `main.rs`) and the
/// generated `rpc_router()`, request DTOs, axum handlers, and `Events`
/// emitter tree become part of your crate. Equivalent to writing
/// `include!(concat!(env!("OUT_DIR"), "/_generated.rs"));` by hand.
///
/// Your `build.rs` must write the generated file to `OUT_DIR`:
///
/// ```ignore
/// // build.rs
/// let out = std::env::var("OUT_DIR").unwrap();
/// draad::codegen::Config::new()
///     .generated_rs(format!("{out}/_generated.rs"))
///     // ...rest of config...
///     .generate()
///     .unwrap();
/// ```
///
/// ```ignore
/// // src/lib.rs
/// draad::include_generated!();
/// ```
#[macro_export]
macro_rules! include_generated {
    ($state:ty $(,)?) => {
        $crate::include_generated!($state, ());
    };
    ($state:ty, $bus:ty $(,)?) => {
        #[allow(
            non_camel_case_types,
            dead_code,
            unused_imports,
            clippy::needless_lifetimes
        )]
        mod __draad_generated {
            // The generated file references `__DraadState` / `__DraadBus`;
            // these aliases bind them to the consumer's types without
            // requiring those types to live at any particular module
            // path. Distinctive names sidestep recursive-alias issues
            // when the caller's type also happens to be `AppContext`.
            pub(super) type __DraadState = $state;
            pub(super) type __DraadBus = $bus;
            include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
        }
        pub use __draad_generated::*;
    };
}

/// Scanner + emitter used from `build.rs` (and the `draad` bin).
///
/// Only available with the `codegen` feature enabled, runtime consumers
/// don't pay the cost of pulling `syn`.
#[cfg(feature = "codegen")]
pub mod codegen;

/// `Response` + `ok`: the axum response shim the generated handlers
/// call into. Only available with the `runtime` feature enabled.
#[cfg(feature = "runtime")]
pub mod runtime;
