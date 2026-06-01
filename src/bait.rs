#![allow(dead_code)]
//! Bait: per-cast consumable buffs. Definitions live in `assets/bait.json`;
//! the player stocks them via a Bait Vendor and equips one as `active`.
//! On a successful catch, one of the active bait is consumed and its
//! magnitude is applied to that cast's outcome.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const BAIT_JSON: &str = include_str!("../assets/bait.json");

#[derive(Clone, Debug, Deserialize)]
pub struct BaitDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cost: u64,
    pub effect: String,
    pub magnitude: f32,
}

static DEFS: OnceLock<Vec<BaitDef>> = OnceLock::new();

pub fn defs() -> &'static [BaitDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(BAIT_JSON)
            .expect("assets/bait.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("bait entry malformed"))
            .collect()
    })
}

pub fn def_by_id(id: &str) -> Option<&'static BaitDef> {
    defs().iter().find(|d| d.id == id)
}

impl BaitDef {
    /// Rod tier required to buy this bait. Derived from cost so the
    /// expensive baits (which have big effects) gate naturally with
    /// progression.
    pub fn min_rod_tier(&self) -> u32 {
        match self.cost {
            0..=49 => 1,
            50..=199 => 10,
            200..=499 => 30,
            500..=1499 => 60,
            1500..=4999 => 100,
            _ => 150,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BaitStock {
    /// bait_id -> remaining count
    #[serde(default)]
    pub stock: BTreeMap<String, u32>,
    /// Currently-active bait id (Some when equipped). Consumed one per
    /// catch.
    #[serde(default)]
    pub active: Option<String>,
}

impl BaitStock {
    pub fn count(&self, id: &str) -> u32 {
        self.stock.get(id).copied().unwrap_or(0)
    }
    pub fn add(&mut self, id: &str, n: u32) {
        let entry = self.stock.entry(id.to_string()).or_insert(0);
        *entry = entry.saturating_add(n);
    }
    /// Consume one of the active bait. Returns the consumed def if any.
    pub fn consume_active(&mut self) -> Option<&'static BaitDef> {
        let id = self.active.clone()?;
        if self.count(&id) == 0 {
            self.active = None;
            return None;
        }
        let entry = self.stock.entry(id.clone()).or_insert(0);
        if *entry > 0 {
            *entry -= 1;
        }
        if *entry == 0 {
            self.active = None;
        }
        def_by_id(&id)
    }
}
