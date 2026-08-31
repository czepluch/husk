//! Source-level checks for rules clippy cannot express fully.

use std::fs;
use std::path::Path;

fn visit(dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source directory") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            visit(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("theme.rs") {
            let text = fs::read_to_string(&path).expect("source file");
            for (i, line) in text.lines().enumerate() {
                if ["Color::", "Stylize", "prelude::*"]
                    .iter()
                    .any(|s| line.contains(s))
                {
                    found.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    }
}

/// Colors and the `Stylize` shortcuts stay inside theme.rs; the glob import
/// of ratatui's prelude would smuggle both past clippy's `disallowed_types`.
#[test]
fn colors_live_only_in_theme_rs() {
    let mut found = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(found.is_empty(), "{}", found.join("\n"));
}
