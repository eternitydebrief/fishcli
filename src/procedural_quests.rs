//! Procedural bounties: one rolling quest the player can opt into by
//! talking to the Bounty Board NPC. Definitions are generated from the
//! catalog of caught fish + an RNG seed (so bounties stay flavor-faithful
//! to species the player has actually met).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProceduralQuest {
    pub id: String,
    pub fish_name: String,
    pub count: u32,
    pub progress: u32,
    pub reward_valu: u64,
    pub reward_points: u32,
}

impl ProceduralQuest {
    pub fn title(&self) -> String {
        format!("Bounty: {} x{}", self.fish_name, self.count)
    }
}

/// Roll a fresh bounty from the player's known fishdex. Returns None if
/// the player hasn't caught anything yet.
pub fn roll(
    caught: &[bool],
    rng_state: &mut u32,
) -> Option<ProceduralQuest> {
    let pool: Vec<usize> = caught
        .iter()
        .enumerate()
        .filter(|(_, c)| **c)
        .map(|(i, _)| i)
        .collect();
    if pool.is_empty() {
        return None;
    }
    let r = crate::fish::next_rand_f32(rng_state);
    let pick = pool[((r * pool.len() as f32) as usize).min(pool.len() - 1)];
    let f = &crate::fishlist::fish()[pick];
    if f.unique {
        // Unique fish (Fish, Five Elders) can't be sold or duplicated; refuse.
        return None;
    }
    let r2 = crate::fish::next_rand_f32(rng_state);
    let count = (3 + (r2 * 6.0) as u32).max(2); // 3..=8
    let reward_valu = (f.sell_price() as u64).saturating_mul(count as u64) * 2;
    let reward_points = 1u32 + (f.difficulty as u32 / 4);
    let id = format!(
        "bounty-{}-{}",
        f.name.replace(' ', "_").to_ascii_lowercase(),
        *rng_state
    );
    Some(ProceduralQuest {
        id,
        fish_name: f.name.clone(),
        count,
        progress: 0,
        reward_valu,
        reward_points,
    })
}
