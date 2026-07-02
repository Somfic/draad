// Per integration test file, Cargo compiles `common` separately; not
// every test uses every helper, so dead-code lints can fire.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

pub fn fresh_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("draad-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

/// Run the full pipeline against `root` and return the aggregator
/// (`_generated.rs`). After the per-module move, this contains the
/// `rpc_router` skeleton and event emitters but *not* the actual
/// handler bodies - those are emitted by `#[api]` into each trait's
/// own module. Use [`module_rust`] to inspect that side.
pub fn run(root: &PathBuf) -> String {
    draad_codegen::Config::new()
        .root(root)
        .rust_only()
        .generate()
        .unwrap();
    fs::read_to_string(root.join("src/_generated.rs")).unwrap()
}

/// Render the per-module Rust chunk that `#[api]` would emit for the
/// first trait it finds in `src`. The chunk holds the wire-args
/// structs, generic handlers, and `apply_routes`.
pub fn module_rust(src: &str, namespace: &str) -> String {
    let file: syn::File = syn::parse_str(src).expect("source parses");
    let t = file
        .items
        .iter()
        .find_map(|i| match i {
            syn::Item::Trait(t) => Some(t),
            _ => None,
        })
        .expect("source contains a trait");
    draad_codegen::render_module_rust(t, namespace)
}
