//! File scanning + AST attribute helpers used by the orchestrator and the
//! emitters.

use std::fs;
use std::path::{Path, PathBuf};
use syn::{Expr, ExprLit, ItemTrait, Lit, Meta};

pub(super) fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

pub(super) fn extract_attr_namespace(t: &ItemTrait, attr_name: &str) -> Option<String> {
    for attr in &t.attrs {
        if !attr_path_matches(attr.path(), attr_name) {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let parser = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated;
        let metas = match syn::parse::Parser::parse2(parser, list.tokens.clone()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for meta in metas {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("namespace") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        return Some(s.value());
                    }
                }
            }
        }
    }
    None
}

pub(super) fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| attr_path_matches(a.path(), name))
}

/// Matches `#[name]` or `#[draad::name]`. We don't accept arbitrary path
/// prefixes, only the bare ident and the crate-qualified form, so we
/// can't be tricked by an unrelated `#[foo::api]` from another crate.
pub(super) fn attr_path_matches(path: &syn::Path, name: &str) -> bool {
    if path.is_ident(name) {
        return true;
    }
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    segs.len() == 2 && segs[0] == "draad" && segs[1] == name
}

/// Pull the path template out of a `#[raw]` method's `#[get("/…")]` marker
/// (any of the five verb names, bare or `draad::`-qualified). Matches only when
/// the attr is a `Meta::List` whose body parses as a string literal — i.e.
/// `#[get("/x")]`, not the bare `#[get]` verb marker used on `#[api]` traits.
/// Returns the first match.
pub(super) fn extract_raw_path(attrs: &[syn::Attribute]) -> Option<String> {
    const VERBS: &[&str] = &["get", "post", "put", "patch", "delete"];
    for attr in attrs {
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        if !VERBS.iter().any(|v| attr_path_matches(attr.path(), v)) {
            continue;
        }
        if let Ok(s) = syn::parse2::<syn::LitStr>(list.tokens.clone()) {
            return Some(s.value());
        }
    }
    None
}

pub(super) fn extract_docs(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) = &nv.value
        else {
            continue;
        };
        let text = s.value();
        let trimmed = text.strip_prefix(' ').unwrap_or(&text);
        lines.push(trimmed.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn attr_from(item: syn::ItemStruct) -> syn::Attribute {
        item.attrs.into_iter().next().expect("attr present")
    }

    #[test]
    fn matcher_accepts_bare_ident() {
        let s: syn::ItemStruct = parse_quote! { #[api] struct X; };
        assert!(attr_path_matches(attr_from(s).path(), "api"));
    }

    #[test]
    fn matcher_accepts_draad_qualified_path() {
        let s: syn::ItemStruct = parse_quote! { #[draad::api] struct X; };
        assert!(attr_path_matches(attr_from(s).path(), "api"));
    }

    #[test]
    fn matcher_rejects_foreign_crate_path() {
        let s: syn::ItemStruct = parse_quote! { #[other::api] struct X; };
        assert!(!attr_path_matches(attr_from(s).path(), "api"));
    }

    #[test]
    fn matcher_rejects_wrong_name() {
        let s: syn::ItemStruct = parse_quote! { #[events] struct X; };
        assert!(!attr_path_matches(attr_from(s).path(), "api"));
    }

    #[test]
    fn has_attr_finds_ty_on_struct() {
        let s: syn::ItemStruct = parse_quote! { #[ty] struct Hit { id: i64 } };
        assert!(has_attr(&s.attrs, "ty"));
        assert!(!has_attr(&s.attrs, "api"));
    }

    #[test]
    fn has_attr_finds_ty_on_enum() {
        let e: syn::ItemEnum = parse_quote! { #[ty] enum Kind { A, B } };
        assert!(has_attr(&e.attrs, "ty"));
        assert!(!has_attr(&e.attrs, "api"));
    }

    #[test]
    fn extract_namespace_bare_form() {
        let t: ItemTrait = parse_quote! {
            #[api(namespace = "search")]
            trait SearchApi {}
        };
        assert_eq!(extract_attr_namespace(&t, "api").as_deref(), Some("search"));
    }

    #[test]
    fn extract_namespace_qualified_form() {
        let t: ItemTrait = parse_quote! {
            #[draad::api(namespace = "search")]
            trait SearchApi {}
        };
        assert_eq!(extract_attr_namespace(&t, "api").as_deref(), Some("search"));
    }

    #[test]
    fn extract_namespace_returns_none_when_missing_args() {
        let t: ItemTrait = parse_quote! {
            #[api]
            trait SearchApi {}
        };
        assert_eq!(extract_attr_namespace(&t, "api"), None);
    }
}
