use serde::Deserialize;
use std::sync::OnceLock;

const RODS_JSON: &str = include_str!("../assets/rods.json");

#[derive(Clone, Debug)]
pub struct RodDef {
    pub tier: u32,
    pub name: String,
}

impl RodDef {
    /// Price in valu to buy this rod. Grows as tier^2 * 5, with a floor.
    pub fn price(&self) -> u64 {
        let t = self.tier as u64;
        (t * t * 5).max(20)
    }

    /// Multiplier applied to fish speed in the minigame. Each tier shaves
    /// 1% off (multiplicatively), so tier 200 ≈ 13.4% of original speed.
    /// (Planned wiring into the fishing minigame.)
    #[allow(dead_code)]
    pub fn fish_speed_mult(&self) -> f32 {
        0.99f32.powi(self.tier as i32)
    }
}

static RODS_CACHE: OnceLock<Vec<RodDef>> = OnceLock::new();

pub fn rods() -> &'static [RodDef] {
    RODS_CACHE.get_or_init(|| {
        let names: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(RODS_JSON)
            .expect("assets/rods.json failed to parse")
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) if !s.starts_with("_comment") => Some(s),
                _ => None,
            })
            .collect();
        names
            .into_iter()
            .enumerate()
            .map(|(i, name)| RodDef {
                tier: i as u32 + 1,
                name,
            })
            .collect()
    })
}

pub fn get(tier: u32) -> Option<&'static RodDef> {
    if tier == 0 {
        return None;
    }
    rods().get(tier as usize - 1)
}

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, Default)]
pub struct OwnedRods {
    /// highest rod tier the player has bought (0 = nothing, 1 = balsa, ...)
    pub max_owned: u32,
    /// currently equipped tier (clamped to max_owned)
    pub equipped: u32,
}
