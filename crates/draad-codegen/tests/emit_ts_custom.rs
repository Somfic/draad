//! Exercises the `.custom_ts("custom")` config knob: a hand-written
//! sidecar TS module that holds type names draad can't resolve. The
//! generated `index.ts` should import the module at the top and rewrite
//! unknown type references to `custom.TypeName`, while `#[ty]`-declared
//! types stay bare (they're defined in the same `index.ts`).

mod common;

#[test]
fn imports_sidecar_and_prefixes_unknown_types() {
    let root = common::fresh_root("ts-custom");
    std::fs::write(
        root.join("src/search.rs"),
        r#"
use draad::{api, ty};

#[ty]
pub struct LocalThing { pub id: i64 }

#[api(namespace = "search")]
pub trait SearchApi {
    /// Echo back a non-#[ty] external type.
    async fn echo(&self, x: ExternalThing) -> Result<AnotherExternal, MyError>;
    /// Pass a local #[ty] through, should stay bare.
    async fn local(&self, t: LocalThing) -> Result<LocalThing, MyError>;
}
"#,
    )
    .unwrap();

    let client_dir = root.join("frontend");
    draad_codegen::Config::new()
        .root(&root)
        .client_dir(&client_dir)
        .custom_ts("custom")
        .generate()
        .unwrap();

    let index = std::fs::read_to_string(client_dir.join("index.ts")).expect("index.ts written");

    // Sidecar import sits between the header and the first type/class block.
    assert!(
        index.contains("import * as custom from \"./custom\";"),
        "missing custom-types import line:\n{index}"
    );

    // Unknown types in method params → `custom.X`.
    assert!(
        index.contains("echo(x: custom.ExternalThing): Promise<custom.AnotherExternal>"),
        "expected unknown types to be custom-prefixed:\n{index}"
    );

    // Error type in @throws also gets the prefix.
    assert!(
        index.contains(" * @throws {RpcError<custom.MyError>}"),
        "expected err type in jsdoc to be custom-prefixed:\n{index}"
    );

    // `#[ty]`-declared types stay bare - they're emitted as `export type ...`
    // in this same file, so `custom.LocalThing` would be a broken reference.
    assert!(
        index.contains("local(t: LocalThing): Promise<LocalThing>"),
        "expected #[ty] types to stay bare:\n{index}"
    );
    assert!(
        !index.contains("custom.LocalThing"),
        "#[ty] type should not be prefixed with custom.:\n{index}"
    );

    // The `#[ty]` declaration itself must still be emitted bare.
    assert!(
        index.contains("export type LocalThing"),
        "missing LocalThing export type:\n{index}"
    );
}

#[test]
fn disabled_by_default_leaves_unknown_types_bare() {
    let root = common::fresh_root("ts-custom-off");
    std::fs::write(
        root.join("src/search.rs"),
        r#"
use draad::api;

#[api(namespace = "search")]
pub trait SearchApi {
    async fn echo(&self, x: ExternalThing) -> Result<String, MyError>;
}
"#,
    )
    .unwrap();

    let client_dir = root.join("frontend");
    draad_codegen::Config::new()
        .root(&root)
        .client_dir(&client_dir)
        .generate()
        .unwrap();

    let index = std::fs::read_to_string(client_dir.join("index.ts")).expect("index.ts written");

    assert!(
        !index.contains("import * as custom"),
        "no custom import expected when feature off:\n{index}"
    );
    assert!(
        index.contains("echo(x: ExternalThing): Promise<string>"),
        "unknown types should pass through bare when feature off:\n{index}"
    );
    assert!(
        index.contains(" * @throws {RpcError<MyError>}"),
        "err type should pass through bare when feature off:\n{index}"
    );
}
