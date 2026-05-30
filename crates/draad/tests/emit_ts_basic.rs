//! Drives the TypeScript pass (no `rust_only`) against a fixture whose
//! API uses only primitive types, so we don't need ts-rs per-type
//! bindings on disk. Asserts the single emitted `index.ts` contains the
//! runtime (Rpc/UnlistenFn/RpcError), the namespace class, and the Api
//! aggregator.

#![cfg(feature = "codegen")]

mod common;

#[test]
fn emits_single_index_with_runtime_and_classes() {
    let root = common::fresh_root("ts-basic");
    std::fs::write(
        root.join("src/search.rs"),
        r#"
use draad::api;

#[api(namespace = "search")]
pub trait SearchApi {
    /// Full-text search.
    async fn query(&self, q: String) -> Result<String, MyError>;
}
"#,
    )
    .unwrap();

    let client_dir = root.join("frontend");
    draad::codegen::Config::new()
        .root(&root)
        .client_dir(&client_dir)
        .rpc_runtime("draad::runtime::{Response, ok}")
        .generate()
        .unwrap();

    let index = std::fs::read_to_string(client_dir.join("index.ts")).expect("index.ts written");

    // Runtime block
    assert!(
        index.contains("export interface Rpc"),
        "missing Rpc:\n{index}"
    );
    assert!(
        index.contains("export type UnlistenFn"),
        "missing UnlistenFn:\n{index}"
    );
    assert!(
        index.contains("export class RpcError extends Error"),
        "missing RpcError:\n{index}"
    );
    assert!(
        index.contains("export function defaultRpc("),
        "missing defaultRpc factory:\n{index}"
    );

    // Namespace class
    assert!(
        index.contains("export class SearchApi"),
        "missing SearchApi class:\n{index}"
    );
    assert!(
        index.contains("constructor(private rpc: Rpc) {}"),
        "missing rpc-injecting constructor:\n{index}"
    );
    assert!(
        index.contains("query(q: string): Promise<string>"),
        "missing typed method signature:\n{index}"
    );
    assert!(
        index.contains("return this.rpc.call(\"search/query\""),
        "missing this.rpc.call() dispatch:\n{index}"
    );
    assert!(
        index.contains("/** Full-text search. */"),
        "missing method jsdoc:\n{index}"
    );

    // Aggregator
    assert!(
        index.contains("export class Api {"),
        "missing Api class:\n{index}"
    );
    assert!(
        index.contains("constructor(rpc: Rpc)"),
        "missing Api(rpc) ctor:\n{index}"
    );
    assert!(
        index.contains("this.search = new SearchApi(rpc);"),
        "missing sub-class wiring:\n{index}"
    );
    assert!(
        !index.contains("export const api"),
        "auto-singleton should be gone:\n{index}"
    );

    // Single-file: no per-namespace file exists
    assert!(
        !client_dir.join("search.ts").exists(),
        "per-namespace file should not be emitted in single-file mode"
    );
    assert!(
        !client_dir.join("_rpc.ts").exists(),
        "_rpc.ts should be inlined into index.ts, not a separate file"
    );
}
