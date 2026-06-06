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
