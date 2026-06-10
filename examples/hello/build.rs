//! Runs draad codegen during `cargo build`. Defaults match this crate's
//! layout (`crate::app::{AppContext, EventBus}` and the embedded
//! `draad::runtime::{Response, ok}`), so config is minimal. `generate()`
//! emits its own `cargo:rerun-if-changed` directives.

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    draad::codegen::Config::new()
        .root(&manifest_dir)
        .generated_rs(format!("{out_dir}/_generated.rs"))
        .client_dir("frontend/src/schema")
        .generate()
        .expect("draad codegen failed");
}
