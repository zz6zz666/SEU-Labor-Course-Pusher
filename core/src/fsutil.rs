//! Small filesystem helpers shared by the persistent stores.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` atomically: a sibling temp file is written and
/// flushed to disk, then renamed over the target, so a crash never leaves a
/// half-written file behind.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Writes pretty JSON plus a trailing newline, atomically.
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let mut raw = serde_json::to_string_pretty(value)?;
    raw.push('\n');
    atomic_write(path, raw.as_bytes())?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}
