#![allow(dead_code)]
//! Foraging: f on a rock / tree / cactus / flower / pebble drops a piece
//! of bait. Drops are biome-aware so a meadow rock yields different
//! critters than a desert one. Each foraged cell is locked for the rest
//! of the in-game day to prevent farming the same boulder.

use crate::world::{Biome, Dimension, Tile};

/// What the player is searching when they interact with this object. Drives
/// the narrator message ("you lift the rock", "you peel back the bark", ...).
#[derive(Clone, Copy, Debug)]
pub enum ForageAction {
    LiftRock,
    PickAtRock,
    PryFromBoulder,
    SearchTrunk,
    SearchLeaves,
    SearchRoots,
    SearchCactus,
    SearchFlower,
    SearchPebble,
}

impl ForageAction {
    pub fn verb(self) -> &'static str {
        match self {
            ForageAction::LiftRock => "lift the rock",
            ForageAction::PickAtRock => "pick at the rock",
            ForageAction::PryFromBoulder => "pry a flake from the boulder",
            ForageAction::SearchTrunk => "poke a hole in the trunk",
            ForageAction::SearchLeaves => "search the leaves",
            ForageAction::SearchRoots => "rake around the roots",
            ForageAction::SearchCactus => "feel between the spines",
            ForageAction::SearchFlower => "split the petals open",
            ForageAction::SearchPebble => "kick the pebble loose",
        }
    }

    /// Flavor line shown when the player tries to forage a cell that's
    /// still cooling down. Deliberately vague about timing — the user
    /// should feel that they cleaned the spot out, not consult a clock.
    pub fn empty_line(self) -> &'static str {
        match self {
            ForageAction::LiftRock => "Already lifted this rock — nothing crawls under it now.",
            ForageAction::PickAtRock => "Picked clean. Try a different stone.",
            ForageAction::PryFromBoulder => "The boulder's good flakes are gone for now.",
            ForageAction::SearchTrunk => "This trunk's hollow is empty for now.",
            ForageAction::SearchLeaves => "These leaves are already turned out.",
            ForageAction::SearchRoots => "Already raked these roots.",
            ForageAction::SearchCactus => "Already searched between the spines.",
            ForageAction::SearchFlower => "These petals are picked clean.",
            ForageAction::SearchPebble => "This pebble's already been kicked over.",
        }
    }
}

/// Resolve which action + drop table to use on the faced tile. None = not
/// forageable. Drop entries are (bug_id, weight). The weighted pick happens
/// in the caller using the player's rng.
pub fn forage_at(
    tile: Tile,
    biome: Biome,
    dim: Dimension,
) -> Option<(ForageAction, &'static [(&'static str, f32)])> {
    if !matches!(dim, Dimension::Surface) {
        return None;
    }
    // Pebble: cheap universal worm-tier drop.
    if matches!(tile, Tile::Pebble) {
        return Some((ForageAction::SearchPebble, PEBBLE));
    }
    if matches!(tile, Tile::Flower) {
        return Some((ForageAction::SearchFlower, FLOWER));
    }
    if matches!(tile, Tile::Cactus) {
        return Some((ForageAction::SearchCactus, CACTUS));
    }
    if matches!(tile, Tile::Rock | Tile::MediumRock | Tile::BigRock) {
        let action = match tile {
            Tile::Rock => ForageAction::LiftRock,
            Tile::MediumRock => ForageAction::PickAtRock,
            _ => ForageAction::PryFromBoulder,
        };
        let table: &'static [(&'static str, f32)] = match biome {
            Biome::Meadow | Biome::Forest | Biome::Scrub => ROCK_GREEN,
            Biome::Rocky => ROCK_ROCKY,
            Biome::Desert => ROCK_DESERT,
            Biome::Tundra => ROCK_TUNDRA,
            Biome::Swamp => ROCK_SWAMP,
        };
        return Some((action, table));
    }
    if matches!(tile, Tile::TreeTrunk) {
        let table: &'static [(&'static str, f32)] = match biome {
            Biome::Forest => TRUNK_FOREST,
            Biome::Swamp => TRUNK_SWAMP,
            Biome::Tundra => TRUNK_TUNDRA,
            _ => TRUNK_DEFAULT,
        };
        return Some((ForageAction::SearchTrunk, table));
    }
    if matches!(tile, Tile::TreeCanopy) {
        let table: &'static [(&'static str, f32)] = match biome {
            Biome::Forest => CANOPY_FOREST,
            Biome::Swamp => CANOPY_SWAMP,
            Biome::Tundra => CANOPY_TUNDRA,
            _ => CANOPY_DEFAULT,
        };
        return Some((ForageAction::SearchLeaves, table));
    }
    None
}

