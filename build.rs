//! Build script — re-runs the build whenever content files change.
//!
//! Without this, `include_dir!` embeds `content/core` at compile
//! time but cargo only watches Rust source files for changes. Edits
//! to TOML / ASCII-art / substance / brand files would sit in the
//! source tree while the embedded binary still carries the old data.
//! This script tells cargo to invalidate the build when anything
//! under `content/` changes.

use std::fs;
use std::path::Path;

fn main() {
    track_tree("content");
}

fn track_tree(root: &str) {
    println!("cargo:rerun-if-changed={root}");
    let path = Path::new(root);
    if !path.exists() {
        return;
    }
    walk(path);
}

fn walk(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if let Some(s) = p.to_str() {
            println!("cargo:rerun-if-changed={s}");
        }
        if p.is_dir() {
            walk(&p);
        }
    }
}
