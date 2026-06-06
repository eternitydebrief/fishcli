#![allow(dead_code)]
//! Tiered achievements. Each chain (e.g. "Walker") has 1..N ordered
//! tiers; the player completes them one at a time, and only the next
//! unmet tier in each chain is "active". Once a tier is unlocked, the
//! chain advances; the previous tier disappears from the active view.
//! Definitions live in `assets/achievements.json`.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const ACH_JSON: &str = include_str!("../assets/achievements.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Tier {
    pub target: i64,
    pub reward_points: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AchievementChain {
    pub id: String,
    pub title: String,
    /// Counter the engine compares against `target`:
    ///   catch_total / casts / steps / valu_earned / fish_sold
    ///   unique_species_caught / mastery_total / rod_tier
    ///   mining_level / fishing_level / play_hours
    ///   pickaxe / boat (target=1 means "owns it")
    ///   visit_dim (target=1=Mines, 2=Atlantis, 3=Inferno)
    pub kind: String,
    pub tiers: Vec<Tier>,
}

static DEFS: OnceLock<Vec<AchievementChain>> = OnceLock::new();

pub fn chains() -> &'static [AchievementChain] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(ACH_JSON)
            .expect("assets/achievements.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("achievement chain malformed"))
            .collect()
    })
}

/// Render a tier number as a Roman numeral (I, II, ..., X, XI, ...).
pub fn roman(n: u32) -> String {
    const NUMS: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut n = n;
    let mut s = String::new();
    for &(val, lit) in NUMS {
        while n >= val {
            s.push_str(lit);
            n -= val;
        }
    }
    if s.is_empty() {
        s.push_str("0");
    }
    s
}

pub struct Snapshot<'a> {
    pub catch_total: u64,
    pub casts: u64,
    pub steps: u64,
    pub valu_earned: u64,
    pub fish_sold: u64,
    pub unique_species: u32,
    pub mastery_total: u32,
    pub rod_tier: u32,
    pub mining_level: u32,
    pub fishing_level: u32,
    pub play_hours: u64,
    pub has_pickaxe: bool,
    pub has_boat: bool,
    pub visited_mines: bool,
    pub visited_atlantis: bool,
    pub visited_inferno: bool,
    pub recipes_cooked: u64,
    pub recipes_mastered: u32,
    pub recipes_discovered: u32,
    pub wood_chopped: u64,
    pub trees_felled: u64,
    pub encyclopedia_level: u32,
    pub cooking_level: u32,
    pub woodcutting_level: u32,
    pub hull_tier: u32,
    pub max_catch_streak: u64,
    pub shiny_catches: u64,
    pub fossils_caught: u64,
    pub fossils_unearthed: u64,
    pub already_unlocked: &'a [String],
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AchievementProgress {
    /// Format "chain_id:tier_idx" (1-indexed). Each entry = one unlocked tier.
    pub unlocked: Vec<String>,
    /// Total reward points granted across all unlocked tiers.
    pub points_granted: u32,
}

pub fn unlocked_id(chain_id: &str, tier_idx: usize) -> String {
    format!("{chain_id}:{tier_idx}")
}

/// How many tiers of this chain are already unlocked.
pub fn tiers_unlocked(unlocked: &[String], chain_id: &str) -> usize {
    let prefix = format!("{chain_id}:");
    unlocked.iter().filter(|u| u.starts_with(&prefix)).count()
}

/// Current counter value for a chain's kind. Returned as i64 to match
/// `Tier::target` for direct comparison.
pub fn counter_for(snap: &Snapshot, kind: &str) -> Option<i64> {
    Some(match kind {
        "catch_total" => snap.catch_total as i64,
        "casts" => snap.casts as i64,
        "steps" => snap.steps as i64,
        "valu_earned" => snap.valu_earned as i64,
        "fish_sold" => snap.fish_sold as i64,
        "unique_species_caught" => snap.unique_species as i64,
        "mastery_total" => snap.mastery_total as i64,
        "rod_tier" => snap.rod_tier as i64,
        "mining_level" => snap.mining_level as i64,
        "fishing_level" => snap.fishing_level as i64,
        "play_hours" => snap.play_hours as i64,
        "pickaxe" => {
            if snap.has_pickaxe { 1 } else { 0 }
        }
        "boat" => {
            if snap.has_boat { 1 } else { 0 }
        }
        "visit_dim" => {
            // Special: 1 if mines visited, 2 if atlantis, 3 if inferno.
            // We compare per-tier target against the most-visited value.
            let mut max = 0;
            if snap.visited_mines && 1 > max { max = 1; }
            if snap.visited_atlantis && 2 > max { max = 2; }
            if snap.visited_inferno && 3 > max { max = 3; }
            max
        }
        "recipes_cooked" => snap.recipes_cooked as i64,
        "recipes_mastered" => snap.recipes_mastered as i64,
        "recipes_discovered" => snap.recipes_discovered as i64,
        "wood_chopped" => snap.wood_chopped as i64,
        "trees_felled" => snap.trees_felled as i64,
        "encyclopedia_level" => snap.encyclopedia_level as i64,
        "cooking_level" => snap.cooking_level as i64,
        "woodcutting_level" => snap.woodcutting_level as i64,
        "hull_tier" => snap.hull_tier as i64,
        "max_catch_streak" => snap.max_catch_streak as i64,
        "shiny_catches" => snap.shiny_catches as i64,
        "fossils_caught" => snap.fossils_caught as i64,
        "fossils_unearthed" => snap.fossils_unearthed as i64,
        _ => return None,
    })
}

/// Returns newly-unlocked (chain_id, tier_idx_1based, reward_points,
/// display_title_with_roman) for everything that crossed its target this
/// snapshot.
pub fn newly_unlocked(snap: &Snapshot) -> Vec<(String, usize, u32, String)> {
    let mut out = Vec::new();
    for chain in chains() {
        let already = tiers_unlocked(snap.already_unlocked, &chain.id);
        let Some(val) = counter_for(snap, &chain.kind) else { continue };
        // Visit_dim chains are exact-match (each chain targets a specific dim).
        for (idx, tier) in chain.tiers.iter().enumerate() {
            if idx < already {
                continue;
            }
            let met = if chain.kind == "visit_dim" {
                val == tier.target
            } else {
                val >= tier.target
            };
            if met {
                let n = (idx + 1) as u32;
                let title = if chain.tiers.len() == 1 {
                    chain.title.clone()
                } else {
                    format!("{} {}", chain.title, roman(n))
                };
                out.push((chain.id.clone(), idx + 1, tier.reward_points, title));
            }
        }
    }
    out
}
