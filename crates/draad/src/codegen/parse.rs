//! AST parsing: turns `#[api]` / `#[events]` traits into the internal
//! [`Api`](super::model::Api) / [`EventApi`](super::model::EventApi)
//! representation, plus the Rust→TS type-mapping helpers used along the
//! way.

use std::collections::BTreeSet;
use syn::{FnArg, GenericArgument, ItemTrait, Pat, PathArguments, ReturnType, TraitItem, Type};

use super::model::{Api, Event, EventApi, Method, Param};
use super::scan::extract_docs;

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

        let mut params = Vec::new();
        for arg in &method.sig.inputs {
            let FnArg::Typed(pat) = arg else { continue };
            let name = match &*pat.pat {
                Pat::Ident(p) => p.ident.to_string(),
                _ => panic!("unsupported param pattern in {}", method.sig.ident),
            };
            let docs = extract_docs(&pat.attrs);
            let ts_type = rust_type_to_ts(&pat.ty, imports);
            let rust_type = rust_type_to_string(&pat.ty);
            params.push(Param {
                name,
                ts_type,
                rust_type,
                docs,
            });
        }

        let (ret_ts, ret_rust, returns_result) = match &method.sig.output {
            ReturnType::Type(_, ty) => {
                let is_result = matches!(ty.as_ref(), Type::Path(p) if {
                    let seg = p.path.segments.last().unwrap();
                    seg.ident == "Result" || seg.ident == "RpcResult"
                });
                (
                    extract_result_inner_ts(ty, imports),
                    rust_type_to_string(ty),
                    is_result,
                )
            }
            ReturnType::Default => ("void".into(), "()".into(), false),
        };

        methods.push(Method {
            rust_name,
            ts_name,
            command,
            params,
            ret_ts,
            ret_rust,
            returns_result,
            docs,
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

impl Method {
    pub(super) fn ret_rust_ok(&self) -> String {
        if !self.returns_result {
            return self.ret_rust.clone();
        }
        match syn::parse_str::<Type>(&self.ret_rust) {
            Ok(Type::Path(p)) => {
                let seg = p.path.segments.last().unwrap();
                if let Some(inner) = first_generic(&seg.arguments) {
                    rust_type_to_string(inner)
                } else {
                    "()".into()
                }
            }
            _ => "()".into(),
        }
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

pub(super) fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn rust_type_to_string(ty: &Type) -> String {
    use quote::ToTokens;
    let raw = ty.to_token_stream().to_string();
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b != b' ' {
            out.push(b as char);
            continue;
        }
        let prev = bytes.get(i.wrapping_sub(1)).copied().unwrap_or(0);
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        let prev_join = matches!(prev, b'<' | b'(' | b':' | b',' | b'&');
        let next_join = matches!(next, b'<' | b'>' | b'(' | b')' | b':' | b',' | b';');
        if prev_join || next_join {
            continue;
        }
        out.push(' ');
    }
    out
}

fn rust_type_to_ts(ty: &Type, imports: &mut BTreeSet<String>) -> String {
    match ty {
        Type::Tuple(t) if t.elems.is_empty() => "void".into(),
        Type::Path(p) => {
            let seg = p.path.segments.last().unwrap();
            let name = seg.ident.to_string();
            match name.as_str() {
                "String" | "str" => "string".into(),
                "bool" => "boolean".into(),
                "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize"
                | "f32" | "f64" => "number".into(),
                "Vec" => {
                    let inner = first_generic(&seg.arguments)
                        .map(|t| rust_type_to_ts(t, imports))
                        .unwrap_or_else(|| "unknown".into());
                    format!("{inner}[]")
                }
                "Option" => {
                    let inner = first_generic(&seg.arguments)
                        .map(|t| rust_type_to_ts(t, imports))
                        .unwrap_or_else(|| "unknown".into());
                    format!("{inner} | null")
                }
                _ => {
                    imports.insert(name.clone());
                    name
                }
            }
        }
        _ => "unknown".into(),
    }
}

fn extract_result_inner_ts(ty: &Type, imports: &mut BTreeSet<String>) -> String {
    if let Type::Path(p) = ty {
        let seg = p.path.segments.last().unwrap();
        if seg.ident == "Result" || seg.ident == "RpcResult" {
            if let Some(inner) = first_generic(&seg.arguments) {
                return rust_type_to_ts(inner, imports);
            }
        }
    }
    rust_type_to_ts(ty, imports)
}

fn first_generic(args: &PathArguments) -> Option<&Type> {
    if let PathArguments::AngleBracketed(a) = args {
        for arg in &a.args {
            if let GenericArgument::Type(t) = arg {
                return Some(t);
            }
        }
    }
    None
}
