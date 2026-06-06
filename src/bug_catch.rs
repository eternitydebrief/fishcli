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
}

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
        let r1 = next_rand_f32(rng_state);
        let center = 0.20 + r1 * 0.60;
        let width = (0.18 + target_widen).clamp(0.08, 0.55);
        let target_lo = (center - width * 0.5).max(0.0);
        let target_hi = (center + width * 0.5).min(1.0);
        Self {
            bug_id,
            world_xy,
            cursor: 0.0,
            direction: 1.0,
            speed: 0.020,
            target_lo,
            target_hi,
            result: None,
            deadline_tick: tick + 200, // ~10s at 20fps
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
            self.result = Some(BugCatchResult::Missed);
        }
    }

    pub fn attempt(&mut self) {
        if self.result.is_some() {
            return;
        }
        self.result = Some(if self.cursor >= self.target_lo && self.cursor <= self.target_hi {
            BugCatchResult::Caught
        } else {
            BugCatchResult::Missed
        });
    }

    pub fn in_target(&self) -> bool {
        self.cursor >= self.target_lo && self.cursor <= self.target_hi
    }
}
