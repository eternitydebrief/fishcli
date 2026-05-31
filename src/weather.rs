//! Regional weather per dimension/biome.
//!
//! Surface weather varies per biome (a desert won't see snow; a tundra
//! won't see a sandstorm). Other dimensions have their own "weather":
//!   - Mines:    Tectonic Activity (Low / Medium / High)
//!   - Inferno:  Temperature       (Low / Medium / High)
//!   - Atlantis: Population        (Low / Medium / High)
//!
//! Weather changes daily — the same in-game day produces the same weather
//! everywhere (so you can rely on it for the day).

use crate::gametime::Season;
use crate::world::{Biome, Dimension};
use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    // Surface
    Clear,
    Cloudy,
    Rain,
    Snow,
    Blizzard,
    Sandstorm,
    Scorching,
    Fog,
    Windy,
    Thunderstorm,
    /// Summer-only extreme: even hotter than Scorching.
    HeatWave,
    // Mines: Tectonic Activity
    TectonicLow,
    TectonicMedium,
    TectonicHigh,
    // Inferno: Temperature
    TempLow,
    TempMedium,
    TempHigh,
    // Atlantis: Population
    PopLow,
    PopMedium,
    PopHigh,
}

impl Weather {
    /// The category label shown in the HUD, always white.
    pub fn category(self) -> &'static str {
        match self {
            Weather::TectonicLow | Weather::TectonicMedium | Weather::TectonicHigh => {
                "Tectonic"
            }
            Weather::TempLow | Weather::TempMedium | Weather::TempHigh => "Temperature",
            Weather::PopLow | Weather::PopMedium | Weather::PopHigh => "Population",
            _ => "Weather",
        }
    }
    /// The value label (Low/Clear/etc.), tinted with `value_color`.
    pub fn value(self) -> &'static str {
        match self {
            Weather::Clear => "Clear",
            Weather::Cloudy => "Cloudy",
            Weather::Rain => "Rain",
            Weather::Snow => "Snow",
            Weather::Blizzard => "Blizzard",
            Weather::Sandstorm => "Sandstorm",
            Weather::Scorching => "Scorching",
            Weather::Fog => "Fog",
            Weather::Windy => "Windy",
            Weather::Thunderstorm => "Thunderstorm",
            Weather::HeatWave => "Heat Wave",
            Weather::TectonicLow | Weather::TempLow | Weather::PopLow => "Low",
            Weather::TectonicMedium | Weather::TempMedium | Weather::PopMedium => "Medium",
            Weather::TectonicHigh | Weather::TempHigh | Weather::PopHigh => "High",
        }
    }
    /// Color of the value text in the HUD.
    pub fn value_color(self) -> Color {
        match self {
            Weather::Clear => Color::LightYellow,
            Weather::Cloudy => Color::Gray,
            Weather::Rain => Color::LightBlue,
            Weather::Snow => Color::White,
            Weather::Blizzard => Color::LightCyan,
            Weather::Sandstorm => Color::Rgb(220, 180, 120),
            Weather::Scorching => Color::Rgb(255, 130, 50),
            Weather::Fog => Color::Rgb(170, 180, 180),
            Weather::Windy => Color::Rgb(180, 200, 220),
            Weather::Thunderstorm => Color::Rgb(200, 200, 255),
            Weather::HeatWave => Color::Rgb(255, 100, 30),
            Weather::TectonicLow | Weather::TempLow | Weather::PopLow => Color::Yellow,
            Weather::TectonicMedium | Weather::TempMedium | Weather::PopMedium => {
                Color::Rgb(255, 150, 60)
            }
            Weather::TectonicHigh | Weather::TempHigh | Weather::PopHigh => Color::Red,
        }
    }
    pub fn icon(self) -> char {
        match self {
            Weather::Clear => '*',
            Weather::Cloudy => '~',
            Weather::Rain => '/',
            Weather::Snow => '+',
            Weather::Blizzard => '#',
            Weather::Sandstorm => '~',
            Weather::Scorching => '*',
            Weather::Fog => '=',
            Weather::Windy => '>',
            Weather::Thunderstorm => '!',
            Weather::HeatWave => '*',
            Weather::TectonicLow | Weather::TempLow | Weather::PopLow => '.',
            Weather::TectonicMedium | Weather::TempMedium | Weather::PopMedium => '~',
            Weather::TectonicHigh | Weather::TempHigh | Weather::PopHigh => '#',
        }
    }
}

