//! TypeScript emit: writes a single `index.ts` containing the transport
//! interface, every wire type, the per-namespace classes, and the `Api`
//! aggregator.

use std::fs;
use std::path::Path;

use super::emit_rust::capitalize;
use super::model::{Api, EventApi, Param};
use super::scan::normalize_numbers;

/// Transport interface, error type, and a default REST+WS `Rpc`
/// implementation embedded at the top of every generated `index.ts`.
///
/// The default factory `defaultRpc(...)` is tree-shaken if unused, so
/// projects that bring their own transport pay nothing for it.
const RPC_RUNTIME_TS: &str = "\
/** Returned by `Rpc.listen`, call to unsubscribe. */\n\
export type UnlistenFn = () => void;\n\
\n\
/**\n\
 * Transport contract the generated API classes call into. Implement this\n\
 * in your app (using fetch / axios / tauri / etc.) and pass it to\n\
 * `new Api(rpc)`. Or use `defaultRpc(...)` below.\n\
 */\n\
export interface Rpc {\n\
\t/** Send an RPC request. The wire command is `{namespace}/{method}`. */\n\
\tcall<T>(command: string, args?: Record<string, unknown>): Promise<T>;\n\
\n\
\t/** Subscribe to a backend event. Returns an unsubscribe handle. */\n\
\tlisten<T>(topic: string, handler: (payload: T) => void): UnlistenFn;\n\
}\n\
\n\
/** Thrown (or returned) by `Rpc.call` implementations on failure. */\n\
export class RpcError extends Error {\n\
\tconstructor(public readonly code: string, message: string) {\n\
\t\tsuper(message);\n\
\t\tthis.name = \"RpcError\";\n\
\t}\n\
}\n\
\n\
export type DefaultRpcOptions = {\n\
\t/** Base URL for RPC calls. Requests go to `${baseUrl}/${command}`. */\n\
\tbaseUrl?: string;\n\
\t/** WebSocket URL for events. Omit to make `listen()` throw. */\n\
\twsUrl?: string;\n\
\t/** Extra headers added to every RPC call. */\n\
\theaders?: Record<string, string>;\n\
\t/** Disable WebSocket auto-reconnect. Default: true. */\n\
\treconnect?: boolean;\n\
\t/** Cap on the backoff between reconnect attempts. Default: 30000. */\n\
\tmaxReconnectDelayMs?: number;\n\
};\n\
\n\
/**\n\
 * Default `Rpc` implementation: POST JSON for calls, a shared WebSocket\n\
 * for events. Wire format:\n\
 *\n\
 *  - Calls: `POST {baseUrl}/{command}` with body `args`. 2xx responses\n\
 *    are parsed as JSON and returned; non-2xx are thrown as `RpcError`.\n\
 *  - Events: WS frames shaped `{ topic: string, payload: T }`.\n\
 *\n\
 * Auto-reconnects with exponential backoff + jitter (capped at\n\
 * `maxReconnectDelayMs`) as long as there are active subscribers. Replace\n\
 * with your own `Rpc` for auth, custom transports, or a different wire\n\
 * format.\n\
 */\n\
export function defaultRpc(opts: DefaultRpcOptions = {}): Rpc {\n\
\tconst baseUrl = opts.baseUrl ?? \"/api\";\n\
\tconst maxDelayMs = opts.maxReconnectDelayMs ?? 30000;\n\
\tconst shouldReconnect = opts.reconnect !== false;\n\
\tconst subs = new Map<string, Set<(payload: unknown) => void>>();\n\
\tlet ws: WebSocket | undefined;\n\
\tlet reconnectDelayMs = 500;\n\
\tlet reconnectTimer: ReturnType<typeof setTimeout> | undefined;\n\
\n\
\tfunction openWs(url: string): WebSocket {\n\
\t\tconst sock = new WebSocket(url);\n\
\t\tsock.addEventListener(\"open\", () => { reconnectDelayMs = 500; });\n\
\t\tsock.addEventListener(\"message\", (ev) => {\n\
\t\t\ttry {\n\
\t\t\t\tconst { topic, payload } = JSON.parse(String(ev.data));\n\
\t\t\t\tsubs.get(topic)?.forEach((h) => h(payload));\n\
\t\t\t} catch {\n\
\t\t\t\t/* ignore malformed frames */\n\
\t\t\t}\n\
\t\t});\n\
\t\tsock.addEventListener(\"close\", () => {\n\
\t\t\tws = undefined;\n\
\t\t\tif (!shouldReconnect || subs.size === 0) return;\n\
\t\t\tconst base = Math.min(reconnectDelayMs, maxDelayMs);\n\
\t\t\tconst jitter = base * (0.8 + Math.random() * 0.4);\n\
\t\t\treconnectDelayMs = Math.min(reconnectDelayMs * 2, maxDelayMs);\n\
\t\t\treconnectTimer = setTimeout(() => {\n\
\t\t\t\treconnectTimer = undefined;\n\
\t\t\t\tif (subs.size > 0 && !ws) ws = openWs(url);\n\
\t\t\t}, jitter);\n\
\t\t});\n\
\t\treturn sock;\n\
\t}\n\
\n\
\tfunction ensureWs(): WebSocket {\n\
\t\tif (!opts.wsUrl) {\n\
\t\t\tthrow new RpcError(\"NO_WS_URL\", \"defaultRpc: wsUrl not configured; cannot listen()\");\n\
\t\t}\n\
\t\tif (ws && ws.readyState !== WebSocket.CLOSED) return ws;\n\
\t\tws = openWs(opts.wsUrl);\n\
\t\treturn ws;\n\
\t}\n\
\n\
\treturn {\n\
\t\tasync call<T>(command: string, args?: Record<string, unknown>): Promise<T> {\n\
\t\t\tconst res = await fetch(`${baseUrl}/${command}`, {\n\
\t\t\t\tmethod: \"POST\",\n\
\t\t\t\theaders: { \"Content-Type\": \"application/json\", ...opts.headers },\n\
\t\t\t\tbody: JSON.stringify(args ?? {}),\n\
\t\t\t});\n\
\t\t\tif (!res.ok) {\n\
\t\t\t\tthrow new RpcError(`HTTP_${res.status}`, await res.text());\n\
\t\t\t}\n\
\t\t\treturn (await res.json()) as T;\n\
\t\t},\n\
\t\tlisten<T>(topic: string, handler: (payload: T) => void): UnlistenFn {\n\
\t\t\tensureWs();\n\
\t\t\tlet set = subs.get(topic);\n\
\t\t\tif (!set) {\n\
\t\t\t\tset = new Set();\n\
\t\t\t\tsubs.set(topic, set);\n\
\t\t\t}\n\
\t\t\tconst wrapped = handler as (p: unknown) => void;\n\
\t\t\tset.add(wrapped);\n\
\t\t\treturn () => {\n\
\t\t\t\tset!.delete(wrapped);\n\
\t\t\t\tif (set!.size === 0) subs.delete(topic);\n\
\t\t\t\tif (subs.size === 0 && reconnectTimer) {\n\
\t\t\t\t\tclearTimeout(reconnectTimer);\n\
\t\t\t\t\treconnectTimer = undefined;\n\
\t\t\t\t}\n\
\t\t\t};\n\
\t\t},\n\
\t};\n\
}\n";

