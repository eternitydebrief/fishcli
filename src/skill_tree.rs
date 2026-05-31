//! Fishing School skill tree.
//!
//! Three parallel trees the player invests skill points into. Points come
//! from lifetime `casts` (one point per 1500 casts). The math is tuned so
//! maxing all three trees takes ~500 hours of active fishing.
//!
//! Each tree has two unlockable tiers (a third is planned later):
//!   - Tier 1 (named after the tree): cheap stat boost, 5 ranks
//!   - Tier 2: active or passive ability, unlocked only when T1 is fully
//!     ranked; 4 or 5 ranks of escalating effect.

use serde::{Deserialize, Serialize};

pub const T1_MAX_RANK: u32 = 5;
pub const TM_T2_MAX_RANK: u32 = 4;
pub const T3_MAX_RANK: u32 = 5;

/// Casts required per skill point. Tuned so 29 points (full tree maxed)
/// takes ~500 hours of active fishing.
pub const CASTS_PER_POINT: u64 = 1500;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SkillTree {
    /// Quickcatch tier 1: fishing speed boost (0..=5)
    pub quickcatch_t1: u32,
    /// Quickcatch tier 2: perfect throw bonus (0..=5)
    pub quickcatch_t2: u32,
    /// Rod of Legends tier 1: rectangle inertia reduce (0..=5)
    pub legends_t1: u32,
    /// Rod of Legends tier 2: active rectangle boost duration (0..=5)
    pub legends_t2: u32,
    /// Rod of Legends "Heavy Yank" — strength of the yank-down key.
    /// 0 = weak (0.30), 5 = full (1.20, same as yank-up). No prereq.
    pub legends_yank: u32,
    /// Quickcatch T3 "Effortless" — +10% catch progress per rank when
    /// fish is inside rectangle. Unlocked after Quickcatch T2 maxed.
    pub quickcatch_t3: u32,
    /// Rod of Legends T3 "Phantom Rod" — rectangle drifts toward fish_y
    /// when no input held; per-rank pull strength. Unlocked after T2 max.
    pub legends_t3: u32,
    /// Tamer T3 "Telepathic Lure" — fish doesn't change direction for
    /// first 2s per rank. Unlocked after Tamer T2 maxed.
    pub tamer_t3: u32,
    /// Tamer tier 1: fish chaos reduce (0..=5)
    pub tamer_t1: u32,
    /// Tamer tier 2: active fish slow strength (0..=4)
    pub tamer_t2: u32,
    /// Total points already invested in any node.
    pub spent: u32,
}

#[allow(dead_code)] // getters are planned-API for fishing minigame wiring
impl SkillTree {
    /// Total points the player has earned, derived from lifetime casts.
    pub fn earned(casts: u64) -> u32 {
        (casts / CASTS_PER_POINT) as u32
    }

    pub fn available(&self, casts: u64) -> u32 {
        Self::earned(casts).saturating_sub(self.spent)
    }

    pub fn total_invested(&self) -> u32 {
        self.quickcatch_t1
            + self.quickcatch_t2
            + self.quickcatch_t3
            + self.legends_t1
            + self.legends_t2
            + self.legends_yank
            + self.legends_t3
            + self.tamer_t1
            + self.tamer_t2
            + self.tamer_t3
    }

    /// Quickcatch T3 "Effortless": +10% per rank to in-rect catch progress
    /// on top of T1/T2.
    pub fn effortless_mult(&self) -> f32 {
        1.0 + 0.10 * self.quickcatch_t3 as f32
    }

    /// Rod of Legends T3 "Phantom Rod": pull strength toward fish_y when
    /// no input. 0.04/rank — at max ranks=5 → 0.20 (pretty grippy).
    pub fn phantom_pull(&self) -> f32 {
        0.04 * self.legends_t3 as f32
    }

    /// Tamer T3 "Telepathic Lure": fish doesn't change direction for the
    /// first N seconds; N = 2 * rank.
    pub fn telepathic_grace_frames(&self) -> u32 {
        40 * self.tamer_t3
    }

