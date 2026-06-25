//! Rust ⇄ TypeScript type mapping. `parse_trait` and `parse_events_trait`
//! consume `syn::Type` nodes and ask this module to turn them into either
//! a TS type expression (for the generated client) or a normalised Rust
//! type string (for the generated handler args struct).
//!
//! All knowledge of which `Verb` arg shapes are query-string-safe lives
//! here too, since it's a question about Rust types.

use syn::{GenericArgument, PathArguments, Type};

/// Map a Rust type to its TypeScript counterpart
pub(super) fn rust_type_to_ts(ty: &Type) -> String {
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
                        .map(rust_type_to_ts)
                        .unwrap_or_else(|| "unknown".into());
                    format!("{inner}[]")
                }
                "Option" => {
                    let inner = first_generic(&seg.arguments)
                        .map(rust_type_to_ts)
                        .unwrap_or_else(|| "unknown".into());
                    format!("{inner} | null")
                }
                // TODO: Add support for custom type mapping
                _ => name,
            }
        }
        _ => "unknown".into(),
    }
}

/// Stringify a `syn::Type` in the same shape the generated Rust file
/// expects (compact, no stray spaces around `<`, `:`, etc.).
pub(super) fn rust_type_to_string(ty: &Type) -> String {
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

/// Map the *Ok* half of a `Result<T, _>` return to TS. For non-Result
/// returns this is equivalent to [`rust_type_to_ts`] on the whole type.
pub(super) fn extract_result_inner_ts(ty: &Type) -> String {
    if let Type::Path(p) = ty {
        let seg = p.path.segments.last().unwrap();
        if seg.ident == "Result" || seg.ident == "RpcResult" {
            if let Some(inner) = first_generic(&seg.arguments) {
                return rust_type_to_ts(inner);
            }
        }
    }
    rust_type_to_ts(ty)
}

/// Map the *Err* half of a `Result<_, E>` return to TS. Returns `None`
/// for non-`Result` returns or unrecognised shapes.
pub(super) fn extract_result_err_ts(ty: &Type) -> Option<String> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Result" && seg.ident != "RpcResult" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args
        .iter()
        .filter_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .nth(1)
        .map(rust_type_to_ts)
}

/// Given the stringified return type of a method (already normalised by
/// [`rust_type_to_string`]), pull out the `Ok` half of a `Result<Ok, _>`
/// so the generated handler can wrap it in `Json<Ok>`. Non-result
/// returns pass through unchanged.
pub(super) fn result_ok_type(returns_result: bool, ret_rust: &str) -> String {
    if !returns_result {
        return ret_rust.to_string();
    }
    result_generic_arg(ret_rust, 0).unwrap_or_else(|| "()".into())
}

/// Pull out the `Err` half of `Result<_, Err>`. Returns `None` for non-
/// `Result` returns (or malformed ones — defensive, shouldn't happen).
pub(super) fn result_err_type(ret_rust: &str) -> Option<String> {
    result_generic_arg(ret_rust, 1)
}

/// Helper: parse `ret_rust` as a `Result<A, B>`-shaped type and return
/// the Nth generic argument re-rendered via [`rust_type_to_string`].
fn result_generic_arg(ret_rust: &str, index: usize) -> Option<String> {
    let ty = syn::parse_str::<Type>(ret_rust).ok()?;
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args
        .iter()
        .filter_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .nth(index)
        .map(rust_type_to_string)
}

pub(super) fn first_generic(args: &PathArguments) -> Option<&Type> {
    if let PathArguments::AngleBracketed(a) = args {
        for arg in &a.args {
            if let GenericArgument::Type(t) = arg {
                return Some(t);
            }
        }
    }
    None
}

// ── query-string safety ──────────────────────────────────────────────
//
// GET/DELETE methods deserialise their args from the query string via
// `axum::extract::Query`, which delegates to `serde_urlencoded`. Only
// primitives, `Option<primitive>`, and `Vec<primitive>` round-trip
// cleanly — anything else silently fails at runtime. We reject the
// rest at codegen time.

pub(super) fn is_query_safe(rust_type: &str) -> bool {
    let t = rust_type.trim();
    if is_primitive(t) {
        return true;
    }
    if let Some(inner) = strip_wrapper(t, "Option").or_else(|| strip_wrapper(t, "Vec")) {
        return is_primitive(inner.trim());
    }
    false
}

fn is_primitive(t: &str) -> bool {
    matches!(
        t,
        "String"
            | "str"
            | "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
    )
}

/// Whether a (normalized) Rust type string is a numeric primitive. Used by the
/// `#[raw]` URL-builder to decide whether a path param is interpolated raw
/// (numbers) or `encodeURIComponent`'d (strings).
pub(super) fn is_numeric_rust(rust_type: &str) -> bool {
    matches!(
        rust_type.trim(),
        "u8" | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
    )
}

/// `strip_wrapper("Option<String>", "Option")` → `Some("String")`.
fn strip_wrapper<'a>(t: &'a str, name: &str) -> Option<&'a str> {
    let rest = t.strip_prefix(name)?.trim_start();
    let inner = rest.strip_prefix('<')?.strip_suffix('>')?;
    Some(inner)
}
