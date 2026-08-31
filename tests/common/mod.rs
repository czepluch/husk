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

/// A directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("husk-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A vdir with three projects: `life` (Apple and todoman fixtures, minus the
/// duplicate UID, with name and color files), `argot` (Tasks.org fixtures, name only) and `solidity`
/// (empty, no metadata files). Fixture file names are kept, so most files
/// are not named after their UID.
pub fn fixture_vdir() -> TempDir {
    let dir = TempDir::new();
    for (project, name, color) in [
        ("life", Some("Life"), Some("#83D754")),
        ("argot", Some("Argot"), None),
        ("solidity", None, None),
    ] {
        let path = dir.path().join(project);
        fs::create_dir(&path).unwrap();
        if let Some(name) = name {
            fs::write(path.join("displayname"), format!("{name}\n")).unwrap();
        }
        if let Some(color) = color {
            fs::write(path.join("color"), format!("{color}\n")).unwrap();
        }
    }
    // completed.ics is the same task as no-due.ics after completion, so the
    // two share a UID. A vdir never holds two files with one UID.
    for (rel, text) in fixtures() {
        if rel.ends_with("completed.ics") {
            continue;
        }
        let project = if rel.starts_with("tasksorg") {
            "argot"
        } else {
            "life"
        };
        fs::write(
            dir.path().join(project).join(rel.file_name().unwrap()),
            text,
        )
        .unwrap();
    }
    dir
}

/// How many tasks `fixture_vdir` puts into `life` and `argot`.
pub fn vdir_task_counts() -> (usize, usize) {
    let all = fixtures();
    let argot = all
        .iter()
        .filter(|(rel, _)| rel.starts_with("tasksorg"))
        .count();
    let life = all.len() - argot - 1;
    (life, argot)
}
