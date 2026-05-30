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
}

/// Skill XP totals. Levels are derived from xp via [`xp_to_level`].
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Skills {
    pub fishing_xp: u64,
    pub walking_xp: u64,
    pub negotiation_xp: u64,
    pub mining_xp: u64,
    pub woodcutting_xp: u64,
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
}

/// Level n requires `level_to_xp(n)` total xp. Quadratic curve so each
/// level takes a bit longer than the last but never absurdly so.
///   level 1 = 0 xp
///   level 2 = 50 xp
///   level 10 = 2250 xp
///   level 50 = 60000 xp
///   level 100 = 247500 xp
pub fn level_to_xp(level: u32) -> u64 {
    let n = level.saturating_sub(1) as u64;
    n * (n + 1) * 25
}

pub fn xp_to_level(xp: u64) -> u32 {
    // invert n*(n+1)*25 = xp -> n ≈ sqrt(xp / 25)
    let approx = ((xp as f64 / 25.0).sqrt() as u32).max(0);
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
