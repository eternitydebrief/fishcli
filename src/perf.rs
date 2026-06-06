#![allow(dead_code)]
//! Lightweight per-frame instrumentation. Each render phase calls
//! `Perf::scope("name", || ...)` (or the explicit `start`/`stop` pair) and
//! the recorded micros land in a 60-sample ring buffer keyed by phase name.
//! The `:perf` menu reads the rings and shows mean/p95/max per phase so we
//! can spot what's actually eating frametime.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const SAMPLES: usize = 60;

#[derive(Clone)]
pub struct Ring {
    pub samples: Vec<u64>, // microseconds, last `SAMPLES` writes
    pub head: usize,
    pub count: usize,
}

impl Ring {
    fn new() -> Self {
        Self {
            samples: vec![0; SAMPLES],
            head: 0,
            count: 0,
        }
    }
    fn push(&mut self, micros: u64) {
        self.samples[self.head] = micros;
        self.head = (self.head + 1) % SAMPLES;
        if self.count < SAMPLES {
            self.count += 1;
        }
    }
    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let sum: u64 = self.samples.iter().take(self.count).sum();
        sum / (self.count as u64)
    }
    pub fn max(&self) -> u64 {
        self.samples
            .iter()
            .take(self.count)
            .copied()
            .max()
            .unwrap_or(0)
    }
    /// Approximate p95 = 3rd largest of the last 60 samples.
    pub fn p95(&self) -> u64 {
        let mut v: Vec<u64> = self.samples.iter().take(self.count).copied().collect();
        v.sort_unstable();
        if v.is_empty() {
            return 0;
        }
        let idx = ((v.len() as f32) * 0.95) as usize;
        v[idx.min(v.len() - 1)]
    }
    pub fn last(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let prev = if self.head == 0 { SAMPLES - 1 } else { self.head - 1 };
        self.samples[prev]
    }
}

thread_local! {
    static RINGS: RefCell<BTreeMap<&'static str, Ring>> = RefCell::new(BTreeMap::new());
}

/// Record `micros` against the named phase.
pub fn record(phase: &'static str, micros: u64) {
    RINGS.with(|r| {
        let mut r = r.borrow_mut();
        r.entry(phase).or_insert_with(Ring::new).push(micros);
    });
}

/// Snapshot the current per-phase rings (cloned) for read-only display.
pub fn snapshot() -> Vec<(&'static str, Ring)> {
    RINGS.with(|r| r.borrow().iter().map(|(k, v)| (*k, v.clone())).collect())
}

pub struct Scope {
    phase: &'static str,
    start: Instant,
}

impl Scope {
    pub fn new(phase: &'static str) -> Self {
        Self {
            phase,
            start: Instant::now(),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let micros = self.start.elapsed().as_micros() as u64;
        record(self.phase, micros);
    }
}

// Atomic per-frame accumulators. The per-cell parallel render loop calls
// AddScope::new(&FOO_NS) on every cell, which fetch_adds the elapsed ns
// to FOO_NS. After the frame, `flush_world_atomics()` swaps each accum
// into the corresponding named ring and resets it. Cross-thread safe and
// lock-free; the Instant::now() probes are ~50ns each so this is cheap
// enough to keep on by default for one major phase at a time.
pub static WORLD_CELL_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_TILE_GET_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_RENDER_TILE_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_BUG_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_SOIL_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_NPC_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_TREE_RENDER_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_WATER_ANIM_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_GRASS_ANIM_NS: AtomicU64 = AtomicU64::new(0);
pub static WORLD_WATER_INFO_NS: AtomicU64 = AtomicU64::new(0);

pub struct AddScope {
    target: &'static AtomicU64,
    start: Instant,
}

impl AddScope {
    pub fn new(target: &'static AtomicU64) -> Self {
        Self { target, start: Instant::now() }
    }
}

impl Drop for AddScope {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos() as u64;
        self.target.fetch_add(ns, Ordering::Relaxed);
    }
}

/// Swap every world.* atomic accumulator into its named ring (microseconds)
/// and reset to 0 for the next frame. Call after the render of the world
/// view completes.
pub fn flush_world_atomics() {
    fn drain(name: &'static str, slot: &AtomicU64) {
        let ns = slot.swap(0, Ordering::Relaxed);
        record(name, ns / 1000);
    }
    drain("world.cell_total", &WORLD_CELL_NS);
    drain("world.tile_get", &WORLD_TILE_GET_NS);
    drain("world.render_tile", &WORLD_RENDER_TILE_NS);
    drain("world.bug_overlay", &WORLD_BUG_NS);
    drain("world.soil_overlay", &WORLD_SOIL_NS);
    drain("world.npc_overlay", &WORLD_NPC_NS);
    drain("world.tree_render", &WORLD_TREE_RENDER_NS);
    drain("world.water_anim", &WORLD_WATER_ANIM_NS);
    drain("world.grass_anim", &WORLD_GRASS_ANIM_NS);
    drain("world.water_info", &WORLD_WATER_INFO_NS);
}
