//! AST parsing: turns `#[api]` / `#[events]` traits into the internal
//! [`Api`](super::model::Api) / [`EventApi`](super::model::EventApi)
//! representation. Type-shape conversion lives in
//! [`super::types`]; this module just walks `syn` and assembles the
//! model.

use syn::{FnArg, GenericArgument, ItemTrait, Pat, PathArguments, ReturnType, TraitItem, Type};

use super::model::{
    Api, ConnInject, Event, EventApi, Method, Param, PathSeg, RawApi, RawEndpoint, Verb,
};
use super::scan::{attr_path_matches, extract_docs, extract_raw_path};
use super::types::{
    extract_result_err_ts, extract_result_inner_ts, is_query_safe, rust_type_to_string,
    rust_type_to_ts,
};
use super::util::snake_to_camel;

pub(super) fn parse_trait(t: &ItemTrait, namespace: String, module: String) -> Api {
    let mut methods = Vec::new();
    for item in &t.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };
        let rust_name = method.sig.ident.to_string();
        let ts_name = snake_to_camel(&rust_name);
        let command = format!("{namespace}_{rust_name}");
        let docs = extract_docs(&method.attrs);
        let verb = parse_verb(&method.attrs, &rust_name);
        let params = parse_params(&method.sig.inputs, &method.sig.ident);
        let Return {
            ret_ts,
            ret_rust,
            returns_result,
            err_rust,
            err_ts,
        } = parse_return(&method.sig.output);

        if !verb.has_body() {
            for p in &params {
                if p.conn.is_some() {
                    continue;
                }
                if !is_query_safe(&p.rust_type) {
                    panic!(
                        "method `{rust_name}` parameter `{name}: {ty}` is not \
                         query-string-safe. GET/DELETE methods may only take \
                         primitives (String, bool, integer, float), \
                         Option<primitive>, or Vec<primitive>.",
                        name = p.name,
                        ty = p.rust_type,
                    );
                }
            }
        }

        methods.push(Method {
            rust_name,
            ts_name,
            command,
            params,
            ret_ts,
            ret_rust,
            returns_result,
            err_rust,
            err_ts,
            docs,
            verb,
        });
    }
    Api {
        namespace,
        module,
        class_name: t.ident.to_string(),
        docs: extract_docs(&t.attrs),
        methods,
    }
}

pub(super) fn parse_events_trait(t: &ItemTrait, namespace: String, module: String) -> EventApi {
    let mut events = Vec::new();
    for item in &t.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };
        let rust_name = method.sig.ident.to_string();
        let docs = extract_docs(&method.attrs);
        let mut payload_ts = "void".to_string();
        let mut payload_rust = "()".to_string();
        for arg in &method.sig.inputs {
            let FnArg::Typed(pat) = arg else { continue };
            payload_ts = rust_type_to_ts(&pat.ty);
            payload_rust = rust_type_to_string(&pat.ty);
        }
        events.push(Event {
            ts_name: snake_to_camel(&rust_name),
            wire: format!("{namespace}/{rust_name}"),
            docs,
            rust_name,
            payload_ts,
            payload_rust,
        });
    }
    EventApi {
        namespace,
        module,
        class_name: t.ident.to_string(),
        docs: extract_docs(&t.attrs),
        events,
    }
}

pub(super) fn parse_raw_trait(t: &ItemTrait) -> RawApi {
    let mut methods = Vec::new();
    for item in &t.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };
        let rust_name = method.sig.ident.to_string();
        let path_template = extract_raw_path(&method.attrs).unwrap_or_else(|| {
            panic!("#[raw] method `{rust_name}` is missing a `#[get(\"/...\")]` path attribute")
        });
        let params = parse_params(&method.sig.inputs, &method.sig.ident);
        for p in &params {
            if !is_query_safe(&p.rust_type) {
                panic!(
                    "#[raw] method `{rust_name}` path param `{name}: {ty}` must be a primitive \
                     (String, bool, integer, float).",
                    name = p.name,
                    ty = p.rust_type,
                );
            }
        }
        let segments = parse_path_template(&path_template, &rust_name);
        methods.push(RawEndpoint {
            ts_name: snake_to_camel(&rust_name),
            rust_name,
            path_template,
            params,
            segments,
            docs: extract_docs(&method.attrs),
        });
    }
    RawApi {
        class_name: t.ident.to_string(),
        docs: extract_docs(&t.attrs),
        methods,
    }
}

