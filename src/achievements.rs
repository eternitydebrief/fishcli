#![allow(dead_code)]
//! Achievements: ~50 lifetime milestones, each grants 1-10 skill points
//! when first crossed. Definitions live in `assets/achievements.json` so
//! titles and descriptions are user-editable.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const ACH_JSON: &str = include_str!("../assets/achievements.json");

#[derive(Clone, Debug, Deserialize)]
pub struct AchievementDef {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Counter the engine compares against `target`:
    ///   catch_total / casts / steps / valu_earned / fish_sold
    ///   unique_species_caught / mastery_total / rod_tier
    ///   mining_level / fishing_level / play_hours
    ///   pickaxe / boat (target=1 means "owns it")
    ///   visit_dim (target=1=Mines, 2=Atlantis, 3=Inferno)
    pub kind: String,
    pub target: i64,
    pub reward_points: u32,
}

static DEFS: OnceLock<Vec<AchievementDef>> = OnceLock::new();

pub fn defs() -> &'static [AchievementDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(ACH_JSON)
            .expect("assets/achievements.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("achievement entry malformed"))
            .collect()
    })
}

/// Live progress snapshot used by the achievement evaluator. Each field is
/// the *current* counter on the player; achievements compare against it.
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
    pub already_unlocked: &'a [String],
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AchievementProgress {
    pub unlocked: Vec<String>,
    /// Total reward points granted from unlocked achievements (sum). Used
    /// as the skill-point contribution to `SkillTree::available`.
    pub points_granted: u32,
}

/// Returns the list of newly-unlocked achievements (not previously listed
/// in `snap.already_unlocked`) given the current snapshot. Caller is
/// responsible for adding them to the persistent unlocked list and
/// totalling `points_granted`.
pub fn newly_unlocked<'a>(snap: &Snapshot<'a>) -> Vec<&'static AchievementDef> {
    let mut out = Vec::new();
    for a in defs() {
        if snap.already_unlocked.iter().any(|id| id == &a.id) {
            continue;
        }
        let met = match a.kind.as_str() {
            "catch_total" => snap.catch_total as i64 >= a.target,
            "casts" => snap.casts as i64 >= a.target,
            "steps" => snap.steps as i64 >= a.target,
            "valu_earned" => snap.valu_earned as i64 >= a.target,
            "fish_sold" => snap.fish_sold as i64 >= a.target,
            "unique_species_caught" => snap.unique_species as i64 >= a.target,
            "mastery_total" => snap.mastery_total as i64 >= a.target,
            "rod_tier" => snap.rod_tier as i64 >= a.target,
            "mining_level" => snap.mining_level as i64 >= a.target,
            "fishing_level" => snap.fishing_level as i64 >= a.target,
            "play_hours" => snap.play_hours as i64 >= a.target,
            "pickaxe" => snap.has_pickaxe,
            "boat" => snap.has_boat,
            "visit_dim" => match a.target {
                1 => snap.visited_mines,
                2 => snap.visited_atlantis,
                3 => snap.visited_inferno,
                _ => false,
            },
            _ => false,
        };
        if met {
            out.push(a);
        }
    }
    out
}
