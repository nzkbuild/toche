use std::fs;
use std::path::Path;

/// Atomically write content to path: temp file + rename
pub fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// JSONC-tolerant settings.json read: strip // comments, then parse
pub fn read_jsonc(path: &Path) -> anyhow::Result<serde_json::Value> {
    let raw = fs::read_to_string(path)?;
    let cleaned: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(serde_json::from_str(&cleaned)?)
}