pub(super) fn write_index(
    out_path: &Path,
    types_in_order: &[String],
    per_type_dir: &Path,
    apis: &[Api],
    event_apis: &[EventApi],
) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str("// Generated by draad-codegen. Do not edit.\n\n");
    out.push_str(RPC_RUNTIME_TS);

    for ty in types_in_order {
        out.push('\n');
        let file = per_type_dir.join(format!("{ty}.ts"));
        let raw = fs::read_to_string(&file).unwrap_or_else(|_| {
            panic!(
                "missing per-type binding {}, run `cargo test export_bindings` first",
                file.display()
            );
        });
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") || trimmed.starts_with("// This file was generated") {
                continue;
            }
            out.push_str(&normalize_numbers(line));
            out.push('\n');
        }
    }

    for api in apis {
        emit_sub_class(&mut out, api);
    }
    for ev in event_apis {
        emit_events_class(&mut out, ev);
    }

    out.push('\n');
    out.push_str("export class Api {\n");
    for api in apis {
        emit_jsdoc(&mut out, &api.docs, "\t");
        out.push_str(&format!(
            "\t{ns}: {cls};\n",
            ns = api.namespace,
            cls = api.class_name
        ));
    }
    for ev in event_apis {
        emit_jsdoc(&mut out, &ev.docs, "\t");
        out.push_str(&format!(
            "\t{ns}Events: {cls};\n",
            ns = ev.namespace,
            cls = ev.class_name
        ));
    }
    out.push_str("\n\tconstructor(rpc: Rpc) {\n");
    for api in apis {
        out.push_str(&format!(
            "\t\tthis.{ns} = new {cls}(rpc);\n",
            ns = api.namespace,
            cls = api.class_name
        ));
    }
    for ev in event_apis {
        out.push_str(&format!(
            "\t\tthis.{ns}Events = new {cls}(rpc);\n",
            ns = ev.namespace,
            cls = ev.class_name
        ));
    }
    out.push_str("\t}\n");
    out.push_str("}\n");

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, out)
}

