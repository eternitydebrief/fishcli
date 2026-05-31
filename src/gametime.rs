//! Game time and calendar.
//!
//! Real-time pacing: 1 real second = 6 game minutes. So 4 real minutes = 1
//! game day, 2 real hours = 1 game month (30 days), 6 real hours = 1 season
//! (3 months), 24 real hours = 1 year (4 seasons). Tuned so a casual session
//! sees the time of day change but a season takes a few sessions.

use ratatui::style::Color;

/// Game minutes since session start (well — since the player started fishing
/// the first time, since we use `total_play_secs`).
pub fn game_minutes(total_play_secs: u64) -> u64 {
    total_play_secs.saturating_mul(6)
}

pub fn game_hours(total_play_secs: u64) -> u64 {
    game_minutes(total_play_secs) / 60
}

pub fn game_days(total_play_secs: u64) -> u64 {
    game_hours(total_play_secs) / 24
}

/// Hour of day (0..24).
pub fn hour_of_day(total_play_secs: u64) -> u32 {
    (game_hours(total_play_secs) % 24) as u32
}

/// Minute of hour (0..60).
pub fn minute_of_hour(total_play_secs: u64) -> u32 {
    (game_minutes(total_play_secs) % 60) as u32
}

/// 1-indexed day of month (1..=30).
pub fn day_of_month(total_play_secs: u64) -> u32 {
    (game_days(total_play_secs) % 30 + 1) as u32
}

/// 0-indexed month (0..12).
pub fn month_of_year(total_play_secs: u64) -> u32 {
    ((game_days(total_play_secs) / 30) % 12) as u32
}

pub fn year(total_play_secs: u64) -> u64 {
    game_days(total_play_secs) / (30 * 12)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn label(self) -> &'static str {
        match self {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        }
    }
    pub fn color(self) -> Color {
        match self {
            Season::Spring => Color::LightGreen,
            Season::Summer => Color::LightYellow,
            Season::Autumn => Color::Rgb(220, 130, 60),
            Season::Winter => Color::LightCyan,
        }
    }
    pub fn icon(self) -> char {
        match self {
            Season::Spring => '*', // flower
            Season::Summer => '#', // sun
            Season::Autumn => '%', // leaf
            Season::Winter => '+', // snowflake
        }
    }
}

pub fn season_for_month(month: u32) -> Season {
    match month % 12 {
        0 | 1 | 2 => Season::Spring,
        3 | 4 | 5 => Season::Summer,
        6 | 7 | 8 => Season::Autumn,
        _ => Season::Winter,
    }
}

pub fn season(total_play_secs: u64) -> Season {
    season_for_month(month_of_year(total_play_secs))
}

/// Phase of the day. Dusk and Midnight are *special* phases: rare-fish
/// rarity weights are amplified 10x during them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeOfDay {
    Morning,
    Day,
    Evening,
    /// Special: rare fish 10x more likely
    Dusk,
    Night,
    /// Special: rare fish 10x more likely
    Midnight,
}

impl TimeOfDay {
    pub fn label(self) -> &'static str {
        match self {
            TimeOfDay::Morning => "Morning",
            TimeOfDay::Day => "Day",
            TimeOfDay::Evening => "Evening",
            TimeOfDay::Dusk => "Dusk",
            TimeOfDay::Night => "Night",
            TimeOfDay::Midnight => "Midnight",
        }
    }
    pub fn color(self) -> Color {
        match self {
            TimeOfDay::Morning => Color::Rgb(255, 200, 150),
            TimeOfDay::Day => Color::LightYellow,
            TimeOfDay::Evening => Color::Rgb(220, 130, 90),
            TimeOfDay::Dusk => Color::Rgb(220, 80, 150),
            TimeOfDay::Night => Color::Rgb(120, 150, 220),
            TimeOfDay::Midnight => Color::Rgb(180, 130, 255),
        }
    }
    pub fn icon(self) -> char {
        match self {
            TimeOfDay::Morning => '/',
            TimeOfDay::Day => '#',
            TimeOfDay::Evening => '\\',
            TimeOfDay::Dusk => '~',
            TimeOfDay::Night => '.',
            TimeOfDay::Midnight => '*',
        }
    }
    /// Whether this time slot bumps rare-fish probability.
    pub fn is_rare_window(self) -> bool {
        matches!(self, TimeOfDay::Dusk | TimeOfDay::Midnight)
    }
}

pub fn time_of_day(total_play_secs: u64) -> TimeOfDay {
    match hour_of_day(total_play_secs) {
        5 | 6 | 7 => TimeOfDay::Morning,
        8..=16 => TimeOfDay::Day,
        17 | 18 => TimeOfDay::Evening,
        19 | 20 => TimeOfDay::Dusk,
        21 | 22 | 23 | 1 | 2 | 3 | 4 => TimeOfDay::Night,
        _ => TimeOfDay::Midnight, // hour 0
    }
}
