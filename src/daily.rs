#![allow(dead_code)]
//! Daily quest: one rotating challenge per UTC date. Definition list lives
//! in `assets/daily_quests.json`. The "today" pick is deterministic from the
//! UTC date string, so anyone playing on the same day sees the same quest.

use serde::Deserialize;
use std::sync::OnceLock;

const DAILY_JSON: &str = include_str!("../assets/daily_quests.json");

#[derive(Clone, Debug, Deserialize)]
pub struct DailyDef {
    pub id: String,
    pub title: String,
    pub description: String,
    /// 'catch' | 'walk' | 'talk'
    pub kind: String,
    /// fish name / biome name / npc id / "any"
    pub target: String,
    pub count: u32,
    #[serde(default)]
    pub reward_valu: u64,
    #[serde(default)]
    pub reward_points: u32,
}

static DEFS: OnceLock<Vec<DailyDef>> = OnceLock::new();

pub fn defs() -> &'static [DailyDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(DAILY_JSON)
            .expect("assets/daily_quests.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("daily quest entry malformed"))
            .collect()
    })
}

/// UTC date string like "2026-06-01". Used both as the day-id and as the
/// hash input for picking today's quest.
pub fn today_id() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn hash_str(s: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Deterministic daily pick. Returns None only if the JSON is empty.
pub fn today_def() -> Option<&'static DailyDef> {
    let list = defs();
    if list.is_empty() {
        return None;
    }
    let h = hash_str(&today_id());
    Some(&list[(h as usize) % list.len()])
}
