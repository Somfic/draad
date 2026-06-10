//! Top-level `scan → parse → emit` driver. [`Config`](super::Config)
//! describes *what* to do; this module is *how*.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use syn::Item;

use super::config::Config;
use super::emit_rust::write_generated_rs;
use super::emit_ts::write_index;
use super::model::{Api, EventApi};
use super::parse::{parse_events_trait, parse_trait};
use super::scan::{collect_rs_files, extract_attr_namespace, has_attr};

/// Run the codegen with `cfg`. Most callers use
/// [`Config::generate`](super::Config::generate) instead.
pub fn generate(cfg: &Config) -> std::io::Result<()> {
    let layout = ResolvedPaths::from_config(cfg);
    emit_input_directives(&layout);

    let scan = scan_workspace(cfg, &layout)?;

    write_generated_rs(cfg, &layout.generated_rs, &scan.apis, &scan.event_apis)?;
    emit_output_directive(&layout.generated_rs);

    if cfg.rust_only {
        return Ok(());
    }

    let types_in_order = scan.types_in_order();
    write_index(
        &layout.client_dir.join("index.ts"),
        &types_in_order,
        &layout.per_type_dir,
        &scan.apis,
        &scan.event_apis,
    )?;
    emit_output_directive(&layout.client_dir.join("index.ts"));

    // Only watch ts-rs's per-type bindings if we actually consumed any
    // on this run. Watching a path that doesn't exist would put cargo
    // in a permanent "modified" state and re-run the build script on
    // every invocation.
    if !types_in_order.is_empty() {
        emit_output_directive(&layout.per_type_dir);
    }
    Ok(())
}

/// Watch the inputs so cargo re-runs the build script when sources or
/// ts-rs's per-type bindings change. We always print, even outside a
/// build-script context — the directives are harmless stdout noise
/// then, and gating on env vars would silently mis-fire if cargo ever
/// changed how it exposes the script environment.
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
    per_type_dir: PathBuf,
    /// Stem of [`Self::generated_rs`]; the scanner skips it so the file
    /// can't pick up its own previous output.
    generated_stem: String,
}

impl ResolvedPaths {
    fn from_config(cfg: &Config) -> Self {
        let src_dir = cfg.root.join(&cfg.src_dir);
        // Match the absolute path the `#[ty]` macro hardcodes into
        // `#[ts(export_to = ...)]` from `CARGO_MANIFEST_DIR`. Tests
        // achieve isolation by pointing `cfg.root` at a fresh tmp dir.
        let per_type_dir = cfg.root.join("target/draad-bindings/_per_type");
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
            per_type_dir,
            generated_stem,
        }
    }
}

/// Result of one scan pass: the apis/events worth emitting, plus the
/// per-module list of `#[ty]` types so `index.ts` can inline them in a
/// stable order.
struct ScanResult {
    apis: Vec<Api>,
    event_apis: Vec<EventApi>,
    module_types: BTreeMap<String, Vec<String>>,
}

impl ScanResult {
    /// Flatten the per-module type lists into a single dedup'd
    /// inline-order list for the TS emitter. Module iteration order is
    /// `BTreeMap`-sorted (alphabetical by file stem) for determinism.
    fn types_in_order(&self) -> Vec<String> {
        let mut seen: BTreeSet<&String> = BTreeSet::new();
        let mut out = Vec::new();
        for types in self.module_types.values() {
            for t in types {
                if seen.insert(t) {
                    out.push(t.clone());
                }
            }
        }
        out
    }
}

fn scan_workspace(cfg: &Config, layout: &ResolvedPaths) -> std::io::Result<ScanResult> {
    let mut module_types: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut apis: Vec<Api> = Vec::new();
    let mut event_apis: Vec<EventApi> = Vec::new();
    let mut imports: BTreeSet<String> = BTreeSet::new();

    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&layout.src_dir, &mut rs_files);
    rs_files.sort();
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
                    let name = s.ident.to_string();
                    module_types.entry(module.clone()).or_default().push(name);
                }
                Item::Enum(e) if has_attr(&e.attrs, "ty") => {
                    let name = e.ident.to_string();
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
    Ok(ScanResult {
        apis,
        event_apis,
        module_types,
    })
}
