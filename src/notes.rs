use anyhow::{anyhow, Result};
use std::path::PathBuf;

fn notes_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("fishcli").join("notes.txt"))
}

pub fn load() -> String {
    notes_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

pub fn save(text: &str) -> Result<()> {
    let path = notes_path().ok_or_else(|| anyhow!("could not resolve data dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}
