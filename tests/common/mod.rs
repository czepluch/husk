#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture as (path relative to tests/fixtures, contents), sorted by path.
pub fn fixtures() -> Vec<(PathBuf, String)> {
    let root = fixture_root();
    let mut out = Vec::new();
    collect(&root, &root, &mut out);
    out.sort();
    assert!(!out.is_empty(), "no fixtures under {}", root.display());
    out
}

/// One fixture by its path relative to tests/fixtures.
pub fn fixture(name: &str) -> String {
    let path = fixture_root().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).expect("fixture directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "ics") {
            let text = fs::read_to_string(&path).expect("fixture is UTF-8");
            out.push((path.strip_prefix(root).unwrap().to_path_buf(), text));
        }
    }
}
