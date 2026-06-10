// Per integration test file, Cargo compiles `common` separately; not
// every test uses every helper, so dead-code lints can fire.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

pub fn fresh_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("draad-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

pub fn run(root: &PathBuf) -> String {
    draad_codegen::Config::new()
        .root(root)
        .rust_only()
        .generate()
        .unwrap();
    fs::read_to_string(root.join("src/_generated.rs")).unwrap()
}
