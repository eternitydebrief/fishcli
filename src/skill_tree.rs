#![allow(dead_code)]
//! Fishing School skill tree — data-driven.
//!
//! 75 nodes across 5 trees (Angler / Naturalist / Mariner / Prospector /
//! Spirit). Each node has 1-5 ranks; investing 1 point in a node unlocks
//! all of its direct children (no need to max). All node definitions live
//! in `assets/skill_tree.json` so labels and descriptions can be rewritten
//! without touching source. Source only knows the effect *ids* and how
//! each one resolves into a gameplay number.
//!
//! Skill points come from fishing level-ups (1 per level) plus future
//! achievement / mastery streams. Maxing every rank takes ~280 points.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const SKILLS_JSON: &str = include_str!("../assets/skill_tree.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TreeBranch {
    Angler,
    Naturalist,
    Mariner,
    Prospector,
    Spirit,
}

impl TreeBranch {
    pub fn label(self) -> &'static str {
        match self {
            TreeBranch::Angler => "Angler",
            TreeBranch::Naturalist => "Naturalist",
            TreeBranch::Mariner => "Mariner",
            TreeBranch::Prospector => "Prospector",
            TreeBranch::Spirit => "Spirit",
        }
    }
    pub const ALL: &'static [TreeBranch] = &[
        TreeBranch::Angler,
        TreeBranch::Naturalist,
        TreeBranch::Mariner,
        TreeBranch::Prospector,
        TreeBranch::Spirit,
    ];
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeDef {
    pub id: String,
    pub tree: TreeBranch,
    #[serde(default)]
    pub parents: Vec<String>,
    pub label: String,
    pub description: String,
    pub max_rank: u32,
    pub effect: String,
    #[serde(default)]
    pub per_rank: f32,
}

static NODES: OnceLock<Vec<NodeDef>> = OnceLock::new();

pub fn nodes() -> &'static [NodeDef] {
    NODES.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(SKILLS_JSON)
            .expect("assets/skill_tree.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("skill node entry malformed"))
            .collect()
    })
}

pub fn node_by_id(id: &str) -> Option<&'static NodeDef> {
    nodes().iter().find(|n| n.id == id)
}

pub fn nodes_in_tree(t: TreeBranch) -> Vec<&'static NodeDef> {
    nodes().iter().filter(|n| n.tree == t).collect()
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SkillTree {
    /// node_id -> rank invested. Empty by default.
    #[serde(default)]
    pub ranks: BTreeMap<String, u32>,
    #[serde(default)]
    pub spent: u32,
}

impl SkillTree {
    pub fn rank(&self, id: &str) -> u32 {
        self.ranks.get(id).copied().unwrap_or(0)
    }

    /// True when at least one direct parent (per JSON) has rank >= 1.
    /// Root nodes (no parents) are always unlocked.
    pub fn is_unlocked(&self, node: &NodeDef) -> bool {
        if node.parents.is_empty() {
            return true;
        }
        node.parents.iter().any(|p| self.rank(p) >= 1)
    }

    pub fn can_invest(&self, node: &NodeDef) -> bool {
        self.rank(&node.id) < node.max_rank && self.is_unlocked(node)
    }

    /// Skill points the player has earned across the lifetime of the
    /// save. Comes from fishing level + future streams.
    /// `encyclopedia_level` is a separate channel: each level grants 2
    /// extra skill points, so wide-roster discovery work is rewarded
    /// without overshadowing the fishing climb.
    pub fn earned(
        fishing_level: u32,
        achievements_unlocked: u32,
        mastery_milestones: u32,
        encyclopedia_level: u32,
    ) -> u32 {
        // Tapered curve so the tree doesn't self-complete from levelling
        // alone — players have to pick a build instead of a full sweep.
        //   L  1- 30 : 1 per level         (30 pts)
        //   L 31- 60 : 1 per 2 levels      (+15 -> 45 at L60)
        //   L 61-120 : 1 per 3 levels      (+20 -> 65 at L120)
        //   L121-240 : 1 per 5 levels      (+24 -> 89 at L240)
        //   L241+    : 1 per 8 levels
        // Achievements + mastery still grant 1 each so off-loop play still
        // contributes meaningfully.
        let l = fishing_level;
        let level_pts = if l <= 30 {
            l
        } else if l <= 60 {
            30 + (l - 30) / 2
        } else if l <= 120 {
            45 + (l - 60) / 3
        } else if l <= 240 {
            65 + (l - 120) / 5
        } else {
            89 + (l - 240) / 8
        };
        level_pts
            .saturating_add(achievements_unlocked)
            .saturating_add(mastery_milestones)
            .saturating_add(encyclopedia_level.saturating_mul(2))
    }

