//! Forgeable equipment: boots (feet), necklaces (neck), rings (ring), and
//! capes (cape). Cape is achievement-only; the other three come out of the
//! blacksmith's forge. Each slot holds an optional gear-id string that
//! resolves to a [`GearDef`] in `assets/gear.json` (loaded on demand).
//!
//! The "tier" field on each def is the forge-progression rank (1..=5 for
//! boots/neck/ring). Higher tier = better perks + more rare ingots in the
//! recipe + higher blacksmithing-level gate.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const GEAR_JSON: &str = include_str!("../assets/gear.json");

/// Equipment slots. Distinct from the tackle slots (Hat/Vest/Line/Lure)
/// because the gear here drives movement/economy/meta perks rather than
/// the fishing minigame itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Feet,
    Neck,
    Ring,
    Cape,
    Pickaxe,
}

impl Slot {
    pub const ALL: [Slot; 5] = [Slot::Feet, Slot::Neck, Slot::Ring, Slot::Cape, Slot::Pickaxe];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Feet => "Feet",
            Slot::Neck => "Neck",
            Slot::Ring => "Ring",
            Slot::Cape => "Cape",
            Slot::Pickaxe => "Pickaxe",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Slot::Feet => "feet",
            Slot::Neck => "neck",
            Slot::Ring => "ring",
            Slot::Cape => "cape",
            Slot::Pickaxe => "pickaxe",
        }
    }
}

/// Per-slot perks. Each field is a multiplier or pct delta that downstream
/// systems read when computing their numbers. Tier-0/None gear contributes
/// nothing — `Default` returns the *neutral* perks (mults = 1.0, chances/
/// counts = 0) so `combined_perks()` can start from the no-op baseline.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Perks {
    /// Multiplier on movement-step cooldown. <1.0 = faster. e.g. 0.7 = 30% faster.
    #[serde(default = "one")]
    pub move_speed_mult: f32,
    /// Multiplier on per-step stamina loss. <1.0 = less drain.
    #[serde(default = "one")]
    pub stamina_loss_mult: f32,
    /// Chance (0..1) for a catch to yield two fish instead of one.
    #[serde(default)]
    pub double_fish_chance: f32,
    /// Chance (0..1) that a cast does not consume bait.
    #[serde(default)]
    pub no_bait_consume_chance: f32,
    /// Number of letters of every ore name pre-filled in the smelt minigame.
    #[serde(default)]
    pub ore_prewrite_letters: u32,
    /// Tier this pickaxe can mine. 0 = not a pickaxe. Ores carry a required
    /// pickaxe tier and gate accordingly.
    #[serde(default)]
    pub pickaxe_tier: u32,
}

impl Default for Perks {
    fn default() -> Self {
        Self {
            move_speed_mult: 1.0,
            stamina_loss_mult: 1.0,
            double_fish_chance: 0.0,
            no_bait_consume_chance: 0.0,
            ore_prewrite_letters: 0,
            pickaxe_tier: 0,
        }
    }
}

fn one() -> f32 {
    1.0
}

/// Recipe: ingots required by id + quantity, plus a flat valu cost.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Recipe {
    #[serde(default)]
    pub ingots: Vec<(String, u32)>,
    #[serde(default)]
    pub valu: u64,
}

/// Static gear definition loaded from JSON. `id` is the canonical key
/// referenced from `EquippedGear` slots and from forge recipes.
#[derive(Clone, Debug, Deserialize)]
pub struct GearDef {
    pub id: String,
    pub slot: String,
    pub tier: u32,
    pub name: String,
    #[serde(default)]
    pub min_blacksmithing_level: u32,
    /// Secondary skill gate. Only nonzero for pickaxes (Mining) and any
    /// future skill-gated gear.
    #[serde(default)]
    pub min_mining_level: u32,
    #[serde(default)]
    pub recipe: Recipe,
    #[serde(default)]
    pub perks: Perks,
}

impl GearDef {
    pub fn slot_enum(&self) -> Option<Slot> {
        match self.slot.as_str() {
            "feet" => Some(Slot::Feet),
            "neck" => Some(Slot::Neck),
            "ring" => Some(Slot::Ring),
            "cape" => Some(Slot::Cape),
            "pickaxe" => Some(Slot::Pickaxe),
            _ => None,
        }
    }
}

static GEAR_CACHE: OnceLock<Vec<GearDef>> = OnceLock::new();

pub fn defs() -> &'static [GearDef] {
    GEAR_CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(GEAR_JSON)
            .expect("assets/gear.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("gear entry malformed"))
            .collect()
    })
}

pub fn def_by_id(id: &str) -> Option<&'static GearDef> {
    defs().iter().find(|d| d.id == id)
}

pub fn defs_for_slot(slot: Slot) -> Vec<&'static GearDef> {
    defs()
        .iter()
        .filter(|d| d.slot_enum() == Some(slot))
        .collect()
}

/// Player's currently-equipped gear. Each slot holds an optional id of a
/// `GearDef` in the catalog. None = nothing equipped in that slot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EquippedGear {
    #[serde(default)]
    pub feet: Option<String>,
    #[serde(default)]
    pub neck: Option<String>,
    #[serde(default)]
    pub ring: Option<String>,
    #[serde(default)]
    pub cape: Option<String>,
    #[serde(default)]
    pub pickaxe: Option<String>,
    /// Ids of every piece the player has crafted (kept across equip/sell).
    /// Used by the inventory/forge UI to know what's already been made.
    #[serde(default)]
    pub owned: Vec<String>,
}

impl EquippedGear {
    pub fn equipped(&self, slot: Slot) -> Option<&str> {
        match slot {
            Slot::Feet => self.feet.as_deref(),
            Slot::Neck => self.neck.as_deref(),
            Slot::Ring => self.ring.as_deref(),
            Slot::Cape => self.cape.as_deref(),
            Slot::Pickaxe => self.pickaxe.as_deref(),
        }
    }

    pub fn equip(&mut self, slot: Slot, id: Option<String>) {
        match slot {
            Slot::Feet => self.feet = id,
            Slot::Neck => self.neck = id,
            Slot::Ring => self.ring = id,
            Slot::Cape => self.cape = id,
            Slot::Pickaxe => self.pickaxe = id,
        }
    }

    pub fn owns(&self, id: &str) -> bool {
        self.owned.iter().any(|x| x == id)
    }

    pub fn add_owned(&mut self, id: &str) {
        if !self.owns(id) {
            self.owned.push(id.to_string());
        }
    }

    /// Sum perks across every equipped slot. Multipliers compose
    /// multiplicatively; chances add additively (clamped to 1.0).
    pub fn combined_perks(&self) -> Perks {
        let mut out = Perks::default();
        for slot in Slot::ALL {
            let Some(id) = self.equipped(slot) else { continue };
            let Some(def) = def_by_id(id) else { continue };
            let p = def.perks;
            out.move_speed_mult *= p.move_speed_mult;
            out.stamina_loss_mult *= p.stamina_loss_mult;
            out.double_fish_chance =
                (out.double_fish_chance + p.double_fish_chance).min(1.0);
            out.no_bait_consume_chance =
                (out.no_bait_consume_chance + p.no_bait_consume_chance).min(1.0);
            out.ore_prewrite_letters =
                out.ore_prewrite_letters.max(p.ore_prewrite_letters);
            out.pickaxe_tier = out.pickaxe_tier.max(p.pickaxe_tier);
        }
        out
    }
}
