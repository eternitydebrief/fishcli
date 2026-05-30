use serde::{Deserialize, Serialize};

/// Permanent buffs accumulated from catching rare effect-bearing fish.
/// Each catch with an `effect` string in `fish.json` updates these.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Buffs {
    /// Additive bonus to all future sell prices. Final mult = 1.0 + this.
    pub price_mult_bonus: f32,
    /// Number of free rod purchases banked.
    pub free_rods: u32,
    /// Extra max cast distance in tiles, on top of strength-derived range.
    pub bobber_range_bonus: i32,
    /// Additive to wait time. Final mult = (1.0 + this).clamp(0.1, 2.0).
    /// Negative values mean shorter waits.
    pub wait_mult_bonus: f32,
    /// Walk speed bonus: each +0.05 shaves ~5% off step intervals.
    pub walk_speed_bonus: f32,
    /// Catch-rarity boost for rarer/harder fish. Currently stored for later.
    pub luck_bonus: f32,
}

impl Buffs {
    pub fn price_mult(&self) -> f32 {
        (1.0 + self.price_mult_bonus).max(0.1)
    }

    pub fn wait_mult(&self) -> f32 {
        (1.0 + self.wait_mult_bonus).clamp(0.1, 2.0)
    }

    pub fn walk_mult(&self) -> f32 {
        // 1.0 = baseline; bigger bonus = shorter interval (faster)
        (1.0 / (1.0 + self.walk_speed_bonus)).clamp(0.25, 1.5)
    }
}

/// Parse and apply an effect string from `FishDef::effect`.
/// Returns a short human-readable description for the narrator log,
/// or `None` if the string is unknown.
pub fn apply_effect(buffs: &mut Buffs, effect: &str) -> Option<(String, EffectKind)> {
    let (key, val) = match effect.split_once(':') {
        Some((k, v)) => (k.trim(), v.trim()),
        None => (effect.trim(), ""),
    };
    match key {
        "price_mult" => {
            let v: f32 = val.parse().ok()?;
            buffs.price_mult_bonus += v;
            Some((
                format!("Sell prices +{:.0}% forever.", v * 100.0),
                EffectKind::Persistent,
            ))
        }
        "free_rod" => {
            buffs.free_rods += 1;
            Some(("Next rod purchase is free.".to_string(), EffectKind::Persistent))
        }
        "bobber_range" => {
            let v: i32 = val.parse().ok()?;
            buffs.bobber_range_bonus += v;
            Some((
                format!("Cast distance +{v} tile(s) forever."),
                EffectKind::Persistent,
            ))
        }
        "wait_mult" => {
            let v: f32 = val.parse().ok()?;
            buffs.wait_mult_bonus += v;
            let pct = (v * 100.0).round() as i32;
            Some((
                format!("Bite wait {pct:+}% forever."),
                EffectKind::Persistent,
            ))
        }
        "walk_speed" => {
            let v: f32 = val.parse().ok()?;
            buffs.walk_speed_bonus += v;
            Some((
                format!("Walk speed +{:.0}% forever.", v * 100.0),
                EffectKind::Persistent,
            ))
        }
        "luck" => {
            let v: f32 = val.parse().ok()?;
            buffs.luck_bonus += v;
            Some((
                format!("Luck +{:.0}% forever.", v * 100.0),
                EffectKind::Persistent,
            ))
        }
        "fishing_xp" => {
            let v: u64 = val.parse().ok()?;
            Some((format!("+{v} Fishing XP burst."), EffectKind::FishingXp(v)))
        }
        _ => None,
    }
}

pub enum EffectKind {
    Persistent,
    /// Instant XP grant the caller must apply to the player's skills.
    FishingXp(u64),
}