    /// Strength of the yank-down impulse. Scales from 0.30 (rank 0) to
    /// 1.20 (rank 5, matching yank-up).
    pub fn yank_down_strength(&self) -> f32 {
        0.30 + 0.18 * self.legends_yank as f32
    }

    // ---- effect getters -------------------------------------------------

    /// Multiplier applied to the catch progress rate in the reel minigame.
    /// +0.5% per Quickcatch T1 rank → up to +2.5% at rank 5.
    pub fn fishing_speed_mult(&self) -> f32 {
        1.0 + 0.005 * self.quickcatch_t1 as f32
    }

    /// Multiplier applied to catch progress when the player's cast was at
    /// near-max strength (a "perfect throw"). +20% per Quickcatch T2 rank.
    pub fn perfect_throw_mult(&self) -> f32 {
        1.0 + 0.20 * self.quickcatch_t2 as f32
    }

    /// Inertia reduction for the player rectangle. 0.0 = full inertia,
    /// 1.0 = perfectly robotic. At rank 5 = 1.0.
    pub fn inertia_reduce(&self) -> f32 {
        0.20 * self.legends_t1 as f32
    }

    /// Coyote time (in frames) — how long the rectangle hovers after the
    /// player stops pressing before gravity takes over. 0 by default, 12
    /// frames at max Rod of Legends T1.
    pub fn coyote_frames(&self) -> u32 {
        2 * self.legends_t1
    }

    /// Active rectangle-boost duration in 20-fps frames. Zero = ability
    /// not unlocked. Rank 1 = 40 frames (2s), rank 5 = 200 (10s).
    pub fn legends_boost_frames(&self) -> u32 {
        if self.legends_t2 == 0 {
            0
        } else {
            40 * self.legends_t2
        }
    }

    /// Multiplier applied to fish's chaos (target change interval). >1.0
    /// means fish change direction less often. +0.5% per Tamer T1 rank.
    pub fn tamer_calm_mult(&self) -> f32 {
        1.0 + 0.005 * self.tamer_t1 as f32
    }

    /// Active slow strength — fraction by which fish movement speed is
    /// reduced. 0.0 = ability not unlocked, 0.40 = rank 4 max.
    pub fn tamer_slow_strength(&self) -> f32 {
        0.10 * self.tamer_t2 as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillNode {
    QuickcatchT1,
    QuickcatchT2,
    QuickcatchT3,
    LegendsT1,
    LegendsT2,
    LegendsYank,
    LegendsT3,
    TamerT1,
    TamerT2,
    TamerT3,
}

impl SkillNode {
    pub const ALL: &'static [SkillNode] = &[
        SkillNode::QuickcatchT1,
        SkillNode::QuickcatchT2,
        SkillNode::QuickcatchT3,
        SkillNode::LegendsT1,
        SkillNode::LegendsT2,
        SkillNode::LegendsYank,
        SkillNode::LegendsT3,
        SkillNode::TamerT1,
        SkillNode::TamerT2,
        SkillNode::TamerT3,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SkillNode::QuickcatchT1 => "Quickcatch",
            SkillNode::QuickcatchT2 => "Perfect Throw",
            SkillNode::QuickcatchT3 => "Effortless",
            SkillNode::LegendsT1 => "Rod of Legends",
            SkillNode::LegendsT2 => "Rod Boost",
            SkillNode::LegendsYank => "Heavy Yank",
            SkillNode::LegendsT3 => "Phantom Rod",
            SkillNode::TamerT1 => "The Tamer",
            SkillNode::TamerT2 => "Slow Fish",
            SkillNode::TamerT3 => "Telepathic Lure",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SkillNode::QuickcatchT1 => "+0.5% catch speed per rank.",
            SkillNode::QuickcatchT2 => "+20% catch progress per rank when your throw was near-max strength.",
            SkillNode::QuickcatchT3 => "+10% per rank to in-rect catch progress. Stacks with the rest.",
            SkillNode::LegendsT1 => "Less rectangle inertia per rank. Maxed: nearly robotic + coyote hover.",
            SkillNode::LegendsT2 => "Active 'b' during fishing: +1 rectangle height for 2s/rank.",
            SkillNode::LegendsYank => "Stronger yank-down ('t') per rank. Maxed: equal to yank-up.",
            SkillNode::LegendsT3 => "Rectangle drifts toward the fish when no key is held; pull +4%/rank.",
            SkillNode::TamerT1 => "Fish change direction 0.5% less per rank.",
            SkillNode::TamerT2 => "Active 's' during fishing: slow fish by 10% per rank for 5s.",
            SkillNode::TamerT3 => "Fish stays still for the first 2s/rank of the reel minigame.",
        }
    }