/// Stable hash of (day, dim, biome, seed) for daily-deterministic weather.
fn hash_day(day: u64, salt: u32, seed: u32) -> u32 {
    let mut h = seed.wrapping_add(0x517E_517E);
    h = h.wrapping_add((day as u32).wrapping_mul(2_654_435_761));
    h = h.wrapping_add(salt.wrapping_mul(374_761_393));
    h ^= h >> 13;
    h.wrapping_mul(1_274_126_177) ^ (h >> 16)
}

pub fn weather_for(day: u64, dim: Dimension, biome: Biome, seed: u32) -> Weather {
    weather_for_season(day, dim, biome, seed, Season::Spring)
}

/// Variant that takes the current season into account. Surface weather is
/// filtered: no rain in winter, no scorching outside summer, etc.
pub fn weather_for_with_season(
    day: u64,
    dim: Dimension,
    biome: Biome,
    seed: u32,
    season: Season,
) -> Weather {
    weather_for_season(day, dim, biome, seed, season)
}

fn weather_for_season(
    day: u64,
    dim: Dimension,
    biome: Biome,
    seed: u32,
    season: Season,
) -> Weather {
    let _ = season; // currently only the surface branch uses it
    match dim {
        Dimension::Mines => {
            let h = hash_day(day, 0x111, seed) % 3;
            match h {
                0 => Weather::TectonicLow,
                1 => Weather::TectonicMedium,
                _ => Weather::TectonicHigh,
            }
        }
        Dimension::Inferno => {
            let h = hash_day(day, 0x222, seed) % 3;
            match h {
                0 => Weather::TempLow,
                1 => Weather::TempMedium,
                _ => Weather::TempHigh,
            }
        }
        Dimension::Atlantis => {
            let h = hash_day(day, 0x333, seed) % 3;
            match h {
                0 => Weather::PopLow,
                1 => Weather::PopMedium,
                _ => Weather::PopHigh,
            }
        }
        Dimension::Surface => surface_weather_with_season(day, biome, seed, season),
    }
}

fn surface_weather_with_season(day: u64, biome: Biome, seed: u32, season: Season) -> Weather {
    // Two-stage roll. Per-season Clear chance: Summer 40%, Spring/Autumn 25%,
    // Winter 25%. Otherwise pick from the (biome, season) weighted table.
    let salt = 0x444 + biome_salt(biome) + season_salt(season);
    let h = hash_day(day, salt, seed);
    let clear_pct = match season {
        Season::Summer => 400,
        _ => 250,
    };
    if h % 1000 < clear_pct {
        return Weather::Clear;
    }
    let table = weather_table(biome, season);
    if table.is_empty() {
        return Weather::Clear;
    }
    let total: u32 = table.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return Weather::Clear;
    }
    let pick = (h / 1000) % total;
    let mut acc = 0;
    for (w, weight) in table {
        acc += weight;
        if pick < acc {
            return w;
        }
    }
    Weather::Clear
}

