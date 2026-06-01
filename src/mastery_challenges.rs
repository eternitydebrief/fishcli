#![allow(dead_code)]
//! Per-fish mastery challenges. Each non-unique species auto-generates a
//! handful of challenges (big hauler / streak / bulk sale / huge catch);
//! their targets scale with the fish's difficulty so easy fish are quick
//! wins and hard fish are long campaigns. Completing a challenge grants
//! skill points (fed into the skill tree).
//!
//! Challenges are *generated*, not authored — there are too many fish to
//! hand-write per-species. If the user wants narrative variants for
//! specific fish (e.g. story bosses), wire a JSON override file later.

use crate::fish::FishDef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChallengeKind {
    /// Catch N "large" catches of this fish.
    CatchLarge,
    /// Catch N "huge" catches of this fish.
    CatchHuge,
    /// Catch N consecutive of this fish (the streak resets when a
    /// different species is caught).
    Streak,
    /// Sell N copies of this fish in a single transaction.
    BulkSale,
}

#[derive(Clone, Debug)]
pub struct Challenge {
    pub id: String,
    pub fish_name: String,
    pub kind: ChallengeKind,
    pub target: u32,
    pub reward_points: u32,
    pub title: String,
    pub description: String,
}

pub fn challenges_for(fish: &FishDef) -> Vec<Challenge> {
    if fish.unique || fish.joke {
        return Vec::new();
    }
    let d = fish.difficulty.max(1) as u32;
    let large_target = (5 + d).min(20);
    let huge_target = (3 + d).min(12);
    let streak_target = (3 + d / 2).min(10);
    let bulk_target = (10 + d * 2).min(40);
    let n = fish.name.clone();
    vec![
        Challenge {
            id: format!("ch-large-{}", n),
            fish_name: n.clone(),
            kind: ChallengeKind::CatchLarge,
            target: large_target,
            reward_points: 1,
            title: format!("Big Hauler: {n}"),
            description: format!("Catch {large_target} LARGE {n}."),
        },
        Challenge {
            id: format!("ch-huge-{}", n),
            fish_name: n.clone(),
            kind: ChallengeKind::CatchHuge,
            target: huge_target,
            reward_points: 2,
            title: format!("Huge Catch: {n}"),
            description: format!("Catch {huge_target} HUGE {n}."),
        },
        Challenge {
            id: format!("ch-streak-{}", n),
            fish_name: n.clone(),
            kind: ChallengeKind::Streak,
            target: streak_target,
            reward_points: 1,
            title: format!("Streak: {n}"),
            description: format!("Catch {streak_target} {n} in a row."),
        },
        Challenge {
            id: format!("ch-bulk-{}", n),
            fish_name: n,
            kind: ChallengeKind::BulkSale,
            target: bulk_target,
            reward_points: 1,
            title: format!("Bulk Sale: {fish_name}", fish_name = fish.name),
            description: format!("Sell {bulk_target} {} in one transaction.", fish.name),
        },
    ]
}

/// Cached `(fish_name -> Vec<Challenge>)` so we don't regenerate every
/// catch. Built lazily on first lookup.
use std::collections::HashMap;
use std::sync::OnceLock;

static CACHE: OnceLock<HashMap<String, Vec<Challenge>>> = OnceLock::new();

pub fn challenges_by_fish() -> &'static HashMap<String, Vec<Challenge>> {
    CACHE.get_or_init(|| {
        let mut m: HashMap<String, Vec<Challenge>> = HashMap::new();
        for f in crate::fishlist::fish() {
            let ch = challenges_for(f);
            if !ch.is_empty() {
                m.insert(f.name.clone(), ch);
            }
        }
        m
    })
}

pub fn challenges_for_name(name: &str) -> &'static [Challenge] {
    challenges_by_fish()
        .get(name)
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

/// Coarse size class for a catch. Determined per-event by an RNG sample;
/// callers narrate the size class to the player and feed it into challenge
/// detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeClass {
    Small,
    Normal,
    Large,
    Huge,
}

impl SizeClass {
    pub fn label(self) -> &'static str {
        match self {
            SizeClass::Small => "small",
            SizeClass::Normal => "normal",
            SizeClass::Large => "large",
            SizeClass::Huge => "huge",
        }
    }
}

/// Roll a size class from a uniform float in [0, 1).
/// Distribution: small 35%, normal 45%, large 15%, huge 5%.
pub fn roll_size(r: f32) -> SizeClass {
    if r < 0.35 {
        SizeClass::Small
    } else if r < 0.80 {
        SizeClass::Normal
    } else if r < 0.95 {
        SizeClass::Large
    } else {
        SizeClass::Huge
    }
}
