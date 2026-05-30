//! Public API: the [`Config`] builder and the top-level [`generate`]
//! orchestrator that drives scan → parse → emit.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use syn::Item;

use super::emit_rust::write_generated_rs;
use super::emit_ts::write_index;
use super::model::{Api, EventApi};
use super::parse::{parse_events_trait, parse_trait};
use super::scan::{collect_rs_files, extract_attr_namespace, has_attr};

/// Builder-style configuration for [`generate`].
#[derive(Clone, Debug)]
pub struct Config {
    /// Project root. Every path field below is resolved against this.
    /// Default: current working directory.
    pub root: PathBuf,
    /// Directory to scan (recursive). Default: `src`.
    pub src_dir: PathBuf,
    /// Output path for the generated Rust file. Default: `src/_generated.rs`.
    pub generated_rs: PathBuf,
    /// Frontend output directory for per-namespace `.ts` files.
    /// Default: `frontend/src/lib/schema`.
    pub client_dir: PathBuf,
    /// Where ts-rs writes per-type bindings. Defaults to the
    /// `TS_RS_EXPORT_DIR` env var if set, otherwise `target/draad-bindings`.
    pub per_type_dir: Option<PathBuf>,

    /// `use` statement (without the `use` keyword or trailing `;`) for the
    /// RPC response wrapper. The generated handlers call `ok(...)` and
    /// return `Response<T>`. Default points at the shim shipped under the
    /// `runtime` feature; override to plug in a custom wrapper.
    /// Default: `draad::runtime::{Response, ok}`.
    pub rpc_runtime_use: String,
    /// Module path prefix for discovered API modules. The codegen emits
    /// one `use {prefix}::{module}::*;` per scanned `.rs` file that holds
    /// `#[api]` / `#[events]` / `#[ty]` items. Default: `crate::api`.
    pub api_modules_prefix: String,

    /// File stems excluded from scanning. The generated file itself is
    /// always skipped. Default: `["lib", "main", "mod"]`.
    pub skip_files: Vec<String>,

    /// Skip the TypeScript pass entirely. Useful for `build.rs` where we
    /// only need `_generated.rs` to be fresh so the backend compiles.
    /// Default: false.
    pub rust_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            src_dir: PathBuf::from("src"),
            generated_rs: PathBuf::from("src/_generated.rs"),
            client_dir: PathBuf::from("frontend/src/lib/schema"),
            per_type_dir: None,
            rpc_runtime_use: "draad::runtime::{Response, ok}".into(),
            api_modules_prefix: "crate::api".into(),
            skip_files: vec!["lib".into(), "main".into(), "mod".into()],
            rust_only: false,
        }
    }
}

impl Config {
    /// Fresh config with defaults.
    pub fn new() -> Self {
        Self::default()
    }
    pub fn root(mut self, p: impl Into<PathBuf>) -> Self {
        self.root = p.into();
        self
    }
    pub fn src_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.src_dir = p.into();
        self
    }
    pub fn generated_rs(mut self, p: impl Into<PathBuf>) -> Self {
        self.generated_rs = p.into();
        self
    }
    pub fn client_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.client_dir = p.into();
        self
    }
    pub fn per_type_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.per_type_dir = Some(p.into());
        self
    }
    pub fn rpc_runtime(mut self, s: impl Into<String>) -> Self {
        self.rpc_runtime_use = s.into();
        self
    }
    pub fn api_modules_prefix(mut self, s: impl Into<String>) -> Self {
        self.api_modules_prefix = s.into();
        self
    }
    pub fn skip_file(mut self, stem: impl Into<String>) -> Self {
        self.skip_files.push(stem.into());
        self
    }
    pub fn rust_only(mut self) -> Self {
        self.rust_only = true;
        self
    }
    /// Run the codegen with this configuration.
    pub fn generate(self) -> std::io::Result<()> {
        generate(&self)
    }
}

/// Run the codegen. Most callers will use [`Config::generate`] instead.
pub fn generate(cfg: &Config) -> std::io::Result<()> {
    let src_dir = cfg.root.join(&cfg.src_dir);
    let bindings_dir = cfg
        .per_type_dir
        .clone()
        .or_else(|| std::env::var_os("TS_RS_EXPORT_DIR").map(PathBuf::from))
        .unwrap_or_else(|| cfg.root.join("target/draad-bindings"));
    let per_type_dir = bindings_dir.join("_per_type");
    let client_dir = cfg.root.join(&cfg.client_dir);
    let generated_rs = cfg.root.join(&cfg.generated_rs);

    let generated_stem = cfg
        .generated_rs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("_generated")
        .to_string();

    let mut type_to_module: BTreeMap<String, String> = BTreeMap::new();
    let mut module_types: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut apis: Vec<Api> = Vec::new();
    let mut event_apis: Vec<EventApi> = Vec::new();
    let mut imports: BTreeSet<String> = BTreeSet::new();

    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&src_dir, &mut rs_files);
    rs_files.sort();
    for path in &rs_files {
        let module = path.file_stem().unwrap().to_string_lossy().into_owned();
        if module == generated_stem || cfg.skip_files.iter().any(|s| s == &module) {
            continue;
        }
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let Ok(file) = syn::parse_file(&src) else {
            continue;
        };
        for item in &file.items {
            match item {
                Item::Struct(s) if has_attr(&s.attrs, "ty") => {
                    let name = s.ident.to_string();
                    type_to_module.insert(name.clone(), module.clone());
                    module_types.entry(module.clone()).or_default().push(name);
                }
                Item::Enum(e) if has_attr(&e.attrs, "wire") => {
                    let name = e.ident.to_string();
                    type_to_module.insert(name.clone(), module.clone());
                    module_types.entry(module.clone()).or_default().push(name);
                }
                Item::Trait(t) => {
                    if let Some(namespace) = extract_attr_namespace(t, "api") {
                        apis.push(parse_trait(t, namespace, module.clone(), &mut imports));
                    } else if let Some(namespace) = extract_attr_namespace(t, "events") {
                        event_apis.push(parse_events_trait(
                            t,
                            namespace,
                            module.clone(),
                            &mut imports,
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    apis.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    event_apis.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    write_generated_rs(cfg, &generated_rs, &apis, &event_apis)?;

    if cfg.rust_only {
        return Ok(());
    }

    let mut types_in_order: Vec<String> = Vec::new();
    for types in module_types.values() {
        for t in types {
            if !types_in_order.contains(t) {
                types_in_order.push(t.clone());
            }
        }
    }
    write_index(
        &client_dir.join("index.ts"),
        &types_in_order,
        &per_type_dir,
        &apis,
        &event_apis,
    )?;
    Ok(())
}
