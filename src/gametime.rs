//! Game time and calendar.
//!
//! Real-time pacing: 1 real second = 6 game minutes. So 4 real minutes = 1
//! game day, 28 days = 1 game month, 10 months = 1 game year.
//! 4 seasons of uneven length: Spring(2 mo), Summer(3 mo), Autumn(2 mo),
//! Winter(3 mo) = 10 months total.

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

pub const DAYS_PER_MONTH: u64 = 28;
pub const MONTHS_PER_YEAR: u64 = 10;

/// 1-indexed day of month (1..=28).
pub fn day_of_month(total_play_secs: u64) -> u32 {
    (game_days(total_play_secs) % DAYS_PER_MONTH + 1) as u32
}

/// 0-indexed month (0..10).
pub fn month_of_year(total_play_secs: u64) -> u32 {
    ((game_days(total_play_secs) / DAYS_PER_MONTH) % MONTHS_PER_YEAR) as u32
}

pub fn year(total_play_secs: u64) -> u64 {
    game_days(total_play_secs) / (DAYS_PER_MONTH * MONTHS_PER_YEAR)
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
    // 10-month year: Spring 0-1, Summer 2-4, Autumn 5-6, Winter 7-9
    match month % MONTHS_PER_YEAR as u32 {
        0 | 1 => Season::Spring,
        2 | 3 | 4 => Season::Summer,
        5 | 6 => Season::Autumn,
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
    /// Multiplier applied to tile rgb values to simulate ambient light.
    /// 1.0 = bright daylight. ~0.35 = midnight. Smoothly varies through
    /// dawn/dusk so the world fades instead of snapping.
    pub fn light_factor(self) -> f32 {
        match self {
            TimeOfDay::Day => 1.0,
            TimeOfDay::Morning => 0.85,
            TimeOfDay::Evening => 0.65,
            TimeOfDay::Dusk => 0.50,
            TimeOfDay::Night => 0.40,
            TimeOfDay::Midnight => 0.32,
        }
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
