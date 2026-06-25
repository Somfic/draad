//! Public builder for the codegen pipeline. The actual scan → parse →
//! emit work lives in [`super::pipeline`].

use std::path::PathBuf;

/// Builder-style configuration for [`super::pipeline::generate`].
#[derive(Clone, Debug)]
pub struct Config {
    /// Project root. Every path field below is resolved against this.
    /// Default: current working directory.
    pub root: PathBuf,
    /// Directory to scan (recursive). Default: `src`.
    pub src_dir: PathBuf,
    /// Output path for the generated Rust file. Default: `src/_generated.rs`.
    pub generated_rs: PathBuf,
    /// Frontend output directory for the unified `index.ts`.
    /// Default: `frontend/src/lib/schema`.
    pub client_dir: PathBuf,

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

    /// Hand-written sidecar module for type names draad can't resolve.
    /// `Some("custom")` makes the generated `index.ts` start with
    /// `import * as custom from "./custom";` and rewrites unknown type
    /// references in API signatures, event payloads, and `#[ty]` fields
    /// from a bare `Foo` to `custom.Foo`.
    ///
    /// Default: `None`.
    pub custom_ts: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            src_dir: PathBuf::from("src"),
            generated_rs: PathBuf::from("src/_generated.rs"),
            client_dir: PathBuf::from("frontend/src/lib/schema"),
            api_modules_prefix: "crate::api".into(),
            skip_files: vec!["lib".into(), "main".into(), "mod".into()],
            rust_only: false,
            custom_ts: None,
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
    /// Enable the custom-types sidecar module. `name` is used as both the
    /// imported file stem (`./<name>.ts`) and the import alias, so unknown
    /// types render as `<name>.TypeName`.
    pub fn custom_ts(mut self, name: impl Into<String>) -> Self {
        self.custom_ts = Some(name.into());
        self
    }
    /// Run the codegen with this configuration.
    pub fn generate(self) -> std::io::Result<()> {
        super::pipeline::generate(&self)
    }
}
