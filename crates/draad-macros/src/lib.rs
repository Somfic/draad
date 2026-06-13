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

/// Derives serde + the small set of standard traits on a wire type. The
/// TypeScript counterpart is generated separately by the codegen — it
/// parses this very item out of the source file and renders the TS
/// declaration directly, no ts-rs / runtime export step required.
#[proc_macro_attribute]
pub fn ty(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    quote! {
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
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

/// Declares a set of browser-direct binary/streaming HTTP endpoints (range
/// video, images, HLS segments…). Like [`events`], the trait is a pure
/// manifest — never implemented, erased at compile time, read from source by
/// the codegen. Each method carries a path template via a `#[get("/path/{x}")]`
/// marker; the codegen emits a TypeScript URL-builder (`api.urls.*`) and Rust
/// path constants (`crate::urls::*`). draad does **not** serve the bytes — mount
/// your own Axum handlers against those constants.
#[proc_macro_attribute]
pub fn raw(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Erase the trait (and its `#[get("…")]` method markers) entirely; the
    // codegen reads the path templates from source.
    let _input = parse_macro_input!(item as ItemTrait);
    TokenStream::new()
}

/// Drive the whole codegen pipeline at macro-expansion time.
///
/// ```ignore
/// draad::include_generated!(AppContext, EventBus);
/// ```
///
/// Walks the consumer crate's `src/`, runs scan/parse/emit, splices the
/// resulting handler tree + router into the call site, and writes the
/// TypeScript client to `frontend/src/lib/schema/index.ts` as a side
/// effect. No `build.rs`, no `cargo test` step.
///
/// `include_bytes!` invalidation guards are emitted for every scanned
/// file so rustc re-expands when sources change.
#[proc_macro]
pub fn include_generated(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as IncludeGeneratedArgs);
    expand_include_generated(args).unwrap_or_else(|e| e.to_compile_error().into())
}

struct IncludeGeneratedArgs {
    state: syn::Type,
    bus: Option<syn::Type>,
}

impl syn::parse::Parse for IncludeGeneratedArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let state: syn::Type = input.parse()?;
        let bus = if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
            if input.is_empty() {
                None
            } else {
                Some(input.parse()?)
            }
        } else {
            None
        };
        Ok(IncludeGeneratedArgs { state, bus })
    }
}

fn expand_include_generated(args: IncludeGeneratedArgs) -> syn::Result<TokenStream> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "CARGO_MANIFEST_DIR not set; `include_generated!` must run under cargo",
        )
    })?;

    let cfg = draad_codegen::Config::new().root(&manifest_dir);
    let artifacts = draad_codegen::run(&cfg).map_err(|e| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("draad codegen failed: {e}"),
        )
    })?;

    if let Some(ts) = &artifacts.ts_source {
        write_if_changed(&artifacts.ts_path, ts).map_err(|e| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "draad: failed to write {}: {e}",
                    artifacts.ts_path.display()
                ),
            )
        })?;
    }

    let rust = syn::parse_str::<proc_macro2::TokenStream>(&artifacts.rust_source).map_err(|e| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("draad codegen produced invalid Rust: {e}"),
        )
    })?;

    let state = &args.state;
    let bus = args.bus.unwrap_or_else(|| syn::parse_quote!(()));

    // Tell rustc which source files this macro read so the cached
    // expansion gets invalidated when any of them change. We
    // deliberately do *not* include the generated TS output here — its
    // mtime is updated on every run, which would trap incremental
    // compilation in a re-run loop.
    let guards: Vec<proc_macro2::TokenStream> = artifacts
        .files_read
        .iter()
        .map(|p| {
            let s = p.to_string_lossy().into_owned();
            quote! { const _: &[u8] = include_bytes!(#s); }
        })
        .collect();

    let expanded = quote! {
        #[allow(
            non_camel_case_types,
            dead_code,
            unused_imports,
            clippy::needless_lifetimes,
        )]
        mod __draad_generated {
            // Bind the consumer's state/bus types to the internal names
            // the generated code references. The aliases stay inside the
            // module so they don't pollute the caller's namespace.
            pub(super) type __DraadState = #state;
            pub(super) type __DraadBus = #bus;
            #rust
        }
        pub use __draad_generated::*;

        // Tell rustc which files this macro depended on so the cached
        // expansion is invalidated when any of them change. The
        // surrounding `const _` discards the bytes — we only need the
        // tracking side effect.
        #(#guards)*
    };

    Ok(expanded.into())
}

fn write_if_changed(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    std::fs::write(path, content)
}
