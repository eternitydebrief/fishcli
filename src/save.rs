use crate::item::Item;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct SaveData {
    pub name: String,
    pub player_x: i32,
    pub player_y: i32,
    pub valu: u64,
    pub inventory: Vec<String>,
    pub caught: Vec<bool>,
    pub world_seed: u32,
    pub rng_state: u32,
    pub play_time_secs: u64,
    pub lifetime_valu_earned: u64,
    #[serde(default)]
    pub quest_progress: Vec<(String, u32)>,
    #[serde(default)]
    pub quest_done: Vec<String>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub pinned_quest: Option<String>,
    #[serde(default)]
    pub seen_cells: Vec<(i32, i32)>,
}

fn save_path() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("fishcli");
    Some(dir.join("save.json"))
}

pub fn save_to_disk(data: &SaveData) -> Result<()> {
    let path = save_path().ok_or_else(|| anyhow!("could not resolve data dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_from_disk() -> Option<SaveData> {
    let path = save_path()?;
    let json = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}
