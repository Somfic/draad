//! Rust emit: two surfaces.
//!
//! * [`render_module_rust`] is called by `#[api]` on a trait at macro
//!   expansion time. It returns a Rust source chunk: a wrapper module
//!   containing per-namespace handlers and a generic `apply_routes<S>`
//!   that registers them against an axum `Router<S>`, which the macro
//!   splices in alongside the trait. Because the chunk lives in the
//!   same module as the trait, handler signatures resolve types via
//!   that module's existing `use` statements.
//! * [`render_generated_rs`] is the aggregator that `include_generated!`
//!   splices at the call site. It emits the `__DraadState`/`__DraadBus`
//!   aliases, `pub fn rpc_router()` which chains each module's
//!   `apply_routes`, the events emitters, and `#[raw]` URL constants.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::config::Config;
use super::model::{Api, ConnInject, EventApi, Method, RawApi};
use super::parse::parse_trait;
use super::types::{result_ok_type, TypeCtx};
use super::util::{capitalize, last_path_segment, snake_to_camel};
use super::writer::Writer;

// `include_generated!(MyState, MyBus)` wraps the include in a module
// that aliases these internal names to the caller's actual types, so
// the emitted file doesn't need to know where the state lives.
const STATE_TY: &str = "__DraadState";
const BUS_TY: &str = "__DraadBus";
// The generic parameter handlers + apply_routes are parameterised on.
// Double-underscore to avoid clashing
const GENERIC_S: &str = "__S";

/// Render the per-module Rust chunk for a single `#[api]` trait.
pub fn render_module_rust(t: &syn::ItemTrait, namespace: &str) -> String {
    let empty: BTreeSet<String> = BTreeSet::new();
    let ctx = TypeCtx {
        local_tys: &empty,
        custom_module: None,
    };
    let api = parse_trait(t, namespace.to_string(), String::new(), &ctx);
    let mut w = Writer::new("    ");
    write_module_chunk(&mut w, &api);
    w.into_string()
}

pub(super) fn render_generated_rs(
    cfg: &Config,
    apis: &[Api],
    event_apis: &[EventApi],
    raw_apis: &[RawApi],
    ty_modules: &[String],
) -> String {
    let mut w = Writer::new("    ");

    write_aggregator_header(&mut w);
    write_aggregator_uses(&mut w, cfg, event_apis, ty_modules);
    write_rpc_router(&mut w, cfg, apis);
    write_event_emitters(&mut w, event_apis);
    write_url_consts(&mut w, raw_apis);

    w.into_string()
}

pub(super) fn write_generated_rs(
    cfg: &Config,
    out_path: &Path,
    apis: &[Api],
    event_apis: &[EventApi],
    raw_apis: &[RawApi],
    ty_modules: &[String],
) -> std::io::Result<()> {
    let out = render_generated_rs(cfg, apis, event_apis, raw_apis, ty_modules);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(existing) = fs::read_to_string(out_path) {
        if existing == out {
            return Ok(());
        }
    }
    fs::write(out_path, out)
}

/// Whether a trait has any method with an injected `conn: &Conn` /
/// `Option<&Conn>` parameter. Drives extra bound and `use` emission.
fn any_conn(api: &Api) -> bool {
    api.methods
        .iter()
        .any(|m| m.params.iter().any(|p| p.conn.is_some()))
}

/// Write the per-module chunk: a wrapper `mod __draad_<ns>_routes`
/// containing private args structs + handlers and a `pub(super) fn
/// apply_routes<S>` generic over the state, plus a `pub use` re-export
/// that lifts `apply_routes` into the trait module's namespace.
fn write_module_chunk(w: &mut Writer, api: &Api) {
    let module_ident = format!("__draad_{}_routes", api.namespace);
    let conn = any_conn(api);

    w.line("#[doc(hidden)]");
    w.line("#[allow(non_camel_case_types, dead_code, unused_imports)]");
    w.line(&format!("mod {module_ident} {{"));
    w.indented(|w| {
        w.line("use super::*;");
        w.line("use ::axum::extract::{State, Query};");
        w.line("use ::axum::routing::{delete, get, patch, post, put};");
        w.line("use ::axum::{Json, Router};");
        if conn {
            // `Caller` extracts the live `Conn` for the current request;
            // the trait bound on `apply_routes` ensures `Conns: FromRef<S>`.
            // `IntoResponse` (as `_`) is imported for method dispatch only.
            w.line("use ::axum::response::IntoResponse as _;");
            w.line("use ::draad::runtime::Caller;");
        }
        w.blank();

        for m in &api.methods {
            write_args_struct(w, m);
            write_handler(w, api, m, conn);
        }

        write_apply_routes(w, api, conn);
    });
    w.line("}");
    w.blank();
    w.line(&format!(
        "pub use {module_ident}::apply_routes as __draad_{namespace}_apply_routes;",
        namespace = api.namespace
    ));
    w.blank();
}

