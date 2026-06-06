#![allow(dead_code)]
//! Bugs: a second gathering loop alongside fishing. Bugs are caught with
//! the Bug Net on biome tiles (and a handful inside specialty dims) and
//! slot into the bait stock as targeted bait for specific fish pools.
//!
//! Definitions live in `assets/bugs.json`. The order there is the canonical
//! index used by the per-bug parallel `bugs_caught` mastery vector on
//! `SaveData` — only append to that file, never reorder, or old saves'
//! mastery counts will land on the wrong species.

use crate::world::{Biome, Dimension};
use ratatui::style::Color;
use serde::Deserialize;
use std::sync::OnceLock;

const BUGS_JSON: &str = include_str!("../assets/bugs.json");

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum Diurnal {
    #[serde(rename = "day")]
    Day,
    #[serde(rename = "night")]
    Night,
    #[serde(rename = "any")]
    Any,
}

impl Default for Diurnal {
    fn default() -> Self {
        Diurnal::Any
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct BugDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub glyph: String,
    pub color: String,
    /// Biomes this bug can spawn in (empty = any). Surface-only biomes.
    #[serde(default)]
    pub biomes: Vec<String>,
    /// Dimensions this bug can spawn in. Empty = Surface only (back-compat).
    #[serde(default)]
    pub dims: Vec<String>,
    /// Fish-pool tag this bug attracts when used as bait. Free-form string.
    pub pool_pull: String,
    /// Pool-pull weight multiplier (1.5 mild, 3.0 strong, 5.0 extreme).
    pub magnitude: f32,
    /// Optional generic effect (catch_speed | rare_chance | valu_mult | xp_mult | bite_speed).
    #[serde(default)]
    pub generic_effect: String,
    #[serde(default)]
    pub generic_magnitude: f32,
    /// Spawn weight on eligible tiles (0..1).
    #[serde(default = "default_rarity")]
    pub rarity: f32,
    #[serde(default)]
    pub diurnal: Diurnal,
}

fn default_rarity() -> f32 {
    0.3
}

impl BugDef {
    pub fn render_char(&self) -> char {
        self.glyph.chars().next().unwrap_or('.')
    }

    pub fn render_color(&self) -> Color {
        match self.color.as_str() {
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "gray" => Color::Gray,
            "darkgray" => Color::DarkGray,
            "lightgreen" => Color::LightGreen,
            "lightblue" => Color::LightBlue,
            "lightyellow" => Color::LightYellow,
            "lightred" => Color::LightRed,
            "lightcyan" => Color::LightCyan,
            "lightmagenta" => Color::LightMagenta,
            _ => Color::White,
        }
    }

    /// True if this bug can spawn in the given (dim, biome) cell at this
    /// time of day. `is_night` is the game's day/night flag.
    pub fn eligible(&self, dim: Dimension, biome: Biome, is_night: bool) -> bool {
        // Diurnal filter
        match self.diurnal {
            Diurnal::Day if is_night => return false,
            Diurnal::Night if !is_night => return false,
            _ => {}
        }
        // Dim filter: empty = Surface only.
        let dim_label = dim.label();
        let dim_ok = if self.dims.is_empty() {
            matches!(dim, Dimension::Surface)
        } else {
            self.dims.iter().any(|d| d == dim_label || dim_matches(d, dim))
        };
        if !dim_ok {
            return false;
        }
        // Biome filter: empty = any biome. Only meaningful on Surface; in
        // dims with no biome concept, this short-circuits via empty list.
        if !self.biomes.is_empty() && !self.biomes.iter().any(|b| biome_matches(b, biome)) {
            return false;
        }
        true
    }
}

fn dim_matches(name: &str, dim: Dimension) -> bool {
    Dimension::from_name(name).map(|d| d == dim).unwrap_or(false)
}

fn biome_matches(name: &str, biome: Biome) -> bool {
    match name {
        "Meadow" => matches!(biome, Biome::Meadow),
        "Forest" => matches!(biome, Biome::Forest),
        "Rocky" | "Rocky Plain" => matches!(biome, Biome::Rocky),
        "Scrub" | "Scrubland" => matches!(biome, Biome::Scrub),
        "Desert" => matches!(biome, Biome::Desert),
        "Tundra" => matches!(biome, Biome::Tundra),
        "Swamp" => matches!(biome, Biome::Swamp),
        _ => false,
    }
}

static DEFS: OnceLock<Vec<BugDef>> = OnceLock::new();

pub fn defs() -> &'static [BugDef] {
    DEFS.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(BUGS_JSON)
            .expect("assets/bugs.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("bug entry malformed"))
            .collect()
    })
}

