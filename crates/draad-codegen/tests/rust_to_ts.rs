//! End-to-end: generate the TS client from a Rust fixture, then run it
//! through Bun against a mocked `Rpc` and assert observable behaviour.
//!
//! Catches the kind of regression that pure substring tests miss —
//! verb dispatch wired to the wrong URL, `onError` not firing,
//! `RpcError` constructed with the wrong shape, etc. Skipped silently
//! when `bun` isn't on `PATH` so a stripped CI image doesn't fail the
//! suite.

mod common;

use std::path::Path;
use std::process::Command;

#[test]
fn generated_ts_client_round_trips_through_mock_rpc() {
    let Some(bun) = find_bun() else {
        eprintln!("skipping rust_to_ts: bun not on PATH");
        return;
    };

    let root = common::fresh_root("rust-to-ts");

    // Fixture covers every emit branch we care about: each HTTP verb,
    // a typed Result error, a struct + enum `#[ty]`, and a method
    // returning a Vec so the array TS shape gets exercised too.
    std::fs::write(
        root.join("src/api.rs"),
        r#"
use draad::{api, ty};

#[ty]
pub enum ApiError {
    NotFound,
    Unauthorized,
}

#[ty]
pub struct Item { pub id: i64, pub name: String }

#[api(namespace = "items")]
pub trait ItemsApi {
    async fn list(&self, prefix: String) -> Result<Vec<Item>, ApiError>;

    #[get]
    async fn get(&self, id: i64) -> Result<Item, ApiError>;

    #[put]
    async fn replace(&self, id: i64, name: String) -> Result<Item, ApiError>;

    #[delete]
    async fn remove(&self, id: i64) -> Result<(), ApiError>;

    /// No declared error; should NOT get `@throws` jsdoc.
    async fn ping(&self) -> String;
}
"#,
    )
    .unwrap();

    let client_dir = root.join("client");
    draad_codegen::Config::new()
        .root(&root)
        .client_dir(&client_dir)
        .generate()
        .unwrap();

    let test_script = root.join("test.ts");
    std::fs::write(&test_script, ts_test_program(&client_dir)).unwrap();

    let out = Command::new(&bun)
        .arg("run")
        .arg(&test_script)
        .output()
        .expect("failed to invoke bun");

    assert!(
        out.status.success(),
        "bun smoke failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn find_bun() -> Option<String> {
    Command::new("bun")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| "bun".to_string())
}

/// The bun-executable TS smoke test. Mocks `Rpc` directly so the test
/// doesn't need to spin up an actual server — what we're verifying is
/// the *generated client's* dispatch, not network behaviour.
fn ts_test_program(client_dir: &Path) -> String {
    let index = client_dir.join("index.ts");
    format!(
        r#"
import {{ Api, RpcError }} from "{index}";

// ── mock Rpc ─────────────────────────────────────────────────────────
type Call = {{ command: string; args: Record<string, unknown> | undefined; method: string }};
const calls: Call[] = [];
const errorHandlers: Array<(e: any) => void> = [];

const responses: Record<string, unknown> = {{
    "items/list":    [{{ id: 1, name: "first" }}, {{ id: 2, name: "second" }}],
    "items/get":     {{ id: 1, name: "first" }},
    "items/replace": {{ id: 1, name: "renamed" }},
    "items/remove":  null,
    "items/ping":    "pong",
}};

const rpc = {{
    async call(command: string, args?: Record<string, unknown>, method: string = "POST"): Promise<unknown> {{
        calls.push({{ command, args, method }});
        if (command === "items/get" && args?.id === 999) {{
            const err = new RpcError("HTTP_404", 404, {{ variant: "NotFound" }}, "missing");
            for (const h of errorHandlers) h(err);
            throw err;
        }}
        return responses[command];
    }},
    listen() {{ return () => {{}}; }},
    onError(h: (e: any) => void) {{
        errorHandlers.push(h);
        return () => {{
            const i = errorHandlers.indexOf(h);
            if (i >= 0) errorHandlers.splice(i, 1);
        }};
    }},
}};

// ── assertions ───────────────────────────────────────────────────────
function expect<T>(actual: T, want: T, label: string) {{
    const a = JSON.stringify(actual), w = JSON.stringify(want);
    if (a !== w) throw new Error(`${{label}}: expected ${{w}}, got ${{a}}`);
}}

const api = new Api(rpc);

const seen: any[] = [];
const stop = api.onError((err) => seen.push(err));

// 1. Each verb dispatches with the right HTTP method
const list = await api.items.list("a");
expect(list, [{{ id: 1, name: "first" }}, {{ id: 2, name: "second" }}], "list result");
expect(calls[0].method, "POST", "list default verb");
expect(calls[0].args, {{ prefix: "a" }}, "list args");

const got = await api.items.get(1);
expect(got, {{ id: 1, name: "first" }}, "get result");
expect(calls[1].method, "GET", "get verb");

const replaced = await api.items.replace(1, "renamed");
expect(replaced, {{ id: 1, name: "renamed" }}, "replace result");
expect(calls[2].method, "PUT", "replace verb");

const removed = await api.items.remove(1);
expect(removed, null, "remove result");
expect(calls[3].method, "DELETE", "remove verb");

const pong = await api.items.ping();
expect(pong, "pong", "ping result");
expect(calls[4].method, "POST", "ping default verb");

// 2. onError fires AND the promise still rejects.
let caught: any = null;
try {{
    await api.items.get(999);
}} catch (e) {{
    caught = e;
}}
if (!(caught instanceof RpcError)) {{
    throw new Error(`expected RpcError, got: ${{caught}}`);
}}
expect(caught.status, 404, "RpcError.status");
expect((caught.body as any).variant, "NotFound", "RpcError.body");
if (seen.length !== 1) {{
    throw new Error(`onError observer didn't fire exactly once; saw ${{seen.length}}`);
}}
if (seen[0] !== caught) {{
    throw new Error("onError observer saw a different RpcError than the throw");
}}

// 3. Unsubscribe takes effect.
stop();
try {{
    await api.items.get(999);
}} catch {{}}
if (seen.length !== 1) {{
    throw new Error(`unsubscribe didn't detach the handler; saw ${{seen.length}}`);
}}

console.log("OK");
"#,
        index = index.display(),
    )
}