fn write_args_struct(w: &mut Writer, m: &Method) {
    let wire: Vec<_> = m.params.iter().filter(|p| p.conn.is_none()).collect();
    if wire.is_empty() {
        return;
    }
    w.line("#[derive(::serde::Deserialize)]");
    w.line(&format!("struct __{cmd}_args {{", cmd = m.command));
    w.indented(|w| {
        for p in &wire {
            w.line(&format!("{}: {},", p.name, p.rust_type));
        }
    });
    w.line("}");
    w.blank();
}

/// Bound clause shared by handlers and `apply_routes`. `_any_conn` is
/// passed in so the caller doesn't recompute it; we add the `FromRef`
/// bound for the `Caller` extractor only when at least one method needs
/// it.
fn write_where_bounds(w: &mut Writer, api: &Api, conn: bool) {
    w.indented(|w| {
        w.line(&format!(
            "{GENERIC_S}: {trait_} + ::core::clone::Clone + \
             ::core::marker::Send + ::core::marker::Sync + 'static,",
            trait_ = api.class_name,
        ));
        if conn {
            w.line(&format!(
                "::draad::runtime::Conns: ::axum::extract::FromRef<{GENERIC_S}>,"
            ));
        }
    });
}

fn write_handler(w: &mut Writer, api: &Api, m: &Method, any_conn_in_trait: bool) {
    let wire: Vec<_> = m.params.iter().filter(|p| p.conn.is_none()).collect();
    let has_conn = m.params.iter().any(|p| p.conn.is_some());
    let needs_required_conn = m
        .params
        .iter()
        .any(|p| matches!(p.conn, Some(ConnInject { required: true })));

    let mut extractors = vec![format!("State(__ctx): State<{GENERIC_S}>")];
    if has_conn {
        // `Caller` is a `FromRequestParts` extractor, so it must precede
        // the body extractor.
        extractors.push("__caller: Caller".to_string());
    }
    if !wire.is_empty() {
        let inner = format!("__{cmd}_args", cmd = m.command);
        extractors.push(if m.verb.has_body() {
            format!("Json(__args): Json<{inner}>")
        } else {
            format!("Query(__args): Query<{inner}>")
        });
    }

    let call_args = {
        let parts: Vec<String> = m
            .params
            .iter()
            .map(|p| match p.conn {
                None => format!("__args.{}", p.name),
                Some(ConnInject { required: true }) => "__conn".to_string(),
                Some(ConnInject { required: false }) => "__caller.0.as_ref()".to_string(),
            })
            .collect();
        if parts.is_empty() {
            String::new()
        } else {
            format!(", {}", parts.join(", "))
        }
    };
    let call = format!(
        "<{GENERIC_S} as {trait_}>::{method}(&__ctx{call_args}).await",
        trait_ = api.class_name,
        method = m.rust_name,
    );

    if has_conn {
        w.line(&format!(
            "async fn __{cmd}<{GENERIC_S}>({extractors}) -> ::axum::response::Response",
            cmd = m.command,
            extractors = extractors.join(", "),
        ));
        w.line("where");
        write_where_bounds(w, api, any_conn_in_trait);
        w.line("{");
        w.indented(|w| {
            if needs_required_conn {
                w.line("let Some(__conn) = __caller.0.as_ref() else {");
                w.indented(|w| {
                    w.line(
                        "return (::axum::http::StatusCode::CONFLICT, \
                         \"draad: no live connection for this client\").into_response();",
                    );
                });
                w.line("};");
            }
            let body = if m.returns_result {
                format!("{call}.map(Json).into_response()")
            } else {
                format!("Json({call}).into_response()")
            };
            w.line(&body);
        });
        w.line("}");
        w.blank();
        return;
    }

    // ── No injected conn: clean Result<Json<Ok>, Err> / Json<Ok> shape ──
    let ok = result_ok_type(m.returns_result, &m.ret_rust);
    let return_ty = match (m.returns_result, m.err_rust.as_deref()) {
        (true, Some(err)) => format!("::std::result::Result<Json<{ok}>, {err}>"),
        (true, None) => format!("::std::result::Result<Json<{ok}>, ()>"),
        (false, _) => format!("Json<{ok}>"),
    };
    let body = if m.returns_result {
        format!("{call}.map(Json)")
    } else {
        format!("Json({call})")
    };

    w.line(&format!(
        "async fn __{cmd}<{GENERIC_S}>({extractors}) -> {return_ty}",
        cmd = m.command,
        extractors = extractors.join(", "),
    ));
    w.line("where");
    write_where_bounds(w, api, any_conn_in_trait);
    w.line("{");
    w.indented(|w| {
        w.line(&body);
    });
    w.line("}");
    w.blank();
}

fn write_apply_routes(w: &mut Writer, api: &Api, conn: bool) {
    // `pub` because the outer `pub use __draad_<ns>_routes::apply_routes`
    // re-exports it. The wrapper module is itself private, so this still
    // only escapes via the re-export.
    w.line(&format!(
        "pub fn apply_routes<{GENERIC_S}>(router: Router<{GENERIC_S}>) -> Router<{GENERIC_S}>"
    ));
    w.line("where");
    write_where_bounds(w, api, conn);
    w.line("{");
    w.indented(|w| {
        w.line("router");
        w.indented(|w| {
            for m in &api.methods {
                w.line(&format!(
                    ".route(\"/{ns}/{name}\", {verb_fn}(__{cmd}::<{GENERIC_S}>))",
                    ns = api.namespace,
                    name = m.rust_name,
                    verb_fn = m.verb.axum_fn(),
                    cmd = m.command,
                ));
            }
        });
    });
    w.line("}");
}

