use serde::Deserialize;
use std::sync::OnceLock;

const RODS_JSON: &str = include_str!("../assets/rods.json");

#[derive(Clone, Debug)]
pub struct RodDef {
    pub tier: u32,
    pub name: String,
}

impl RodDef {
    /// Price in valu to buy this rod. Cubic + quadratic curve tuned so the
    /// full 200-rod ladder takes hundreds of hours of cumulative income to
    /// climb. Early rods are tens-of-valu cheap; tier 200 costs millions.
    ///   tier   1 → ~80
    ///   tier  11 → ~5k     (cumulative ~20k)
    ///   tier  50 → ~200k   (cumulative ~3M)
    ///   tier 100 → ~1.3M   (cumulative ~33M)
    ///   tier 200 → ~9M     (cumulative ~485M)
    pub fn price(&self) -> u64 {
        let t = self.tier as u64;
        (t * t * t) + (30 * t * t) + 50
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

/// Mastery gate: returns `(difficulty, count)` of catches required across
/// any non-unique non-joke fish of that difficulty before this rod tier is
/// buyable. Tiers below 26 (the easy on-ramp) are ungated; tiers 201/202
/// (Fishing Rod, The Rod) have their own custom Pantheon gates and skip
/// this check. Keeps players from money-saving past whole biomes.
pub fn mastery_gate(tier: u32) -> Option<(u8, u32)> {
    match tier {
        0..=25 => None,
        26..=50 => Some((2, 5)),
        51..=75 => Some((3, 5)),
        76..=100 => Some((4, 5)),
        101..=130 => Some((5, 5)),
        131..=160 => Some((6, 5)),
        161..=180 => Some((7, 5)),
        181..=200 => Some((8, 5)),
        _ => None,
    }
}