/// Per-(biome, season) weighted weather table. Higher weight = more likely.
/// Heat Wave only shows in Summer; Blizzard only in Winter; etc.
fn weather_table(biome: Biome, season: Season) -> Vec<(Weather, u32)> {
    use Season::*;
    use Weather::*;
    let base: Vec<(Weather, u32)> = match (biome, season) {
        // ---- Desert ----
        (Biome::Desert, Summer) => vec![
            (HeatWave, 30),
            (Scorching, 30),
            (Sandstorm, 25),
            (Windy, 15),
            (Cloudy, 5),
        ],
        (Biome::Desert, Spring) | (Biome::Desert, Autumn) => vec![
            (Sandstorm, 30),
            (Windy, 25),
            (Cloudy, 15),
            (Rain, 5),
        ],
        (Biome::Desert, Winter) => vec![
            (Sandstorm, 20),
            (Windy, 30),
            (Cloudy, 30),
            (Snow, 5),
        ],

        // ---- Tundra ----
        (Biome::Tundra, Winter) => vec![
            (Blizzard, 35),
            (Snow, 35),
            (Cloudy, 20),
            (Windy, 10),
        ],
        (Biome::Tundra, Autumn) | (Biome::Tundra, Spring) => vec![
            (Snow, 35),
            (Cloudy, 30),
            (Windy, 15),
            (Rain, 10),
            (Fog, 10),
        ],
        (Biome::Tundra, Summer) => vec![
            (Cloudy, 35),
            (Windy, 20),
            (Rain, 20),
            (Fog, 15),
        ],

        // ---- Swamp ----
        (Biome::Swamp, Summer) => vec![
            (Rain, 30),
            (Thunderstorm, 5),
            (Fog, 25),
            (Cloudy, 20),
            (HeatWave, 5),
            (Scorching, 10),
        ],
        (Biome::Swamp, Spring) | (Biome::Swamp, Autumn) => vec![
            (Rain, 35),
            (Thunderstorm, 10),
            (Fog, 30),
            (Cloudy, 15),
            (Windy, 5),
        ],
        (Biome::Swamp, Winter) => vec![
            (Snow, 35),
            (Fog, 30),
            (Cloudy, 25),
            (Blizzard, 5),
        ],

        // ---- Forest ----
        (Biome::Forest, Summer) => vec![
            (Rain, 25),
            (Thunderstorm, 5),
            (Cloudy, 25),
            (Windy, 15),
            (HeatWave, 5),
            (Scorching, 10),
        ],
        (Biome::Forest, Spring) | (Biome::Forest, Autumn) => vec![
            (Rain, 30),
            (Thunderstorm, 15),
            (Cloudy, 25),
            (Windy, 20),
            (Fog, 10),
        ],
        (Biome::Forest, Winter) => vec![
            (Snow, 35),
            (Blizzard, 10),
            (Cloudy, 30),
            (Windy, 15),
            (Fog, 10),
        ],

        // ---- Meadow ----
        (Biome::Meadow, Summer) => vec![
            (Rain, 25),
            (Thunderstorm, 5),
            (Cloudy, 25),
            (Windy, 15),
            (HeatWave, 10),
            (Scorching, 10),
        ],
        (Biome::Meadow, Spring) | (Biome::Meadow, Autumn) => vec![
            (Rain, 35),
            (Thunderstorm, 15),
            (Cloudy, 20),
            (Windy, 20),
        ],
        (Biome::Meadow, Winter) => vec![
            (Snow, 40),
            (Blizzard, 10),
            (Cloudy, 25),
            (Windy, 15),
        ],

        // ---- Rocky / Scrub (catch-all, similar to meadow but drier) ----
        (Biome::Rocky, s) | (Biome::Scrub, s) => match s {
            Summer => vec![(Cloudy, 30), (Windy, 30), (Rain, 15), (Scorching, 10)],
            Spring | Autumn => vec![(Cloudy, 30), (Windy, 25), (Rain, 25), (Fog, 10)],
            Winter => vec![(Snow, 35), (Cloudy, 30), (Windy, 20), (Blizzard, 5)],
        },
    };
    base
}

fn season_salt(s: Season) -> u32 {
    match s {
        Season::Spring => 0x10,
        Season::Summer => 0x20,
        Season::Autumn => 0x30,
        Season::Winter => 0x40,
    }
}

fn biome_salt(b: Biome) -> u32 {
    match b {
        Biome::Meadow => 1,
        Biome::Forest => 2,
        Biome::Rocky => 3,
        Biome::Scrub => 4,
        Biome::Desert => 5,
        Biome::Tundra => 6,
        Biome::Swamp => 7,
    }
}
