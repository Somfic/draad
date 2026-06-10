//! Trait-driven RPC schema for Rust↔TypeScript.
//!
//! These attribute macros mark structs, enums and traits as part of the
//! wire-level API. The companion `draad-codegen` crate scans your source at
//! build time and emits:
//!
//!  - Axum handlers + a `rpc_router()` for every `#[api]` trait method.
//!  - A `*Emitter` struct per `#[events]` namespace that publishes
//!    onto a user-provided event bus (typically a `tokio::sync::broadcast`).
//!  - A per-namespace TypeScript file with a typed client.
//!
//! ## Required runtime dependencies on the consumer crate
//!
//! ```toml
//! serde = { version = "1", features = ["derive"] }
//! ts-rs = "11"
//! async-trait = "0.1"
//! ```
//!
//! ## Example
//!
//! ```ignore
//! use draad::{api, events, ty};
//!
//! #[ty]
//! pub struct Hit { pub id: i64, pub title: String }
//!
//! #[api(namespace = "search")]
//! pub trait SearchApi {
//!     async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
//! }
//!
//! #[events(namespace = "search")]
//! pub trait SearchEvents {
//!     /// Topic: `search_progress`.
//!     fn progress(payload: Progress);
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::{
    parse_macro_input, punctuated::Punctuated, DeriveInput, Expr, ExprLit, ItemTrait, Lit, Meta,
    Token,
};

/// Derives the standard set of traits for any type that crosses the
/// frontend/backend boundary: serde for the wire, ts-rs for the TypeScript
/// counterpart. Per-type ts-rs bindings land in the consumer crate's
/// `target/draad-bindings/_per_type/` — the absolute path is baked into
/// the emitted `#[ts(export_to = ...)]` at macro expansion time, so
/// callers don't need to set `TS_RS_EXPORT_DIR` or any other env var.
#[proc_macro_attribute]
pub fn ty(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    // CARGO_MANIFEST_DIR is set by cargo on every rustc invocation to
    // the path of the crate currently being compiled — i.e. the
    // consumer's package dir during proc-macro expansion. Falling back
    // to a relative path keeps the macro usable in odd contexts where
    // the var isn't set (e.g. rust-analyzer in some configurations);
    // ts-rs will resolve it against its own default base then.
    let export_to = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => format!("{dir}/target/draad-bindings/_per_type/"),
        Err(_) => "_per_type/".to_string(),
    };
    quote! {
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::ts_rs::TS)]
        #[ts(export, export_to = #export_to)]
        #input
    }
    .into()
}

fn parse_namespace(attr: TokenStream, macro_name: &str) -> Result<String, TokenStream> {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let metas = match parser.parse(attr) {
        Ok(m) => m,
        Err(e) => return Err(e.to_compile_error().into()),
    };

    let mut namespace: Option<String> = None;
    for meta in metas {
        if let Meta::NameValue(nv) = meta {
            if nv.path.is_ident("namespace") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = nv.value
                {
                    namespace = Some(s.value());
                }
            }
        }
    }

    namespace.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("#[{macro_name}] requires `namespace = \"...\"`"),
        )
        .to_compile_error()
        .into()
    })
}

/// On a `trait`: declares an API namespace (requires `namespace = "..."`)
/// and injects `#[async_trait]`. On an `impl ... for State`: no args, just
/// shorthand for `#[async_trait]`. The codegen reads the namespace from
/// source.
///
/// `#[get]` / `#[post]` / `#[put]` / `#[patch]` / `#[delete]` markers on
/// trait methods (or impl methods) are treated as helper attributes of
/// `#[api]`: the codegen sees them in the raw source (it parses the file
/// directly), and this macro strips them from its expansion so they
/// never reach rustc as standalone attributes. That avoids name clashes
/// with `axum::routing::{get, post, ...}` and friends.
#[proc_macro_attribute]
pub fn api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::Item);
    match input {
        syn::Item::Trait(mut t) => {
            if let Err(ts) = parse_namespace(attr, "api") {
                return ts;
            }
            for item in &mut t.items {
                if let syn::TraitItem::Fn(m) = item {
                    m.attrs.retain(|a| !is_verb_marker(a));
                }
            }
            quote! {
                #[::async_trait::async_trait]
                #t
            }
            .into()
        }
        syn::Item::Impl(mut i) => {
            for item in &mut i.items {
                if let syn::ImplItem::Fn(m) = item {
                    m.attrs.retain(|a| !is_verb_marker(a));
                }
            }
            quote! {
                #[::async_trait::async_trait]
                #i
            }
            .into()
        }
        other => syn::Error::new_spanned(other, "#[api] expects a trait or impl block")
            .to_compile_error()
            .into(),
    }
}

/// Bare `#[get]` / `#[draad::get]` and the four other verbs. Mirrors
/// `attr_path_matches` over in the codegen crate, but inlined here so
/// `draad-macros` stays self-contained.
fn is_verb_marker(attr: &syn::Attribute) -> bool {
    const VERBS: &[&str] = &["get", "post", "put", "patch", "delete"];
    let path = attr.path();
    if let Some(ident) = path.get_ident() {
        return VERBS.contains(&ident.to_string().as_str());
    }
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    segs.len() == 2 && segs[0] == "draad" && VERBS.contains(&segs[1].as_str())
}

/// Declares a namespace of backend→frontend events. The annotated trait is
/// a pure manifest consumed by the codegen, never implemented, erased at
/// compile time. Codegen reads the trait from source, not from expansion.
#[proc_macro_attribute]
pub fn events(attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Err(ts) = parse_namespace(attr, "events") {
        return ts;
    }
    let _input = parse_macro_input!(item as ItemTrait);
    TokenStream::new()
}

