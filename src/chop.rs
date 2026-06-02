//! Lumberjacking minigame. The player must type a random sequence of
//! F / G / H / J keys to chop a tree. A wrong keypress locks the input
//! for 3 seconds (60 ticks at 20fps). Completing the sequence yields
//! wood + woodcutting xp.
//!
//! Sequence length tapers as the player levels up:
//!   level   1 -> 16 chars
//!   level  25 -> 12 chars
//!   level  50 -> 10 chars
//!   level 100 ->  6 chars
//!   level 200+->  4 chars (floor)

use crate::fish::next_rand_f32;

pub const LOCKOUT_TICKS: u64 = 60;
pub const KEYS: &[char] = &['f', 'g', 'h', 'j'];

#[derive(Clone)]
pub struct Chopping {
    pub sequence: Vec<char>,
    pub typed: usize,
    /// Tick value at which the lockout (if any) ends. 0 = no lockout.
    pub lockout_until_tick: u64,
    /// Wood the player will receive on completion. Set when the scene
    /// starts and tweaked downward if the player eats a lockout.
    pub wood_yield: u32,
}

impl Chopping {
    pub fn new(woodcutting_level: u32, rng_state: &mut u32) -> Self {
        let len = sequence_length(woodcutting_level);
        let mut seq = Vec::with_capacity(len);
        for _ in 0..len {
            let r = next_rand_f32(rng_state);
            let idx = ((r * KEYS.len() as f32) as usize).min(KEYS.len() - 1);
            seq.push(KEYS[idx]);
        }
        // Base yield matches the previous single-swing chop: 3 + level/5, capped
        // at 12. The minigame replaces the old free wood; surviving a clean run
        // gives full yield, eating any lockouts halves it (set when the
        // mistake hits in `type_char`).
        let wood_yield = (3 + (woodcutting_level / 5)).min(12);
        Self {
            sequence: seq,
            typed: 0,
            lockout_until_tick: 0,
            wood_yield,
        }
    }

    pub fn is_locked(&self, tick: u64) -> bool {
        tick < self.lockout_until_tick
    }

    /// Apply a keypress at `tick`. Returns true when the sequence is
    /// fully typed (caller should commit the yield + xp and exit).
    /// Wrong keys arm a `LOCKOUT_TICKS` window during which subsequent
    /// presses are ignored, and halve the wood reward (one-time penalty
    /// per chop, not per mistake).
    pub fn type_char(&mut self, c: char, tick: u64) -> bool {
        if self.is_locked(tick) {
            return false;
        }
        if self.typed >= self.sequence.len() {
            return true;
        }
        let expected = self.sequence[self.typed];
        if c.eq_ignore_ascii_case(&expected) {
            self.typed += 1;
            return self.typed >= self.sequence.len();
        }
        // Wrong key: lockout + first-mistake penalty.
        self.lockout_until_tick = tick + LOCKOUT_TICKS;
        if self.wood_yield > 1 {
            self.wood_yield /= 2;
        }
        false
    }
}

fn sequence_length(level: u32) -> usize {
    match level {
        0..=4 => 16,
        5..=14 => 14,
        15..=24 => 12,
        25..=49 => 10,
        50..=99 => 8,
        100..=199 => 6,
        _ => 4,
    }
}