fn write_aggregator_header(w: &mut Writer) {
    w.line("// Generated by draad-codegen. Do not edit.");
    w.line("// Include via `draad::include_generated!(StateTy, BusTy)`.");
    w.blank();
    w.line("use ::axum::Router;");
    w.blank();
}

/// The aggregator no longer needs to import handler-side types: those
/// resolve inside each api module. It still needs `use crate::api::<mod>::*;`
/// for the modules whose payload types the event emitters reference.
fn write_aggregator_uses(
    w: &mut Writer,
    cfg: &Config,
    event_apis: &[EventApi],
    ty_modules: &[String],
) {
    let mut modules: BTreeSet<&str> = BTreeSet::new();
    for ev in event_apis {
        modules.insert(ev.module.as_str());
    }
    for module in ty_modules {
        modules.insert(module.as_str());
    }
    for module in &modules {
        w.line(&format!(
            "use {prefix}::{module}::*;",
            prefix = cfg.api_modules_prefix
        ));
    }
    if !modules.is_empty() {
        w.blank();
    }
}

/// `pub fn rpc_router() -> Router<__DraadState>` that builds an empty
/// `Router` and threads it through each api module's `apply_routes`.
fn write_rpc_router(w: &mut Writer, cfg: &Config, apis: &[Api]) {
    w.line(&format!("pub fn rpc_router() -> Router<{STATE_TY}> {{"));
    w.indented(|w| {
        // Collapse the per-module calls behind a single mutable router so
        // each `apply_routes` is one line. Inference handles `S = __DraadState`
        // from the binding's type — no turbofish needed.
        if apis.is_empty() {
            w.line("Router::new()");
        } else {
            w.line(&format!(
                "let mut router: Router<{STATE_TY}> = Router::new();"
            ));
            for api in apis {
                w.line(&format!(
                    "router = {prefix}::{module}::__draad_{namespace}_apply_routes(router);",
                    prefix = cfg.api_modules_prefix,
                    module = api.module,
                    namespace = api.namespace,
                ));
            }
            w.line("router");
        }
    });
    w.line("}");
    w.blank();
}

fn write_event_emitters(w: &mut Writer, event_apis: &[EventApi]) {
    for ev in event_apis {
        let pascal = capitalize(&snake_to_camel(&ev.namespace));
        w.line("#[derive(Clone)]");
        w.line(&format!("pub struct {pascal}Emitter {{ bus: {BUS_TY} }}"));
        w.blank();
        w.line(&format!("impl {pascal}Emitter {{"));
        w.indented(|w| {
            for e in &ev.events {
                let payload = last_path_segment(&e.payload_rust);
                w.line(&format!(
                    "/// Publishes the `{wire}` event to all WS subscribers.",
                    wire = e.wire
                ));
                w.line(&format!(
                    "pub fn emit_{name}(&self, payload: &{payload}) {{",
                    name = e.rust_name,
                ));
                w.indented(|w| {
                    w.line(&format!(
                        "self.bus.publish(\"{wire}\", payload);",
                        wire = e.wire
                    ));
                });
                w.line("}");
            }
        });
        w.line("}");
        w.blank();
    }

    w.line("#[derive(Clone)]");
    w.line("pub struct Events {");
    w.indented(|w| {
        for ev in event_apis {
            let pascal = capitalize(&snake_to_camel(&ev.namespace));
            w.line(&format!("pub {ns}: {pascal}Emitter,", ns = ev.namespace));
        }
    });
    w.line("}");
    w.blank();

    w.line("impl Events {");
    w.indented(|w| {
        let bus_param = if event_apis.is_empty() { "_bus" } else { "bus" };
        w.line(&format!("pub fn new({bus_param}: {BUS_TY}) -> Self {{"));
        w.indented(|w| {
            w.line("Self {");
            w.indented(|w| {
                for ev in event_apis {
                    let pascal = capitalize(&snake_to_camel(&ev.namespace));
                    w.line(&format!(
                        "{ns}: {pascal}Emitter {{ bus: bus.clone() }},",
                        ns = ev.namespace
                    ));
                }
            });
            w.line("}");
        });
        w.line("}");
    });
    w.line("}");
}

/// Emit a `pub mod urls { pub const … }` of path templates for every `#[raw]`
/// endpoint. A no-op when there are no raw endpoints.
fn write_url_consts(w: &mut Writer, raw_apis: &[RawApi]) {
    if raw_apis.iter().all(|r| r.methods.is_empty()) {
        return;
    }
    w.line("pub mod urls {");
    w.indented(|w| {
        for raw in raw_apis {
            for m in &raw.methods {
                w.line(&format!(
                    "pub const {konst}: &str = {path:?};",
                    konst = m.rust_name.to_uppercase(),
                    path = m.path_template,
                ));
            }
        }
    });
    w.line("}");
    w.blank();
}
