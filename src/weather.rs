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

fn surface_weather(day: u64, biome: Biome, seed: u32) -> Weather {
    surface_weather_with_season(day, biome, seed, Season::Spring)
}

fn surface_weather_with_season(day: u64, biome: Biome, seed: u32, season: Season) -> Weather {
    // each biome has its own pool of plausible weathers, filtered by
    // season so winter doesn't see rain and summer doesn't see snow
    let candidates: &[Weather] = match biome {
        Biome::Desert => &[Weather::Clear, Weather::Scorching, Weather::Sandstorm, Weather::Windy],
        Biome::Tundra => &[Weather::Clear, Weather::Snow, Weather::Blizzard, Weather::Cloudy],
        Biome::Swamp => &[
            Weather::Clear,
            Weather::Rain,
            Weather::Fog,
            Weather::Cloudy,
            Weather::Thunderstorm,
            Weather::Snow,
        ],
        Biome::Forest => &[
            Weather::Clear,
            Weather::Rain,
            Weather::Cloudy,
            Weather::Windy,
            Weather::Thunderstorm,
            Weather::Snow,
        ],
        Biome::Meadow => &[
            Weather::Clear,
            Weather::Rain,
            Weather::Cloudy,
            Weather::Windy,
            Weather::Thunderstorm,
            Weather::Snow,
        ],
        Biome::Rocky | Biome::Scrub => &[
            Weather::Clear,
            Weather::Cloudy,
            Weather::Windy,
            Weather::Rain,
            Weather::Snow,
        ],
    };
    let pool: Vec<Weather> = candidates
        .iter()
        .copied()
        .filter(|w| weather_fits_season(*w, season))
        .collect();
    let pool: &[Weather] = if pool.is_empty() { candidates } else { &pool };
    let salt = 0x444 + biome_salt(biome) + season_salt(season);
    let h = hash_day(day, salt, seed) as usize % pool.len();
    pool[h]
}

fn weather_fits_season(w: Weather, s: Season) -> bool {
    use Season::*;
    use Weather::*;
    match (w, s) {
        // No rain or thunderstorms in winter (it'd be snow).
        (Rain, Winter) | (Thunderstorm, Winter) => false,
        // No snow or blizzards in summer.
        (Snow, Summer) | (Blizzard, Summer) => false,
        // Scorching only in summer.
        (Scorching, Spring) | (Scorching, Autumn) | (Scorching, Winter) => false,
        // Blizzards only in winter (snowflake in autumn is fine).
        (Blizzard, Spring) | (Blizzard, Autumn) => false,
        _ => true,
    }
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
