use serde::{Deserialize, Serialize};

/// Lifetime counters of player actions. Each field is monotonic.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub steps: u64,
    pub casts: u64,
    pub fish_caught: u64,
    pub fish_escaped: u64,
    pub items_picked: u64,
    pub quests_completed: u64,
    pub npcs_talked: u64,
    pub valu_earned: u64,
    pub fish_sold: u64,
    /// Lifetime well casts. At 100, the next well interaction drops the
    /// player into the Inferno (one-time portal opens then stays open).
    #[serde(default)]
    pub well_casts: u64,
    /// Total wood logs collected (sum of chop yields).
    #[serde(default)]
    pub wood_chopped: u64,
    /// Number of trees the player has felled (each `:chop` minigame win).
    #[serde(default)]
    pub trees_felled: u64,
}

/// Skill XP totals. Levels are derived from xp via [`xp_to_level`].
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Skills {
    pub fishing_xp: u64,
    pub walking_xp: u64,
    pub negotiation_xp: u64,
    pub mining_xp: u64,
    pub woodcutting_xp: u64,
    #[serde(default)]
    pub blacksmithing_xp: u64,
    #[serde(default)]
    pub cooking_xp: u64,
    /// Encyclopedia xp — accumulated from first-time discoveries (a fish
    /// you've never caught, a recipe that's just been unlocked). Levels
    /// reward you with skill points like the other lines.
    #[serde(default)]
    pub encyclopedia_xp: u64,
}

impl Skills {
    pub fn fishing_level(&self) -> u32 {
        xp_to_level(self.fishing_xp)
    }
    pub fn walking_level(&self) -> u32 {
        xp_to_level(self.walking_xp)
    }
    pub fn negotiation_level(&self) -> u32 {
        xp_to_level(self.negotiation_xp)
    }
    pub fn mining_level(&self) -> u32 {
        xp_to_level(self.mining_xp)
    }
    pub fn woodcutting_level(&self) -> u32 {
        xp_to_level(self.woodcutting_xp)
    }
    pub fn blacksmithing_level(&self) -> u32 {
        xp_to_level(self.blacksmithing_xp)
    }
    pub fn cooking_level(&self) -> u32 {
        xp_to_level(self.cooking_xp)
    }
    pub fn encyclopedia_level(&self) -> u32 {
        xp_to_level(self.encyclopedia_xp)
    }
}

/// Level n requires `level_to_xp(n)` total xp. Quadratic gap so each
/// next level takes 2n·k more xp than the last.
///
/// k = 125 is calibrated against the actual catch loop:
///   * ~25s per catch (cast + wait + reel + walk)
///   * 72 catches per 30 min
///   * XP per catch = difficulty² · 4 + 10
///   * Player at level N targets difficulty D where min_fishing_level(D) ≤ N
///
/// Resulting per-level time, fishing the difficulty appropriate for that level:
///   L1→L2     250 xp, diff 1  ( 14 xp/catch) →  18 catches ≈  8 min
///   L14→L15  3,500 xp, diff 4  ( 74 xp/catch) →  47 catches ≈ 20 min
///   L24→L25  6,000 xp, diff 5  (110 xp/catch) →  55 catches ≈ 23 min
///   L59→L60 14,750 xp, diff 7  (206 xp/catch) →  72 catches ≈ 30 min ← target
///   L89→L90 22,250 xp, diff 8  (266 xp/catch) →  84 catches ≈ 35 min
///   L179→L180 44,750 xp, diff 10 (410 xp/catch) → 109 catches ≈ 45 min
///
/// Very early levels run fast for first-hour dopamine; mid-game settles
/// at the 30-min target; late game trails out to ~45 min so endgame
/// mastery still feels earned.
pub fn level_to_xp(level: u32) -> u64 {
    let n = level.saturating_sub(1) as u64;
    n * (n + 1) * 125
}

pub fn xp_to_level(xp: u64) -> u32 {
    // invert n*(n+1)*125 = xp -> n ≈ sqrt(xp / 125)
    let approx = ((xp as f64 / 125.0).sqrt() as u32).max(0);
    // refine: walk forward until we exceed
    let mut lvl = approx + 1;
    while level_to_xp(lvl + 1) <= xp {
        lvl += 1;
    }
    while lvl > 1 && level_to_xp(lvl) > xp {
        lvl -= 1;
    }
    lvl
}

/// XP awarded for catching a fish of the given difficulty (1..=10).
pub fn fish_catch_xp(difficulty: u8) -> u64 {
    (difficulty as u64).pow(2) * 4 + 10
}