    pub fn max_rank(self) -> u32 {
        match self {
            SkillNode::TamerT2 => TM_T2_MAX_RANK,
            SkillNode::QuickcatchT2 | SkillNode::LegendsT2 | SkillNode::LegendsYank => 5,
            SkillNode::QuickcatchT3 | SkillNode::LegendsT3 | SkillNode::TamerT3 => T3_MAX_RANK,
            _ => T1_MAX_RANK,
        }
    }

    /// The prereq node. None for T1/Yank nodes. T2 needs its T1 maxed.
    /// T3 needs its T2 maxed (Tamer T3 needs T2 maxed at 4).
    pub fn prerequisite(self) -> Option<SkillNode> {
        match self {
            SkillNode::QuickcatchT2 => Some(SkillNode::QuickcatchT1),
            SkillNode::LegendsT2 => Some(SkillNode::LegendsT1),
            SkillNode::TamerT2 => Some(SkillNode::TamerT1),
            SkillNode::QuickcatchT3 => Some(SkillNode::QuickcatchT2),
            SkillNode::LegendsT3 => Some(SkillNode::LegendsT2),
            SkillNode::TamerT3 => Some(SkillNode::TamerT2),
            _ => None,
        }
    }

    pub fn rank(self, tree: &SkillTree) -> u32 {
        match self {
            SkillNode::QuickcatchT1 => tree.quickcatch_t1,
            SkillNode::QuickcatchT2 => tree.quickcatch_t2,
            SkillNode::QuickcatchT3 => tree.quickcatch_t3,
            SkillNode::LegendsT1 => tree.legends_t1,
            SkillNode::LegendsT2 => tree.legends_t2,
            SkillNode::LegendsYank => tree.legends_yank,
            SkillNode::LegendsT3 => tree.legends_t3,
            SkillNode::TamerT1 => tree.tamer_t1,
            SkillNode::TamerT2 => tree.tamer_t2,
            SkillNode::TamerT3 => tree.tamer_t3,
        }
    }

    /// True if the node can accept another point right now.
    pub fn can_invest(self, tree: &SkillTree) -> bool {
        if self.rank(tree) >= self.max_rank() {
            return false;
        }
        if let Some(prereq) = self.prerequisite() {
            if prereq.rank(tree) < prereq.max_rank() {
                return false;
            }
        }
        true
    }
}

/// Invest one point in a node. No-op if the node can't accept it.
pub fn invest(tree: &mut SkillTree, node: SkillNode) -> bool {
    if !node.can_invest(tree) {
        return false;
    }
    match node {
        SkillNode::QuickcatchT1 => tree.quickcatch_t1 += 1,
        SkillNode::QuickcatchT2 => tree.quickcatch_t2 += 1,
        SkillNode::QuickcatchT3 => tree.quickcatch_t3 += 1,
        SkillNode::LegendsT1 => tree.legends_t1 += 1,
        SkillNode::LegendsT2 => tree.legends_t2 += 1,
        SkillNode::LegendsYank => tree.legends_yank += 1,
        SkillNode::LegendsT3 => tree.legends_t3 += 1,
        SkillNode::TamerT1 => tree.tamer_t1 += 1,
        SkillNode::TamerT2 => tree.tamer_t2 += 1,
        SkillNode::TamerT3 => tree.tamer_t3 += 1,
    }
    tree.spent += 1;
    true
}
