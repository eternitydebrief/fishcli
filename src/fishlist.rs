//! catalog of all catchable fish.
//!
//! to add a species, append a `FishDef::new(name, description, rarity, difficulty)`
//! to the array below. fields:
//!   name:        short common name, lowercase.
//!   description: one short flavour line shown in the fishdex.
//!   rarity:      weight in the random picker. larger = more common.
//!                guide: 1.0 abundant, 0.5 common, 0.1 uncommon,
//!                       0.05 rare, 0.005 legendary, 0.0005 mythic.
//!   difficulty:  1..=10. scales the minigame:
//!                  1  = huge catch zone, sluggish fish
//!                  10 = sliver of a zone, frantic fish
//!
//! ordering inside the array does not matter; the picker is weighted by rarity.

use crate::fish::FishDef;

pub const FISH: &[FishDef] = &[
    // -- abundant pondfish (diff 1-2) --
    FishDef::new("common carp", "a humble pond dweller. abundant year-round.", 1.00, 1),
    FishDef::new("bluegill", "scrappy little panfish. always hungry.", 0.95, 1),
    FishDef::new("yellow perch", "stripey and curious.", 0.80, 2),
    FishDef::new("crappie", "schools like silver coins in dim water.", 0.75, 2),

    // -- common stream/lake (diff 3-4) --
    FishDef::new("rainbow trout", "speckled like a wet rainbow.", 0.65, 3),
    FishDef::new("brown trout", "wary, deep-pool dweller.", 0.55, 4),
    FishDef::new("smallmouth bass", "fights twice its weight.", 0.45, 4),

    // -- uncommon (diff 5-6) --
    FishDef::new("walleye", "glowing eyes at dusk.", 0.30, 5),
    FishDef::new("salmon", "leaping upstream, single-minded.", 0.20, 6),
    FishDef::new("northern pike", "all teeth and ambush.", 0.18, 6),

    // -- rare (diff 7-8) --
    FishDef::new("muskellunge", "fish of ten thousand casts.", 0.05, 8),
    FishDef::new("sturgeon", "armored relic of an older river.", 0.03, 9),

    // -- legendary (diff 9-10) --
    FishDef::new("kraken's whisker", "a tendril of something far larger, briefly hooked.", 0.001, 10),
];
