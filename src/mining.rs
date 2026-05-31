//! Mining: ore definitions, per-cell ore lookup, vein cooldown state.
//!
//! Each OreRock cell has a *predetermined* ore — derived from a hash of
//! (x, y, dim, seed) — so the same vein always gives the same ore.
//!
//! Each vein has 3 charges. After the third successful mine, the vein
//! enters a 30-minute real-time cooldown. The cooldown is wall-clock
//! based: the game can be closed and reopened; if 30 minutes elapse, the
//! vein is ready again.

use crate::world::Dimension;
use ratatui::style::Color;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub const COOLDOWN_SECS: u64 = 30 * 60;
pub const MAX_CHARGES: u8 = 3;

pub struct OreDef {
    pub name: &'static str,
    pub value: u64,
    pub color: Color,
}

pub const ORES: &[OreDef] = &[
    OreDef { name: "copper",     value: 60,  color: Color::Rgb(220, 130, 90) },
    OreDef { name: "iron",       value: 80,  color: Color::Rgb(180, 130, 100) },
    OreDef { name: "silver",     value: 120, color: Color::Rgb(200, 220, 240) },
    OreDef { name: "gold",       value: 200, color: Color::Rgb(230, 200, 90) },
    OreDef { name: "turquoise",  value: 90,  color: Color::Rgb(100, 220, 180) },
    OreDef { name: "amethyst",   value: 130, color: Color::Rgb(180, 160, 220) },
    OreDef { name: "ruby",       value: 220, color: Color::Rgb(240, 100, 100) },
    OreDef { name: "sapphire",   value: 220, color: Color::Rgb(80, 200, 240) },
    OreDef { name: "emerald",    value: 240, color: Color::Rgb(100, 220, 130) },
    OreDef { name: "diamond",    value: 500, color: Color::Rgb(230, 245, 255) },
];

fn hash3(x: i32, y: i32, dim: u32, seed: u32) -> u32 {
    let mut h = seed
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(x as u32 ^ 0x85eb_ca77)
        .wrapping_add((y as u32).wrapping_mul(0xC2B2_AE3D))
        .wrapping_add(dim.wrapping_mul(0x27D4_EB2F));
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// Resolve which ore lives at this vein. Pure function of position +
/// dimension + world seed — the same vein always yields the same ore.
pub fn ore_at_vein(x: i32, y: i32, dim: Dimension, seed: u32) -> &'static OreDef {
    let h = hash3(x, y, dim as u32, seed);
    // Weighted lookup: commons (copper, iron) most likely; diamond rare.
    let weights: [(usize, u32); 10] = [
        (0, 30), // copper
        (1, 25), // iron
        (2, 14), // silver
        (3, 10), // gold
        (4, 12), // turquoise
        (5, 8),  // amethyst
        (6, 5),  // ruby
        (7, 5),  // sapphire
        (8, 4),  // emerald
        (9, 1),  // diamond
    ];
    let total: u32 = weights.iter().map(|(_, w)| *w).sum();
    let pick = h % total;
    let mut acc = 0u32;
    for (idx, w) in weights {
        acc += w;
        if pick < acc {
            return &ORES[idx];
        }
    }
    &ORES[0]
}

#[derive(Clone, Copy)]
pub struct VeinState {
    /// Number of successful mines so far in this charge cycle (0..MAX_CHARGES).
    pub charges_used: u8,
    /// Unix-seconds timestamp at which the vein is next mineable. 0 = ready.
    pub ready_at_secs: u64,
}

impl Default for VeinState {
    fn default() -> Self {
        Self { charges_used: 0, ready_at_secs: 0 }
    }
}

pub type VeinKey = (Dimension, i32, i32);
pub type VeinMap = HashMap<VeinKey, VeinState>;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns the vein's status: Ready, OnCooldown(seconds_left).
pub enum VeinStatus {
    Ready,
    OnCooldown(u64),
}

pub fn vein_status(map: &VeinMap, key: VeinKey) -> VeinStatus {
    let now = now_secs();
    let state = map.get(&key).copied().unwrap_or_default();
    if state.charges_used < MAX_CHARGES {
        return VeinStatus::Ready;
    }
    if now >= state.ready_at_secs {
        VeinStatus::Ready
    } else {
        VeinStatus::OnCooldown(state.ready_at_secs - now)
    }
}

/// Record a successful mine. If this puts the vein at MAX_CHARGES, start
/// the cooldown clock. If charges had already expired (cooldown lapsed),
/// reset to 1 charge used.
pub fn record_mine(map: &mut VeinMap, key: VeinKey) {
    let now = now_secs();
    let state = map.entry(key).or_default();
    // Lapsed cooldown? Reset before counting this mine.
    if state.charges_used >= MAX_CHARGES && now >= state.ready_at_secs {
        state.charges_used = 0;
        state.ready_at_secs = 0;
    }
    state.charges_used = state.charges_used.saturating_add(1);
    if state.charges_used >= MAX_CHARGES {
        state.ready_at_secs = now + COOLDOWN_SECS;
    }
}

pub struct Mining {
    pub x: i32,
    pub y: i32,
    pub dim: Dimension,
    pub ore: &'static OreDef,
    pub typed: String,
}

impl Mining {
    pub fn new(x: i32, y: i32, dim: Dimension, ore: &'static OreDef) -> Self {
        Self { x, y, dim, ore, typed: String::new() }
    }

    /// Apply a typed character. Case-insensitive. Returns true when the
    /// ore name has been fully spelled out.
    pub fn type_char(&mut self, c: char) -> bool {
        let target = self.ore.name;
        let next_idx = self.typed.chars().count();
        if next_idx >= target.chars().count() {
            return true;
        }
        if let Some(expected) = target.chars().nth(next_idx) {
            if c.eq_ignore_ascii_case(&expected) {
                self.typed.push(expected);
            }
        }
        self.typed.chars().count() >= target.chars().count()
    }
}
