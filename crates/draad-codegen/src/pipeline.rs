//! Top-level `scan → parse → emit` driver. [`Config`](super::Config)
//! describes *what* to do; this module is *how*.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use syn::Item;

use super::config::Config;
use super::emit_rust::{render_generated_rs, write_generated_rs};
use super::emit_ts::{render_index, write_index};
use super::model::{Api, EventApi};
use super::parse::{parse_events_trait, parse_trait};
use super::scan::{collect_rs_files, extract_attr_namespace, has_attr};

/// Rendered codegen output without any disk side effects, plus the list
/// of source files the scanner read. The proc-macro driver wants both
/// pieces — the rendered Rust to splice into a `TokenStream`, and the
/// file list to emit `include_bytes!` invalidation guards.
pub struct Artifacts {
    /// Body of the would-be `_generated.rs` (handlers, router, event
    /// emitters, the `use` lines). The proc-macro splices this into a
    /// `mod __draad_generated` block.
    pub rust_source: String,
    /// Full TypeScript content for `index.ts`. `None` when `rust_only`
    /// is set.
    pub ts_source: Option<String>,
    /// Default location for `index.ts` (against `cfg.root`). Callers
    /// can ignore this and write somewhere else.
    pub ts_path: PathBuf,
    /// Absolute paths of every `.rs` file under `src_dir` the scanner
    /// visited — including ones it skipped via `cfg.skip_files`. The
    /// proc-macro emits an `include_bytes!` guard per file so rustc
    /// invalidates the cached macro expansion on any source change.
    pub files_read: Vec<PathBuf>,
}

/// Render the codegen output as in-memory strings without touching the
/// filesystem. Used by the `include_generated!` proc-macro driver.
pub fn run(cfg: &Config) -> std::io::Result<Artifacts> {
    let layout = ResolvedPaths::from_config(cfg);
    let scan = scan_workspace(cfg, &layout)?;
    let ty_modules: Vec<String> = scan
        .module_types
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(m, _)| m.clone())
        .collect();
    let rust_source = render_generated_rs(cfg, &scan.apis, &scan.event_apis, &ty_modules);
    let ts_source = if cfg.rust_only {
        None
    } else {
        Some(render_index(
            &scan.ty_items_in_order(),
            &scan.apis,
            &scan.event_apis,
        ))
    };
    Ok(Artifacts {
        rust_source,
        ts_source,
        ts_path: layout.client_dir.join("index.ts"),
        files_read: scan.files_read,
    })
}

/// Run the codegen and write everything to disk. Used from hand-written
/// `build.rs` scripts; the proc-macro path uses [`run`] instead.
pub fn generate(cfg: &Config) -> std::io::Result<()> {
    let layout = ResolvedPaths::from_config(cfg);
    emit_input_directives(&layout);

    let scan = scan_workspace(cfg, &layout)?;

    let ty_modules: Vec<String> = scan
        .module_types
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(m, _)| m.clone())
        .collect();
    write_generated_rs(
        cfg,
        &layout.generated_rs,
        &scan.apis,
        &scan.event_apis,
        &ty_modules,
    )?;
    emit_output_directive(&layout.generated_rs);

    if cfg.rust_only {
        return Ok(());
    }

    write_index(
        &layout.client_dir.join("index.ts"),
        &scan.ty_items_in_order(),
        &scan.apis,
        &scan.event_apis,
    )?;
    emit_output_directive(&layout.client_dir.join("index.ts"));
    Ok(())
}

/// Watch the inputs so cargo re-runs the build script when sources
/// change. We always print, even outside a build-script context — the
/// directives are harmless stdout noise then, and gating on env vars
/// would silently mis-fire if cargo ever changed how it exposes the
/// script environment.
fn emit_input_directives(layout: &ResolvedPaths) {
    println!("cargo:rerun-if-changed={}", layout.src_dir.display());
}

/// Watch a generated path so cargo re-runs the build script when it's
/// deleted out from under us — cargo treats a missing watched path as
/// "modified". Only meaningful for paths we know we just wrote.
fn emit_output_directive(path: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

/// Output paths after `cfg.root` is folded in.
struct ResolvedPaths {
    src_dir: PathBuf,
    generated_rs: PathBuf,
    client_dir: PathBuf,
    /// Stem of [`Self::generated_rs`]; the scanner skips it so the file
    /// can't pick up its own previous output.
    generated_stem: String,
}

impl ResolvedPaths {
    fn from_config(cfg: &Config) -> Self {
        let src_dir = cfg.root.join(&cfg.src_dir);
        let client_dir = cfg.root.join(&cfg.client_dir);
        let generated_rs = cfg.root.join(&cfg.generated_rs);
        let generated_stem = cfg
            .generated_rs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("_generated")
            .to_string();
        Self {
            src_dir,
            generated_rs,
            client_dir,
            generated_stem,
        }
    }
}

/// Result of one scan pass: the apis/events worth emitting, plus the
/// `#[ty]` items per module so `index.ts` can render them in a stable
/// order.
struct ScanResult {
    apis: Vec<Api>,
    event_apis: Vec<EventApi>,
    /// `module_name` → list of `#[ty]` items declared in that module,
    /// in source order. Each item is the full `syn` node so the TS
    /// emitter can render the declaration directly.
    module_types: BTreeMap<String, Vec<Item>>,
    /// Every `.rs` file the scanner enumerated under `src_dir`, even
    /// the ones it ended up skipping. Used for invalidation tracking.
    files_read: Vec<PathBuf>,
}

impl ScanResult {
    /// Flatten the per-module type lists into a single dedup'd
    /// inline-order list for the TS emitter. Module iteration order is
    /// `BTreeMap`-sorted (alphabetical by file stem) for determinism.
    fn ty_items_in_order(&self) -> Vec<&Item> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut out = Vec::new();
        for items in self.module_types.values() {
            for it in items {
                let name = item_ident(it);
                if seen.insert(name) {
                    out.push(it);
                }
            }
        }
        out
    }
}

fn item_ident(item: &Item) -> String {
    match item {
        Item::Struct(s) => s.ident.to_string(),
        Item::Enum(e) => e.ident.to_string(),
        _ => String::new(),
    }
}

fn scan_workspace(cfg: &Config, layout: &ResolvedPaths) -> std::io::Result<ScanResult> {
    let mut module_types: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    let mut apis: Vec<Api> = Vec::new();
    let mut event_apis: Vec<EventApi> = Vec::new();
    let mut imports: BTreeSet<String> = BTreeSet::new();

    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&layout.src_dir, &mut rs_files);
    rs_files.sort();
    let files_read = rs_files.clone();
    for path in &rs_files {
        let module = path.file_stem().unwrap().to_string_lossy().into_owned();
        if module == layout.generated_stem || cfg.skip_files.iter().any(|s| s == &module) {
            continue;
        }
        let Ok(src) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(file) = syn::parse_file(&src) else {
            continue;
        };
        for item in &file.items {
            match item {
                Item::Struct(s) if has_attr(&s.attrs, "ty") => {
                    module_types
                        .entry(module.clone())
                        .or_default()
                        .push(item.clone());
                }
                Item::Enum(e) if has_attr(&e.attrs, "ty") => {
                    module_types
                        .entry(module.clone())
                        .or_default()
                        .push(item.clone());
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
    Ok(ScanResult {
        apis,
        event_apis,
        module_types,
        files_read,
    })
}