// Drop tables. Mix the new forage-only bugs (rock-louse, wood-grub, ...)
// with overworld bugs so foraging surfaces the same species you might
// otherwise net on the ground. Weights add to no fixed total; the caller
// samples weighted-random over them.

const PEBBLE: &[(&str, f32)] = &[
    ("earthworm", 5.0),
    ("rock-louse", 3.0),
    ("pebble-worm", 1.5),
];

const FLOWER: &[(&str, f32)] = &[
    ("petal-mite", 3.0),
    ("meadow-ladybug", 1.5),
    ("meadow-butterfly", 0.8),
    ("scrub-ant", 1.0),
];

const CACTUS: &[(&str, f32)] = &[
    ("cactus-tick", 4.0),
    ("desert-scarab", 1.0),
    ("desert-sun-spider", 0.6),
];

const ROCK_GREEN: &[(&str, f32)] = &[
    ("rock-louse", 3.0),
    ("earthworm", 4.0),
    ("meadow-june-beetle", 1.0),
    ("scrub-grub", 1.2),
];

const ROCK_ROCKY: &[(&str, f32)] = &[
    ("rock-louse", 4.0),
    ("rocky-pill-bug", 2.0),
    ("rocky-centipede", 1.2),
    ("rocky-scorpion", 0.4),
];

const ROCK_DESERT: &[(&str, f32)] = &[
    ("cactus-tick", 1.5),
    ("desert-scarab", 1.5),
    ("desert-sun-spider", 1.0),
    ("desert-sandhopper", 2.0),
];

const ROCK_TUNDRA: &[(&str, f32)] = &[
    ("frost-aphid", 2.5),
    ("tundra-frozen-larva", 1.5),
    ("tundra-ice-flea", 2.0),
    ("rock-louse", 1.0),
];

const ROCK_SWAMP: &[(&str, f32)] = &[
    ("swamp-leech-pup", 2.5),
    ("swamp-leech", 1.0),
    ("swamp-mosquito", 2.0),
    ("rock-louse", 1.0),
];

const TRUNK_DEFAULT: &[(&str, f32)] = &[
    ("wood-grub", 3.0),
    ("earthworm", 2.0),
    ("forest-walking-stick", 0.8),
];

const TRUNK_FOREST: &[(&str, f32)] = &[
    ("wood-grub", 3.5),
    ("forest-stag-beetle", 1.5),
    ("forest-glowworm", 1.2),
    ("forest-walking-stick", 1.0),
];

const TRUNK_SWAMP: &[(&str, f32)] = &[
    ("wood-grub", 2.0),
    ("swamp-cricket", 1.5),
    ("swamp-leech-pup", 1.8),
];

const TRUNK_TUNDRA: &[(&str, f32)] = &[
    ("wood-grub", 2.0),
    ("frost-aphid", 2.0),
    ("tundra-frost-spider", 1.0),
];

const CANOPY_DEFAULT: &[(&str, f32)] = &[
    ("leaf-aphid", 3.0),
    ("meadow-butterfly", 1.0),
];

const CANOPY_FOREST: &[(&str, f32)] = &[
    ("leaf-aphid", 3.0),
    ("forest-mantis", 1.0),
    ("forest-walking-stick", 1.5),
    ("forest-stag-beetle", 0.8),
];

const CANOPY_SWAMP: &[(&str, f32)] = &[
    ("leaf-aphid", 2.0),
    ("swamp-dragonfly", 1.5),
    ("swamp-mosquito", 1.5),
];

const CANOPY_TUNDRA: &[(&str, f32)] = &[
    ("leaf-aphid", 1.5),
    ("frost-aphid", 3.0),
    ("tundra-ice-flea", 1.5),
];

/// Roll a weighted random pick from a drop table.
pub fn pick<'a>(table: &'a [(&'static str, f32)], rng: &mut u32) -> Option<&'static str> {
    if table.is_empty() {
        return None;
    }
    let total: f32 = table.iter().map(|(_, w)| *w).sum();
    if total <= 0.0 {
        return None;
    }
    let r = crate::fish::next_rand_f32(rng) * total;
    let mut acc = 0.0;
    for (id, w) in table {
        acc += w;
        if r <= acc {
            return Some(*id);
        }
    }
    table.last().map(|(id, _)| *id)
}