    pub fn available(
        &self,
        fishing_level: u32,
        achievements: u32,
        mastery: u32,
        encyclopedia_level: u32,
    ) -> u32 {
        Self::earned(fishing_level, achievements, mastery, encyclopedia_level)
            .saturating_sub(self.spent)
    }

    pub fn total_invested(&self) -> u32 {
        self.ranks.values().sum()
    }

    // ---- pre-aggregated effect accessors --------------------------------
    // Every effect known to gameplay code is a method here. Each one walks
    // the node list, summing contributions from invested nodes whose effect
    // string matches. This keeps the schema flat: adding a new node with an
    // existing effect string just works.

    fn sum(&self, effect: &str) -> f32 {
        let mut acc = 0.0f32;
        for n in nodes() {
            if n.effect == effect {
                acc += n.per_rank * self.rank(&n.id) as f32;
            }
        }
        acc
    }

    fn any(&self, effect: &str) -> bool {
        for n in nodes() {
            if n.effect == effect && self.rank(&n.id) >= 1 {
                return true;
            }
        }
        false
    }

    // ---- fishing minigame ------------------------------------------------

    pub fn fishing_speed_mult(&self) -> f32 {
        1.0 + self.sum("catch_speed_mult") + self.sum("master_angler_mult")
    }

    pub fn perfect_throw_mult(&self) -> f32 {
        1.0 + self.sum("perfect_throw_mult")
    }

    pub fn effortless_mult(&self) -> f32 {
        1.0 + self.sum("effortless_mult")
    }

    pub fn inertia_reduce(&self) -> f32 {
        // legacy node 'legends_t1' contributed 0.20/rank — new tree uses
        // a smaller per-rank to avoid runaway robotic control at low ranks.
        (self.sum("inertia_reduce") * 4.0).min(1.0)
    }

    pub fn coyote_frames(&self) -> u32 {
        self.sum("coyote_frames") as u32
    }

    pub fn legends_boost_frames(&self) -> u32 {
        // No direct equivalent in the new tree; reserve for a future
        // "rod boost" node. Kept here so the fishing scene compiles.
        0
    }

    pub fn yank_down_strength(&self) -> f32 {
        // Base 0.30, +0.18 per Heavy Yank rank (same as old behavior).
        0.30 + self.sum("yank_down_strength")
    }

    pub fn phantom_pull(&self) -> f32 {
        // Reserved — no node currently emits this effect.
        0.0
    }

    pub fn telepathic_grace_frames(&self) -> u32 {
        0
    }

    pub fn tamer_calm_mult(&self) -> f32 {
        // Reduce fish chaos. Reserved — no node currently emits this.
        1.0
    }

    pub fn tamer_slow_strength(&self) -> f32 {
        0.0
    }

    pub fn rect_h_bonus(&self) -> f32 {
        self.sum("rect_h_bonus")
    }

    pub fn bite_window_mult(&self) -> f32 {
        1.0 + self.sum("bite_window_mult")
    }

    pub fn snap_reel_mult(&self) -> f32 {
        1.0 + self.sum("snap_reel_mult")
    }

    pub fn line_tension_pct(&self) -> f32 {
        self.sum("line_tension_pct")
    }

    pub fn water_catch_pct(&self) -> f32 {
        self.sum("water_catch_pct")
    }

    // ---- value / xp multipliers ------------------------------------------

    /// Multiplier applied to fish sale value at the fishmonger.
    pub fn valu_mult(&self) -> f32 {
        1.0 + self.sum("valu_mult")
    }

    /// Multiplier applied to every xp source (fishing, mining, etc.).
    pub fn global_xp_mult(&self) -> f32 {
        1.0 + self.sum("global_xp_mult")
    }

    pub fn mining_xp_mult(&self) -> f32 {
        1.0 + self.sum("mining_xp_mult") + self.sum("mines_yield_mult")
    }

    pub fn ore_value_mult(&self) -> f32 {
        1.0 + self.sum("ore_value_mult") + self.sum("mines_yield_mult")
    }

    // ---- rarity / variant ------------------------------------------------