fn emit_sub_class(out: &mut String, api: &Api) {
    out.push('\n');
    emit_jsdoc(out, &api.docs, "");
    out.push_str(&format!("export class {} {{\n", api.class_name));
    out.push_str("\tconstructor(private rpc: Rpc) {}\n");

    for m in &api.methods {
        let param_decl = m
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ts_type))
            .collect::<Vec<_>>()
            .join(", ");

        let call_args = if m.params.is_empty() {
            String::new()
        } else {
            let names = m
                .params
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(", {{ {names} }}")
        };

        out.push('\n');
        emit_method_jsdoc(out, &m.docs, &m.params, "\t");
        out.push_str(&format!(
            "\t{name}({decl}): Promise<{ret}> {{\n",
            name = m.ts_name,
            decl = param_decl,
            ret = m.ret_ts,
        ));
        out.push_str(&format!(
            "\t\treturn this.rpc.call(\"{ns}/{name}\"{call_args});\n",
            ns = api.namespace,
            name = m.rust_name,
        ));
        out.push_str("\t}\n");
    }
    out.push_str("}\n");
}

fn emit_events_class(out: &mut String, ev: &EventApi) {
    out.push('\n');
    emit_jsdoc(out, &ev.docs, "");
    out.push_str(&format!("export class {} {{\n", ev.class_name));
    out.push_str("\tconstructor(private rpc: Rpc) {}\n");

    for e in &ev.events {
        out.push('\n');
        emit_jsdoc(out, &e.docs, "\t");
        out.push_str(&format!(
            "\ton{cap}(handler: (payload: {pl}) => void): UnlistenFn {{\n",
            cap = capitalize(&e.ts_name),
            pl = e.payload_ts,
        ));
        out.push_str(&format!(
            "\t\treturn this.rpc.listen<{pl}>(\"{wire}\", handler);\n",
            pl = e.payload_ts,
            wire = e.wire,
        ));
        out.push_str("\t}\n");
    }
    out.push_str("}\n");
}

fn emit_jsdoc(out: &mut String, docs: &[String], indent: &str) {
    if docs.is_empty() {
        return;
    }
    if docs.len() == 1 {
        out.push_str(&format!("{indent}/** {} */\n", docs[0]));
        return;
    }
    out.push_str(&format!("{indent}/**\n"));
    for line in docs {
        out.push_str(&format!("{indent} * {line}\n"));
    }
    out.push_str(&format!("{indent} */\n"));
}

fn emit_method_jsdoc(out: &mut String, method_docs: &[String], params: &[Param], indent: &str) {
    let has_method = !method_docs.is_empty();
    let has_params = params.iter().any(|p| !p.docs.is_empty());
    if !has_method && !has_params {
        return;
    }
    if has_method && !has_params && method_docs.len() == 1 {
        out.push_str(&format!("{indent}/** {} */\n", method_docs[0]));
        return;
    }
    out.push_str(&format!("{indent}/**\n"));
    for line in method_docs {
        out.push_str(&format!("{indent} * {line}\n"));
    }
    for p in params {
        if p.docs.is_empty() {
            continue;
        }
        let summary = p.docs.join(" ");
        out.push_str(&format!("{indent} * @param {} - {summary}\n", p.name));
    }
    out.push_str(&format!("{indent} */\n"));
}