/// Parse `/api/stream/{info_hash}/{file_idx}` into ordered segments:
/// `{name}` → `Param{catch_all:false}`, `{*name}` → `Param{catch_all:true}`,
/// everything else is `Static`. Re-joining the segments reproduces the
/// template. Path templates are ASCII (Axum route syntax).
fn parse_path_template(template: &str, method: &str) -> Vec<PathSeg> {
    let mut segs = Vec::new();
    let mut buf = String::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let end = template[i..].find('}').map(|o| i + o).unwrap_or_else(|| {
                panic!("#[raw] method `{method}`: unterminated `{{` in path `{template}`")
            });
            if !buf.is_empty() {
                segs.push(PathSeg::Static(std::mem::take(&mut buf)));
            }
            let inner = &template[i + 1..end];
            let (name, catch_all) = match inner.strip_prefix('*') {
                Some(rest) => (rest.to_string(), true),
                None => (inner.to_string(), false),
            };
            segs.push(PathSeg::Param { name, catch_all });
            i = end + 1;
        } else {
            buf.push(bytes[i] as char);
            i += 1;
        }
    }
    if !buf.is_empty() {
        segs.push(PathSeg::Static(buf));
    }
    segs
}

fn parse_params(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
    method_ident: &syn::Ident,
) -> Vec<Param> {
    let mut params = Vec::new();
    for arg in inputs {
        let FnArg::Typed(pat) = arg else { continue };
        let name = match &*pat.pat {
            Pat::Ident(p) => p.ident.to_string(),
            _ => panic!("unsupported param pattern in {method_ident}"),
        };
        // An injected `&Conn` / `Option<&Conn>` is server-filled — it's not a
        // wire arg, so don't map it to TS or check query-safety (and don't let
        // its type leak into the import set).
        if let Some(conn) = conn_inject(&pat.ty) {
            params.push(Param {
                name,
                ts_type: String::new(),
                rust_type: rust_type_to_string(&pat.ty),
                docs: extract_docs(&pat.attrs),
                conn: Some(conn),
            });
            continue;
        }
        params.push(Param {
            name,
            ts_type: rust_type_to_ts(&pat.ty),
            rust_type: rust_type_to_string(&pat.ty),
            docs: extract_docs(&pat.attrs),
            conn: None,
        });
    }
    params
}

/// Detect an injected connection parameter: `&Conn` / `&mut Conn` (required)
/// or `Option<&Conn>` (optional). Matched by the type's final path segment
/// being `Conn`, so `draad::runtime::Conn` works too.
fn conn_inject(ty: &Type) -> Option<ConnInject> {
    match ty {
        Type::Reference(r) if last_seg_is_conn(&r.elem) => Some(ConnInject { required: true }),
        Type::Path(p) => {
            let seg = p.path.segments.last()?;
            if seg.ident != "Option" {
                return None;
            }
            let PathArguments::AngleBracketed(args) = &seg.arguments else {
                return None;
            };
            let GenericArgument::Type(Type::Reference(r)) = args.args.first()? else {
                return None;
            };
            last_seg_is_conn(&r.elem).then_some(ConnInject { required: false })
        }
        _ => None,
    }
}

fn last_seg_is_conn(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Conn"))
}

struct Return {
    ret_ts: String,
    ret_rust: String,
    returns_result: bool,
    err_rust: Option<String>,
    err_ts: Option<String>,
}

fn parse_return(output: &ReturnType) -> Return {
    let ReturnType::Type(_, ty) = output else {
        return Return {
            ret_ts: "void".into(),
            ret_rust: "()".into(),
            returns_result: false,
            err_rust: None,
            err_ts: None,
        };
    };
    let returns_result = matches!(ty.as_ref(), Type::Path(p) if {
        let seg = p.path.segments.last().unwrap();
        seg.ident == "Result" || seg.ident == "RpcResult"
    });
    let ret_rust = rust_type_to_string(ty);
    let ret_ts = extract_result_inner_ts(ty);
    let (err_rust, err_ts) = if returns_result {
        let err_ts = extract_result_err_ts(ty);
        let err_rust = super::types::result_err_type(&ret_rust);
        (err_rust, err_ts)
    } else {
        (None, None)
    };
    Return {
        ret_ts,
        ret_rust,
        returns_result,
        err_rust,
        err_ts,
    }
}

/// Pull a verb override off a trait method's attribute list. Recognises
/// `#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` (bare or
/// `#[draad::...]`-qualified). At most one may appear; otherwise we panic
/// with the offending method name so the build fails loudly.
fn parse_verb(attrs: &[syn::Attribute], method_name: &str) -> Verb {
    const CANDIDATES: &[(&str, Verb)] = &[
        ("get", Verb::Get),
        ("post", Verb::Post),
        ("put", Verb::Put),
        ("patch", Verb::Patch),
        ("delete", Verb::Delete),
    ];
    let mut found: Option<(&'static str, Verb)> = None;
    for attr in attrs {
        for (name, verb) in CANDIDATES {
            if attr_path_matches(attr.path(), name) {
                if let Some((prev, _)) = found {
                    panic!(
                        "method `{method_name}` has conflicting verb attributes \
                         `#[{prev}]` and `#[{name}]`; pick one"
                    );
                }
                found = Some((name, *verb));
            }
        }
    }
    found.map(|(_, v)| v).unwrap_or(Verb::Post)
}