    /// Global rare-fish chance bump (additive to weight roll). Combined
    /// here from Lucky + spirit devotion + variant caller.
    pub fn rare_chance_bonus(&self) -> f32 {
        self.sum("global_rare_pct") + self.sum("variant_chance_pct")
    }

    pub fn morning_rare_bonus(&self) -> f32 {
        self.sum("morning_rare_pct")
    }

    pub fn night_rare_bonus(&self) -> f32 {
        self.sum("night_rare_pct")
    }

    pub fn ocean_rare_bonus(&self) -> f32 {
        self.sum("ocean_rare_pct")
    }

    // ---- biome value multipliers ----------------------------------------

    pub fn biome_value_mult(&self, biome_label: &str) -> f32 {
        let mut m = 1.0 + self.sum("biome_value_mult");
        match biome_label {
            "Desert" => m += self.sum("desert_value_pct"),
            "Tundra" => m += self.sum("tundra_value_pct"),
            "Swamp"  => m += self.sum("swamp_value_pct"),
            "Meadow" => m += self.sum("meadow_value_pct"),
            _ => {}
        }
        m
    }

    pub fn deepwater_value_mult(&self) -> f32 {
        1.0 + self.sum("deepwater_value_pct")
    }

    // ---- mining ----------------------------------------------------------

    /// Multiplier on the typing-speed display (purely visual / QoL).
    pub fn mining_typing_mult(&self) -> f32 {
        1.0 + self.sum("mining_typing_mult")
    }

    /// Seconds to subtract from a vein's cooldown on cooldown start.
    pub fn vein_cooldown_reduce(&self) -> u64 {
        self.sum("vein_cooldown_red") as u64
    }

    pub fn double_ore_pct(&self) -> f32 {
        self.sum("double_ore_pct")
    }

    pub fn deep_vein_pct(&self) -> f32 {
        self.sum("deep_vein_pct")
    }

    pub fn diamond_chance_pct(&self) -> f32 {
        self.sum("diamond_chance_pct")
    }

    pub fn rare_ore_pct(&self) -> f32 {
        self.sum("rare_ore_pct")
    }

    pub fn blast_first_letter(&self) -> bool {
        self.any("blast_first_letter")
    }

    pub fn wall_reveal_radius(&self) -> u32 {
        self.sum("wall_reveal_radius") as u32
    }

    pub fn typo_forgive(&self) -> bool {
        self.any("typo_forgive")
    }

    pub fn tunnel_through(&self) -> bool {
        self.any("tunnel_through")
    }

    // ---- mariner ---------------------------------------------------------

    pub fn boat_full_speed(&self) -> bool {
        self.any("boat_full_speed")
    }

    pub fn boat_anchor(&self) -> bool {
        self.any("boat_anchor")
    }

    pub fn atlantis_gate_reduction(&self) -> u64 {
        self.sum("atlantis_gate_red") as u64
    }

    pub fn basket_capacity_bonus(&self) -> u32 {
        self.sum("basket_capacity") as u32
    }

    pub fn fog_radius_bonus(&self) -> u32 {
        self.sum("fog_radius_bonus") as u32
    }

    pub fn escape_save_pct(&self) -> f32 {
        self.sum("escape_save_pct")
    }

    pub fn archivist_per_dex(&self) -> u64 {
        self.sum("archivist_per_dex") as u64
    }

    pub fn dawn_window_hours(&self) -> u32 {
        self.sum("dawn_window_hours") as u32
    }

    pub fn midnight_boost_pct(&self) -> f32 {
        self.sum("midnight_boost_pct")
    }

    // ---- stamina ---------------------------------------------------------

    pub fn stamina_max_bonus(&self) -> f32 {
        self.sum("stamina_max_bonus")
    }

    pub fn stamina_fish_regen_mult(&self) -> f32 {
        1.0 + self.sum("stamina_fish_regen")
    }

    pub fn stamina_walk_reduce(&self) -> f32 {
        self.sum("stamina_walk_red").min(0.9)
    }

    pub fn stamina_idle_regen(&self) -> f32 {
        self.sum("stamina_idle_regen")
    }

    pub fn stamina_second_wind(&self) -> bool {
        self.any("stamina_second_wind")
    }
}

pub fn invest(tree: &mut SkillTree, node: &NodeDef) -> bool {
    if !tree.can_invest(node) {
        return false;
    }
    let entry = tree.ranks.entry(node.id.clone()).or_insert(0);
    *entry += 1;
    tree.spent += 1;
    true
}
