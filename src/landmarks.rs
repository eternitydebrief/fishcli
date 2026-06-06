#![allow(dead_code)]
//! Landmark capes — flat-list progression beats that fire once when their
//! criteria are first met. Each unlock grants small permanent additive
//! bonuses (xp/valu/rare) that accumulate across the run.
//!
//! Designed for the "60 landmarks across 3000h" pacing target: each cape
//! is a small dopamine beat. The seed in `assets/landmarks.json` covers
//! ~15 to prove the engine; add more entries to extend.

use serde::Deserialize;
use std::sync::OnceLock;

const LANDMARKS_JSON: &str = include_str!("../assets/landmarks.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Criteria {
    pub kind: String,
    pub target: i64,
    #[serde(default)]
    pub target_str: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Reward {
    #[serde(default)]
    pub xp_mult: f32,
    #[serde(default)]
    pub valu_mult: f32,
    #[serde(default)]
    pub rare_chance: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Landmark {
    pub id: String,
    pub name: String,
    pub criteria: Criteria,
    #[serde(default)]
    pub reward: Reward,
}

static CACHE: OnceLock<Vec<Landmark>> = OnceLock::new();

pub fn landmarks() -> &'static [Landmark] {
    CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(LANDMARKS_JSON)
            .expect("assets/landmarks.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("landmark malformed"))
            .collect()
    })
}

/// Snapshot of player progress passed to the per-tick check.
pub struct Snapshot {
    pub catches: u64,
    pub fishdex_pct: u8,
    pub rod_tier: u32,
    pub bugs_caught: u64,
    pub play_hours: u64,
    pub visited_dim_labels: Vec<&'static str>,
}

pub fn criteria_met(c: &Criteria, snap: &Snapshot) -> bool {
    match c.kind.as_str() {
        "catches" => snap.catches >= c.target as u64,
        "fishdex_pct" => (snap.fishdex_pct as i64) >= c.target,
        "rod_tier" => snap.rod_tier >= c.target as u32,
        "bugs_caught" => snap.bugs_caught >= c.target as u64,
        "play_hours" => snap.play_hours >= c.target as u64,
        "visited_dim" => snap.visited_dim_labels.iter().any(|d| d.eq_ignore_ascii_case(&c.target_str)),
        _ => false,
    }
}

pub fn def_by_id(id: &str) -> Option<&'static Landmark> {
    landmarks().iter().find(|l| l.id == id)
}
