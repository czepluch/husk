//! Atomic file writes: a temp file in the same directory, fsynced, then
//! renamed over the target, so a reader (vdirsyncer, the next notify run)
//! sees the old file or the new one and never a partial one.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    // A bare file name (`--state notify.json`) has an empty parent.
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("path without a file name")?;
    let tmp = dir.join(format!("{name}.tmp"));
    let written = (|| -> Result<()> {
        let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
            .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    // The rename itself has to reach disk before the write counts as done.
    File::open(dir)?.sync_all()?;
    Ok(())
}
