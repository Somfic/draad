//! Render a `#[ty]` Rust struct or enum as a TypeScript `export type`
//! declaration. Replaces the ts-rs / per-type-file dance that used to
//! mediate this.
//!
//! Coverage is deliberately narrow — flat wire-DTO style, matching how
//! `#[ty]` is meant to be used:
//!
//! - Named-field structs.
//! - Unit-only enums with optional `#[serde(rename_all = "...")]`.
//! - Field types: primitives, `String`, `Option<T>`, `Vec<T>`, references
//!   to other `#[ty]` items (rendered by bare name).
//! - Doc comments → jsdoc on the type itself and on each field.
//!
//! Unsupported shapes panic with a clear message that says exactly which
//! type and which feature tripped the limit, so the build fails loudly
//! instead of silently emitting wrong TS.

use std::collections::BTreeSet;

use syn::{Attribute, Expr, ExprLit, Fields, Item, ItemEnum, ItemStruct, Lit, Meta, Token};

use super::scan::extract_docs;
use super::types::rust_type_to_ts;

/// Render a single `#[ty]` item as `export type Foo = ...;\n\n`.
/// `imports` is appended to with any user-defined types referenced so
/// the caller can verify the references resolve in the unified file.
pub(super) fn emit_ty_decl(item: &Item, imports: &mut BTreeSet<String>) -> String {
    match item {
        Item::Struct(s) => emit_struct(s, imports),
        Item::Enum(e) => emit_enum(e),
        _ => unreachable!("scanner only collects #[ty] structs/enums"),
    }
}

fn emit_struct(s: &ItemStruct, imports: &mut BTreeSet<String>) -> String {
    let name = s.ident.to_string();
    if !s.generics.params.is_empty() {
        panic!("#[ty] does not support generic types (struct `{name}` has type/lifetime params)");
    }
    let fields = match &s.fields {
        Fields::Named(named) => named,
        Fields::Unit => {
            return format!("export type {name} = Record<string, never>;\n");
        }
        Fields::Unnamed(_) => {
            panic!("#[ty] does not support tuple structs (`{name}`)")
        }
    };

    let rename_all = serde_rename_all(&s.attrs);
    let docs = extract_docs(&s.attrs);

    let mut field_lines: Vec<String> = Vec::new();
    for f in &fields.named {
        if has_serde_flatten(&f.attrs) {
            panic!(
                "#[ty] does not support `#[serde(flatten)]` (field `{name}::{}`)",
                f.ident.as_ref().unwrap()
            );
        }
        if has_serde_skip(&f.attrs) {
            continue;
        }
        let ident = f.ident.as_ref().unwrap().to_string();
        let wire_name = serde_field_rename(&f.attrs)
            .unwrap_or_else(|| apply_rename_all(&ident, rename_all.as_deref()));
        let ty = rust_type_to_ts(&f.ty, imports);
        let field_docs = extract_docs(&f.attrs);
        if field_docs.is_empty() {
            field_lines.push(format!("{wire_name}: {ty}"));
        } else {
            // Multi-line jsdoc for a field. ts-rs emits this exact
            // shape; we mirror it so the rendered output looks
            // familiar.
            let joined = field_docs.join("\n");
            field_lines.push(format!(
                "\n/**\n * {}\n */\n{wire_name}: {ty}",
                joined.replace('\n', "\n * ")
            ));
        }
    }

    let mut out = String::new();
    write_type_docs(&mut out, &docs);
    out.push_str(&format!("export type {name} = {{ "));
    out.push_str(&field_lines.join(", "));
    out.push_str(", };\n");
    out
}

fn emit_enum(e: &ItemEnum) -> String {
    let name = e.ident.to_string();
    if !e.generics.params.is_empty() {
        panic!("#[ty] does not support generic types (enum `{name}` has type/lifetime params)");
    }
    for v in &e.variants {
        if !matches!(v.fields, Fields::Unit) {
            panic!(
                "#[ty] only supports unit-only enums; `{name}::{}` carries a payload. \
                 Tagged-payload enums (`#[serde(tag = \"...\")]`) aren't supported yet.",
                v.ident
            );
        }
    }
    let rename_all = serde_rename_all(&e.attrs);
    let docs = extract_docs(&e.attrs);

    let mut out = String::new();
    write_type_docs(&mut out, &docs);
    let variants: Vec<String> = e
        .variants
        .iter()
        .map(|v| {
            let ident = v.ident.to_string();
            let renamed = serde_field_rename(&v.attrs)
                .unwrap_or_else(|| apply_rename_all(&ident, rename_all.as_deref()));
            format!("\"{renamed}\"")
        })
        .collect();
    out.push_str(&format!("export type {name} = {};\n", variants.join(" | ")));
    out
}

fn write_type_docs(out: &mut String, docs: &[String]) {
    if docs.is_empty() {
        return;
    }
    if docs.len() == 1 {
        out.push_str(&format!("/** {} */\n", docs[0]));
        return;
    }
    out.push_str("/**\n");
    for line in docs {
        out.push_str(&format!(" * {line}\n"));
    }
    out.push_str(" */\n");
}

// ── serde-attribute helpers ────────────────────────────────────────────

fn serde_rename_all(attrs: &[Attribute]) -> Option<String> {
    serde_string_value(attrs, "rename_all")
}

fn serde_field_rename(attrs: &[Attribute]) -> Option<String> {
    serde_string_value(attrs, "rename")
}

fn serde_string_value(attrs: &[Attribute], key: &str) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let parser = syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated;
        let Ok(metas) = syn::parse::Parser::parse2(parser, list.tokens.clone()) else {
            continue;
        };
        for meta in metas {
            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident(key) {
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

fn has_serde_flatten(attrs: &[Attribute]) -> bool {
    serde_flag(attrs, "flatten")
}

fn has_serde_skip(attrs: &[Attribute]) -> bool {
    serde_flag(attrs, "skip")
}

fn serde_flag(attrs: &[Attribute], key: &str) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let parser = syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated;
        let Ok(metas) = syn::parse::Parser::parse2(parser, list.tokens.clone()) else {
            continue;
        };
        for meta in metas {
            if let Meta::Path(p) = meta {
                if p.is_ident(key) {
                    return true;
                }
            }
        }
    }
    false
}

/// Apply serde's `rename_all` rule to a Rust identifier. Variants stay
/// strict — anything unknown panics so we don't silently mis-spell
/// wire keys.
fn apply_rename_all(ident: &str, rule: Option<&str>) -> String {
    let Some(rule) = rule else {
        return ident.to_string();
    };
    match rule {
        "lowercase" => ident.to_lowercase(),
        "UPPERCASE" => ident.to_uppercase(),
        "PascalCase" => to_pascal(ident),
        "camelCase" => to_camel(ident),
        "snake_case" => to_snake(ident),
        "SCREAMING_SNAKE_CASE" => to_snake(ident).to_uppercase(),
        "kebab-case" => to_snake(ident).replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => to_snake(ident).replace('_', "-").to_uppercase(),
        other => panic!("#[ty] unsupported `#[serde(rename_all = \"{other}\")]`"),
    }
}

fn to_pascal(s: &str) -> String {
    // snake_case → PascalCase. Rust convention for variants is already
    // PascalCase so this just leaves them untouched in practice.
    let mut out = String::new();
    let mut upper = true;
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

fn to_camel(s: &str) -> String {
    let pascal = to_pascal(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}
