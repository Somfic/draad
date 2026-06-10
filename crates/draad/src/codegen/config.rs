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
        super::pipeline::generate(&self)
    }
}
