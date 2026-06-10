//! AST parsing: turns `#[api]` / `#[events]` traits into the internal
//! [`Api`](super::model::Api) / [`EventApi`](super::model::EventApi)
//! representation. Type-shape conversion lives in
//! [`super::types`]; this module just walks `syn` and assembles the
//! model.

use std::collections::BTreeSet;
use syn::{FnArg, ItemTrait, Pat, ReturnType, TraitItem, Type};

use super::model::{Api, Event, EventApi, Method, Param, Verb};
use super::scan::{attr_path_matches, extract_docs};
use super::types::{
    extract_result_err_ts, extract_result_inner_ts, is_query_safe, rust_type_to_string,
    rust_type_to_ts,
};
use super::util::snake_to_camel;

pub(super) fn parse_trait(
    t: &ItemTrait,
    namespace: String,
    module: String,
    imports: &mut BTreeSet<String>,
) -> Api {
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
        let params = parse_params(&method.sig.inputs, &method.sig.ident, imports);
        let Return {
            ret_ts,
            ret_rust,
            returns_result,
            err_rust,
            err_ts,
        } = parse_return(&method.sig.output, imports);

        if !verb.has_body() {
            for p in &params {
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

pub(super) fn parse_events_trait(
    t: &ItemTrait,
    namespace: String,
    module: String,
    imports: &mut BTreeSet<String>,
) -> EventApi {
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
            payload_ts = rust_type_to_ts(&pat.ty, imports);
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

fn parse_params(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
    method_ident: &syn::Ident,
    imports: &mut BTreeSet<String>,
) -> Vec<Param> {
    let mut params = Vec::new();
    for arg in inputs {
        let FnArg::Typed(pat) = arg else { continue };
        let name = match &*pat.pat {
            Pat::Ident(p) => p.ident.to_string(),
            _ => panic!("unsupported param pattern in {method_ident}"),
        };
        params.push(Param {
            name,
            ts_type: rust_type_to_ts(&pat.ty, imports),
            rust_type: rust_type_to_string(&pat.ty),
            docs: extract_docs(&pat.attrs),
        });
    }
    params
}

struct Return {
    ret_ts: String,
    ret_rust: String,
    returns_result: bool,
    err_rust: Option<String>,
    err_ts: Option<String>,
}

fn parse_return(output: &ReturnType, imports: &mut BTreeSet<String>) -> Return {
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
    let ret_ts = extract_result_inner_ts(ty, imports);
    let (err_rust, err_ts) = if returns_result {
        let err_ts = extract_result_err_ts(ty, imports);
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
