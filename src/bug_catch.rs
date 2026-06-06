//! Bug-net catch micro-game. A bug stands on a biome tile; the player
//! faces it and presses `f`. A horizontal bar pops up with a swinging
//! cursor and a target zone. Space (or `f`) attempts the catch when the
//! cursor is inside the zone.
//!
//! Target zone width tapers with relevant skill (placeholder for the
//! Entomologist skill tree); cursor speed is fixed for now. Deadline of
//! ~10s prevents stalling forever.

use crate::fish::next_rand_f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BugCatchResult {
    Caught,
    Missed,
}

#[derive(Clone)]
pub struct BugCatch {
    pub bug_id: String,
    /// World cell the bug occupies. Used by the caller to mark the cell
    /// as picked-for-today after a result so the glyph stops rendering.
    pub world_xy: (i32, i32),
    /// 0..1 normalized cursor position across the bar.
    pub cursor: f32,
    pub direction: f32,
    pub speed: f32,
    pub target_lo: f32,
    pub target_hi: f32,
    pub result: Option<BugCatchResult>,
    pub deadline_tick: u64,
    /// Swings remaining. The player gets a handful of attempts before the
    /// bug flies off; misses re-roll the target zone instead of ending the
    /// scene until this hits zero.
    pub attempts_left: u8,
    /// Held across attempts for re-rolling the target zone on each miss.
    pub rng_state: u32,
    /// Cached widening passed at construction so re-rolls keep the same
    /// skill-derived target size.
    pub widen: f32,
}

pub const MAX_ATTEMPTS: u8 = 3;

impl BugCatch {
    /// `target_widen` adds to the zone width (skill bonus). Width is clamped
    /// between 0.08 and 0.55 so the game stays a game.
    pub fn new(
        bug_id: String,
        world_xy: (i32, i32),
        rng_state: &mut u32,
        tick: u64,
        target_widen: f32,
    ) -> Self {
        let mut local_rng = *rng_state;
        let (lo, hi) = roll_target(&mut local_rng, target_widen);
        *rng_state = local_rng;
        Self {
            bug_id,
            world_xy,
            cursor: 0.0,
            direction: 1.0,
            speed: 0.020,
            target_lo: lo,
            target_hi: hi,
            result: None,
            deadline_tick: tick + 200, // ~10s at 20fps
            attempts_left: MAX_ATTEMPTS,
            rng_state: local_rng,
            widen: target_widen,
        }
    }

    pub fn tick(&mut self, current_tick: u64) {
        if self.result.is_some() {
            return;
        }
        self.cursor += self.direction * self.speed;
        if self.cursor >= 1.0 {
            self.cursor = 1.0;
            self.direction = -1.0;
        } else if self.cursor <= 0.0 {
            self.cursor = 0.0;
            self.direction = 1.0;
        }
        if current_tick >= self.deadline_tick {
            // Timing out consumes the whole bug — no more attempts.
            self.result = Some(BugCatchResult::Missed);
        }
    }

    /// One swing. A hit ends the scene as Caught. A miss decrements the
    /// remaining attempts; if any remain, re-roll the target zone for the
    /// next swing. If none remain, the scene ends as Missed.
    pub fn attempt(&mut self) {
        if self.result.is_some() {
            return;
        }
        if self.in_target() {
            self.result = Some(BugCatchResult::Caught);
            return;
        }
        self.attempts_left = self.attempts_left.saturating_sub(1);
        if self.attempts_left == 0 {
            self.result = Some(BugCatchResult::Missed);
            return;
        }
        let (lo, hi) = roll_target(&mut self.rng_state, self.widen);
        self.target_lo = lo;
        self.target_hi = hi;
    }

    pub fn in_target(&self) -> bool {
        self.cursor >= self.target_lo && self.cursor <= self.target_hi
    }
}

fn roll_target(rng: &mut u32, widen: f32) -> (f32, f32) {
    let r1 = next_rand_f32(rng);
    let center = 0.20 + r1 * 0.60;
    let width = (0.18 + widen).clamp(0.08, 0.55);
    let lo = (center - width * 0.5).max(0.0);
    let hi = (center + width * 0.5).min(1.0);
    (lo, hi)
}
