#![allow(dead_code)]
//! Tackle: hat / vest / line / lure equipment slots. Each slot has 8 tiers
//! sold by the rod-shop's tackle tab. Definitions live in
//! `assets/tackle.json`; effects stack additively with skill-tree bonuses.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const TACKLE_JSON: &str = include_str!("../assets/tackle.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    Hat,
    Vest,
    Line,
    Lure,
}

impl Slot {
    pub const ALL: &'static [Slot] = &[Slot::Hat, Slot::Vest, Slot::Line, Slot::Lure];
    pub fn label(self) -> &'static str {
        match self {
            Slot::Hat => "Hat",
            Slot::Vest => "Vest",
            Slot::Line => "Line",
            Slot::Lure => "Lure",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TackleDef {
    pub id: String,
    pub slot: Slot,
    pub tier: u32,
    pub name: String,
    pub description: String,
    pub cost: u64,
    pub effect: String,
    pub magnitude: f32,
}

static DEFS: OnceLock<Vec<TackleDef>> = OnceLock::new();

pub fn defs() -> &'static [TackleDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(TACKLE_JSON)
            .expect("assets/tackle.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("tackle entry malformed"))
            .collect()
    })
}

pub fn defs_for_slot(slot: Slot) -> Vec<&'static TackleDef> {
    let mut v: Vec<&'static TackleDef> = defs().iter().filter(|d| d.slot == slot).collect();
    v.sort_by_key(|d| d.tier);
    v
}

impl TackleDef {
    /// Rod tier required to buy this tackle. Scales with tier so the
    /// player can't farm a tier-3 rod to skip into endgame tackle.
    /// Tier 1=rod1, 2=rod15, 3=rod30, 4=rod50, 5=rod75, 6=rod100, 7=rod130, 8=rod160.
    pub fn min_rod_tier(&self) -> u32 {
        match self.tier {
            0 | 1 => 1,
            2 => 15,
            3 => 30,
            4 => 50,
            5 => 75,
            6 => 100,
            7 => 130,
            _ => 160,
        }
    }
}

pub fn def_by_id(id: &str) -> Option<&'static TackleDef> {
    defs().iter().find(|d| d.id == id)
}

/// Player's currently-equipped tackle, keyed by slot. Tier 0 = nothing.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct EquippedTackle {
    #[serde(default)]
    pub hat: u32,
    #[serde(default)]
    pub vest: u32,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub lure: u32,
}

impl EquippedTackle {
    pub fn tier(&self, slot: Slot) -> u32 {
        match slot {
            Slot::Hat => self.hat,
            Slot::Vest => self.vest,
            Slot::Line => self.line,
            Slot::Lure => self.lure,
        }
    }
    pub fn set_tier(&mut self, slot: Slot, tier: u32) {
        match slot {
            Slot::Hat => self.hat = tier,
            Slot::Vest => self.vest = tier,
            Slot::Line => self.line = tier,
            Slot::Lure => self.lure = tier,
        }
    }
    pub fn equipped(&self, slot: Slot) -> Option<&'static TackleDef> {
        let t = self.tier(slot);
        if t == 0 { return None; }
        defs_for_slot(slot).into_iter().find(|d| d.tier == t)
    }

    /// Sum of all equipped tackle's magnitudes matching `effect`.
    pub fn sum_effect(&self, effect: &str) -> f32 {
        let mut acc = 0.0f32;
        for slot in Slot::ALL {
            if let Some(d) = self.equipped(*slot) {
                if d.effect == effect {
                    acc += d.magnitude;
                }
            }
        }
        acc
    }
}