pub fn def_by_id(id: &str) -> Option<&'static BugDef> {
    defs().iter().find(|d| d.id == id)
}

pub fn index_of(id: &str) -> Option<usize> {
    defs().iter().position(|d| d.id == id)
}

/// True if `(x, y)` in `dim`/`biome` is a soil patch — a rare grass cell
/// where the bug net can be used to dig up worms. Deterministic from the
/// world seed so a patch never moves.
pub fn soil_at(x: i32, y: i32, dim: Dimension, biome: Biome, seed: u32) -> bool {
    if !matches!(dim, Dimension::Surface) {
        return false;
    }
    if !matches!(
        biome,
        Biome::Meadow | Biome::Forest | Biome::Swamp | Biome::Scrub
    ) {
        return false;
    }
    let h = hash3(seed.wrapping_add(0x501_7101), x as u32, y as u32);
    (h % 1000) < 5 // ~0.5% of eligible cells
}

/// Tile types that a soil patch can replace visually. Restricting to Grass
/// keeps soil out of trees/buildings/water.
pub fn tile_hosts_soil(t: crate::world::Tile) -> bool {
    matches!(t, crate::world::Tile::Grass)
}

/// Tiles that bugs are willing to sit on (walkable nature/floor tiles only,
/// excluding water, structures, and walls). Keeps bugs out of impossible
/// spots like rooftops and ocean.
pub fn tile_hosts_bugs(t: crate::world::Tile) -> bool {
    use crate::world::Tile;
    matches!(
        t,
        Tile::Grass
            | Tile::Sand
            | Tile::Pebble
            | Tile::Flower
            | Tile::CaveFloor
            | Tile::Seabed
            | Tile::DeepWater
            | Tile::InfernoFloor
    )
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a.wrapping_add(b.wrapping_mul(374_761_393));
    h = h.wrapping_add(c.wrapping_mul(668_265_263));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// Base per-cell probability a bug spawns on an eligible tile (~1 bug per 100
/// eligible tiles). Combined with per-bug `rarity` weighting when multiple
/// bugs share a (dim, biome, time-of-day) bucket.
const SPAWN_RATE: f32 = 0.01;

// Thread-local cache of the eligible-bug filter result keyed by
// (dim, biome, is_night). Filtering the global defs list per render cell
// was the dominant cost of the bug overlay; this cuts it to one filter
// per (dim, biome) bucket per worker.
thread_local! {
    static ELIG_CACHE: std::cell::RefCell<
        Option<(Dimension, Biome, bool, Vec<&'static BugDef>, f32)>,
    > = const { std::cell::RefCell::new(None) };
}

fn with_eligible<R>(
    dim: Dimension,
    biome: Biome,
    is_night: bool,
    f: impl FnOnce(&[&'static BugDef], f32) -> R,
) -> R {
    ELIG_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let needs_rebuild = match &*c {
            Some((d, b, n, _, _)) => *d != dim || *b != biome || *n != is_night,
            None => true,
        };
        if needs_rebuild {
            let v: Vec<&'static BugDef> = defs()
                .iter()
                .filter(|b| b.eligible(dim, biome, is_night))
                .collect();
            let total: f32 = v.iter().map(|b| b.rarity).sum();
            *c = Some((dim, biome, is_night, v, total));
        }
        let (_, _, _, v, total) = c.as_ref().unwrap();
        f(v.as_slice(), *total)
    })
}

/// Deterministic spawn: returns the bug (if any) that lives on `(wx, wy)` in
/// `dim`/`biome` for the given `day_id`. Fully derived from the cell, so the
/// same cell always hosts the same bug for the whole day, and changes the
/// next day. No state stored.
pub fn bug_at(
    wx: i32,
    wy: i32,
    dim: Dimension,
    biome: Biome,
    is_night: bool,
    day_id: u64,
    seed: u32,
) -> Option<&'static BugDef> {
    let cell_key = seed
        .wrapping_add(day_id as u32)
        .wrapping_add(day_id.rotate_left(11) as u32);
    let h = hash3(cell_key, wx as u32, wy as u32);
    let roll = (h % 10_000) as f32 / 10_000.0;
    if roll > SPAWN_RATE {
        return None;
    }
    with_eligible(dim, biome, is_night, |eligible, total| {
        if eligible.is_empty() || total <= 0.0 {
            return None;
        }
        let pick =
            (hash3(cell_key ^ 0xDEAD_BEEF, wx as u32, wy as u32) % 1_000_000) as f32 / 1_000_000.0;
        let mut target = pick * total;
        for b in eligible {
            if target <= b.rarity {
                return Some(*b);
            }
            target -= b.rarity;
        }
        eligible.last().copied()
    })
}

