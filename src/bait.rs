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
    /// Primary axis: one of `catch_speed | rare_chance | valu_mult | xp_mult`.
    /// Wired through the existing single-effect bait path. Kept as-is so old
    /// `assets/bait.json` entries load unchanged.
    pub effect: String,
    pub magnitude: f32,
    /// Fraction of cast wait-time to subtract (0.0..0.6). Applied multiplicatively
    /// to the geometric wait roll.
    #[serde(default)]
    pub bite_speed: f32,
    /// Free-form fish-pool tag this bait attracts (matches a tag added to
    /// fish entries in commit 8). Empty string = no pool pull.
    #[serde(default)]
    pub pool_pull: String,
    /// Multiplier applied to the rarity weight of fish tagged with `pool_pull`.
    /// 1.5 mild, 3.0 strong, 5.0 extreme. 0 = no pull.
    #[serde(default)]
    pub pool_pull_mult: f32,
}

static DEFS: OnceLock<Vec<BaitDef>> = OnceLock::new();

/// Combined bait list: shop entries from `assets/bait.json` followed by
/// synthesized entries for every bug (id `bug:<bug-id>`, cost = 0). Bug
/// entries are appended after the shop list so the shop UI can still
/// iterate `defs()` and skip cost==0 rows for the "buy" action.
pub fn defs() -> &'static [BaitDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(BAIT_JSON)
            .expect("assets/bait.json failed to parse");
        let mut out: Vec<BaitDef> = raw
            .into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("bait entry malformed"))
            .collect();
        for bug in crate::bugs::defs() {
            // Map bug.generic_effect onto BaitDef.effect/magnitude. If the
            // bug's generic axis IS bite_speed we lift it into the new
            // bite_speed field instead so the wait-time path picks it up.
            let mut bite_speed = 0.0;
            let (effect, magnitude) = if bug.generic_effect == "bite_speed" {
                bite_speed = bug.generic_magnitude;
                ("pool_pull".to_string(), 0.0)
            } else if bug.generic_effect.is_empty() {
                ("pool_pull".to_string(), 0.0)
            } else {
                (bug.generic_effect.clone(), bug.generic_magnitude)
            };
            out.push(BaitDef {
                id: format!("bug:{}", bug.id),
                name: bug.name.clone(),
                description: String::new(),
                cost: 0,
                effect,
                magnitude,
                bite_speed,
                pool_pull: bug.pool_pull.clone(),
                pool_pull_mult: bug.magnitude,
            });
        }
        out
    })
}

pub fn def_by_id(id: &str) -> Option<&'static BaitDef> {
    defs().iter().find(|d| d.id == id)
}

/// True if this bait was synthesized from a bug (id begins with `bug:`).
/// The shop UI uses this to suppress the "buy" action for natural bait.
pub fn is_bug_bait(id: &str) -> bool {
    id.starts_with("bug:")
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
