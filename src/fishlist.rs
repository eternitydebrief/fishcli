use crate::fish::FishDef;
use std::sync::OnceLock;

const FISH_JSON: &str = include_str!("../assets/fish.json");

static FISH_CACHE: OnceLock<Vec<FishDef>> = OnceLock::new();

pub fn fish() -> &'static [FishDef] {
    FISH_CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(FISH_JSON)
            .expect("assets/fish.json failed to parse");
        let mut base: Vec<FishDef> = raw
            .into_iter()
            .filter(|v| {
                // skip _comment entries that have no name field
                v.get("name").and_then(|n| n.as_str()).is_some()
            })
            .map(|v| serde_json::from_value(v).expect("fish entry malformed"))
            .collect();
        let variants = generate_variants(&base);
        base.extend(variants);
        base
    })
}

/// A category of variant fish (Astral, Sapphire, Ruby, ...) plus the stat
/// overlays to apply when generating each variant from a base species.
struct Variant {
    prefix: &'static str,
    pool: &'static str,
    price_mult: u64,
    difficulty_bump: u8,
    rarity: f32,
}

const VARIANTS: &[Variant] = &[
    Variant { prefix: "Astral",    pool: "cosmic",   price_mult: 5, difficulty_bump: 2, rarity: 0.05 },
    Variant { prefix: "Hot",       pool: "hot",      price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Burning",   pool: "burning",  price_mult: 5, difficulty_bump: 2, rarity: 0.05 },
    Variant { prefix: "Infernal",  pool: "infernal", price_mult: 8, difficulty_bump: 3, rarity: 0.03 },
    Variant { prefix: "Angelic",   pool: "angelic",  price_mult: 8, difficulty_bump: 3, rarity: 0.03 },
    // mineral variants: one per mined ore so the ore-as-bait system has
    // a matching pool target for every ore the player can chip out.
    Variant { prefix: "Copper",    pool: "mineral",  price_mult: 2, difficulty_bump: 0, rarity: 0.15 },
    Variant { prefix: "Iron",      pool: "mineral",  price_mult: 2, difficulty_bump: 1, rarity: 0.13 },
    Variant { prefix: "Turquoise", pool: "mineral",  price_mult: 2, difficulty_bump: 1, rarity: 0.12 },
    Variant { prefix: "Silver",    pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Amethyst",  pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Gold",      pool: "mineral",  price_mult: 4, difficulty_bump: 2, rarity: 0.08 },
    Variant { prefix: "Sapphire",  pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Ruby",      pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Topaz",     pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Opal",      pool: "mineral",  price_mult: 4, difficulty_bump: 2, rarity: 0.07 },
    Variant { prefix: "Emerald",   pool: "mineral",  price_mult: 3, difficulty_bump: 1, rarity: 0.10 },
    Variant { prefix: "Onyx",      pool: "mineral",  price_mult: 4, difficulty_bump: 2, rarity: 0.07 },
    Variant { prefix: "Diamond",   pool: "mineral",  price_mult: 7, difficulty_bump: 4, rarity: 0.01 },
];

/// Base species that get the full variant treatment. Keep this list to
/// iconic / commonly-encountered fish so the loot pools have meaningful
/// variety without bloating the fishdex beyond reason.
const BASE_SPECIES: &[&str] = &[
    "coelacanth",
    "goldfish",
    "bluefin tuna",
    "great white shark",
    "swordfish",
    "mahi-mahi",
    "arctic char",
    "giant pacific octopus",
    "electric eel",
    "anglerfish",
    "northern pike",
    "common carp",
    "rainbow trout",
    "largemouth bass",
    // Expansion round — popular real-world picks + a couple iconic outliers
    "bluegill",
    "yellow perch",
    "chinook salmon",
    "coho salmon",
    "sockeye salmon",
    "channel catfish",
    "lake trout",
    "brown trout",
    "brook trout",
    "smallmouth bass",
    "white sturgeon",
    "lake sturgeon",
    "arapaima",
    "peacock bass",
    "tigerfish",
    "barracuda",
    "tarpon",
    "marlin",
    "snapper",
    "grouper",
    "lobster",
    "blue crab",
    "lamprey",
    "moray eel",
    "lake whitefish",
    "humpback whale",
    "orca",
    "giant squid",
];

fn generate_variants(base: &[FishDef]) -> Vec<FishDef> {
    let mut out = Vec::new();
    for &name in BASE_SPECIES {
        let Some(b) = base.iter().find(|f| f.name == name) else {
            continue;
        };
        for v in VARIANTS {
            let mut f = b.clone();
            // Variant naming: "<base name> (<Variant>)". The base
            // description is preserved verbatim — the variant is a cosmetic
            // / stat overlay, not a different species, so it shouldn't
            // overwrite the lore the user wrote for the base fish.
            f.name = format!("{} ({})", b.name, v.prefix);
            f.rarity = v.rarity;
            f.difficulty = b.difficulty.saturating_add(v.difficulty_bump).min(10);
            let base_price = if b.price > 0 {
                b.price
            } else {
                let d = b.difficulty as u64;
                10 + d * d * 4
            };
            f.price = base_price.saturating_mul(v.price_mult);
            f.pool = vec![v.pool.to_string()];
            f.biomes = Vec::new();
            f.waters = Vec::new();
            f.effect = None;
            f.joke = false;
            f.unique = false;
            // Attach a per-variant pool tag so ore/elemental baits can
            // call out a specific subset of the pool. Format: "ore_ruby",
            // "elem_astral", etc. The ore-as-bait synth keys off these.
            let tag = format!("ore_{}", v.prefix.to_ascii_lowercase());
            f.pool_tags = vec![tag];
            out.push(f);
        }
    }
    out
}
