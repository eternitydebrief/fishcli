use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct FishDef {
    pub name: String,
    pub description: String,
    pub rarity: f32,
    pub difficulty: u8,
    /// Biomes where this fish can appear. Empty = any biome.
    pub biomes: Vec<String>,
    /// Water types where this fish can appear: "ocean", "lake", "pond",
    /// "puddle", "well". Empty = any water.
    pub waters: Vec<String>,
    /// Optional flat sell price override; 0 = computed from difficulty.
    pub price: u64,
    /// Optional permanent-buff effect on catch:
    ///   "price_mult:0.10"  - +10% to all future sell prices
    ///   "free_rod"         - next rod tier is free
    ///   "fishing_xp:500"   - instant fishing xp boost
    ///   "bobber_range:1"   - permanent +1 max cast distance
    pub effect: Option<String>,
    /// If true, this is a joke pickup (boot, tire, etc.) not a real fish.
    pub joke: bool,
    /// If true, this entry is one-of-a-kind: it goes in the Misc inventory
    /// tab (never the Fish tab), cannot be sold, cannot be discarded, and
    /// catching it twice has no effect.
    pub unique: bool,
    /// Loot-pool tags. Empty = catchable through normal biome/water fishing.
    /// Non-empty = ONLY reachable when the player has The Rod equipped and
    /// selects this pool from the loot-pool menu. Pool names: "cosmic",
    /// "divine", "mineral", "forest", "desert", "tundra", "swamp", etc.
    pub pool: Vec<String>,
    /// Weather where this fish is more likely to bite. Empty = no
    /// preference. Names match the weather enum: "Clear", "Rain", "Snow",
    /// "Blizzard", "Sandstorm", "Scorching", "Fog", "Windy",
    /// "Thunderstorm", "Heat Wave", "Cloudy".
    pub preferred_weather: Vec<String>,
    /// Free-form pool tags this species belongs to for bait pool_pull
    /// matching. Empty = use the auto-derived defaults from biomes/waters
    /// (see `effective_pool_tags`). Override per-fish in `assets/fish.json`
    /// when you want a specific bait to target a specific species set.
    pub pool_tags: Vec<String>,
    /// True for ancient species pulled out of the mines as fossils. These
    /// don't go into the regular fish basket (they're stored separately in
    /// `Player::fossils`), don't sell, and only become valuable after an
    /// archeologist unearths them into their living counterpart.
    pub fossilized: bool,
    /// True for the *living* counterpart of a fossilized species. Only
    /// added to inventory via the archeologist's unearth action; never
    /// picked by `pick_fish_full` because of the synthetic
    /// `unearthed-private` pool tag carried in `pool`.
    pub unearthed: bool,
    /// Slug linking a fossil to its living variant (and vice-versa). The
    /// archeologist uses `fossil_slug` on a fossil to find which species
    /// to grant. Empty for non-fossil entries.
    pub fossil_slug: String,
}

impl Default for FishDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            rarity: 0.0,
            difficulty: 1,
            biomes: Vec::new(),
            waters: Vec::new(),
            price: 0,
            effect: None,
            joke: false,
            unique: false,
            pool: Vec::new(),
            preferred_weather: Vec::new(),
            pool_tags: Vec::new(),
            fossilized: false,
            unearthed: false,
            fossil_slug: String::new(),
        }
    }
}

impl FishDef {
    fn t(&self) -> f32 {
        ((self.difficulty as f32 - 1.0) / 9.0).clamp(0.0, 1.0)
    }

    // Difficulty-driven minigame stats. Full ramp is `t = 0..1` for
    // difficulty 1..10.
    //
    // RECT BALANCE INVARIANT: the *effective* rect (base + skill-tree
    // rect_h_bonus, max +2 with Iron Arms + Wide Net fully ranked) is
    // designed to land in the 4-5 cell band across the player's whole
    // progression. Out-of-level encounters drop to 3 cells occasionally
    // (rare, intended), but 2 cells is never a target — that floor is
    // enforced by capping the base at 3 even at diff 10.
    //
    // The base ramp here goes 5 → 3 across diff 1 → 10. The skill
    // tree's max +2 bonus is sized to exactly restore diff-10 fish to
    // the 5-cell target band, and the min_fishing_level gating ensures
    // a player only sees diff-N fish around the time their rect_h_bonus
    // has caught up enough to keep the effective window at ~5.
    pub fn rect_h(&self) -> f32 {
        // diff 1: 5 cells (25% of the bar)
        // diff 10: 3 cells (15% of the bar)
        5.0 - self.t() * 2.0
    }

    pub fn fish_speed(&self) -> f32 {
        // diff 1: 0.45, diff 10: 1.05 — every fish actively dodges.
        0.45 + self.t() * 0.60
    }

    pub fn target_change_ticks(&self) -> u32 {
        // diff 1: ~32 (1.6s between flicks), diff 10: ~12 (0.6s)
        32 - (self.t() * 20.0) as u32
    }

    pub fn sell_price(&self) -> u64 {
        if self.price > 0 {
            return self.price;
        }
        // Steeper curve: base 10 + difficulty^2.5 * 4 — keeps low-diff
        // common fish around 14-25 valu, ramps hard for tier-10 prey
        // (≈ 1275 valu base). Combined with the fishing-level / min-level
        // decay applied at sale time, this means catching low-tier fish
        // in the late game pays much less per minute than working harder
        // species — preventing "fish carp forever" as a strategy.
        let d = self.difficulty as f32;
        (10.0 + d.powf(2.5) * 4.0) as u64
    }

    /// Auto-derived gate: minimum fishing level required for this species
    /// to appear in the picker. Beginners catch carp & bluegill; the rare
    /// difficulty-10 fish need lvl 180 to even hook.
    pub fn min_fishing_level(&self) -> u32 {
        match self.difficulty {
            0 | 1 => 1,
            2 => 3,
            3 => 8,
            4 => 15,
            5 => 25,
            6 => 40,
            7 => 60,
            8 => 90,
            9 => 130,
            _ => 180, // 10 and above
        }
    }

    /// Auto-derived gate: minimum *rod tier* the player must own for this
    /// species to appear. Prevents skipping into a dim and immediately
    /// scooping up high-value fish without the rod to back it up.
    pub fn min_rod_tier(&self) -> u32 {
        match self.difficulty {
            0 | 1 => 1,
            2 => 2,
            3 => 5,
            4 => 10,
            5 => 20,
            6 => 35,
            7 => 60,
            8 => 90,
            9 => 130,
            _ => 180,
        }
    }

    /// Effective pool tags for bait `pool_pull` matching. Uses the explicit
    /// `pool_tags` field if non-empty, otherwise auto-derives from the
    /// fish's biomes + waters using a fixed table. Tag names line up with
    /// the bug pool_pull values in `assets/bugs.json`.
    pub fn effective_pool_tags(&self) -> Vec<&'static str> {
        if !self.pool_tags.is_empty() {
            return self
                .pool_tags
                .iter()
                .filter_map(|s| auto_tag_name(s.as_str()))
                .collect();
        }
        let mut out: Vec<&'static str> = Vec::new();
        let has_water = |w: &str| self.waters.iter().any(|s| s.eq_ignore_ascii_case(w));
        let has_biome = |b: &str| self.biomes.iter().any(|s| s.eq_ignore_ascii_case(b));
        if has_water("ocean") {
            out.push("saltwater");
        }
        if has_water("well") {
            out.push("cavern");
        }
        let freshwater = self.waters.is_empty()
            || has_water("lake")
            || has_water("pond")
            || has_water("puddle");
        if freshwater {
            if has_biome("Meadow") || self.biomes.is_empty() {
                out.push("freshwater_surface");
            }
            if has_biome("Forest") {
                out.push("freshwater_shaded");
            }
            if has_biome("Rocky") {
                out.push("river");
            }
            if has_biome("Scrub") {
                out.push("warm_water");
            }
            if has_biome("Desert") {
                out.push("oasis");
            }
            if has_biome("Tundra") {
                out.push("cold_water");
            }
            if has_biome("Swamp") {
                out.push("swamp");
            }
        }
        out
    }

    pub fn matches(&self, biome: &str, water: &str) -> bool {
        let biome_ok = self.biomes.is_empty()
            || self
                .biomes
                .iter()
                .any(|b| b.eq_ignore_ascii_case(biome) || b == "any");
        let water_ok = self.waters.is_empty()
            || self
                .waters
                .iter()
                .any(|w| w.eq_ignore_ascii_case(water) || w == "any");
        biome_ok && water_ok
    }
}

/// Pick a fish honouring an optional pool override. When `pool` is `Some`,
/// only fish whose `pool` tag list contains that name are eligible. When it
/// is `None`, the normal biome/water filter is used and fish carrying any
/// pool tag are excluded — variant fish (cosmic/divine/mineral) only
/// appear when their pool is explicitly chosen. When `rare_boost` is true,
/// fish with very low rarity (< 0.01) get a 10x weight multiplier — used
/// for Dusk/Midnight windows. `weather` (if provided) applies a 3x weight
/// boost to fish that list it in `preferred_weather`. `catches` is the
/// player's lifetime catch count; for the first 100 catches, easier fish
/// (difficulty 1-3) get a sharp weight boost so beginners aren't drowned
/// in rare/impossible fish.
pub fn pick_fish_full<'a>(
    rng: &mut u32,
    fish: &'a [FishDef],
    biome: &str,
    water: &str,
    pool: Option<&str>,
    rare_boost: bool,
    weather: Option<&str>,
    catches: u64,
    fishing_level: u32,
    rod_tier: u32,
    ocean_depth: u32,
    bait_pool_pull: Option<(&str, f32)>,
    force_no_trash: bool,
) -> &'a FishDef {
    let gated = |f: &FishDef| {
        // Unique / pool fish ignore level+rod gates so The Rod's pool
        // override and the pantheon fish remain reachable on their own
        // criteria; everything else has to clear both bars.
        f.unique
            || !f.pool.is_empty()
            || (fishing_level >= f.min_fishing_level() && rod_tier >= f.min_rod_tier())
    };
    let eligible: Vec<&'a FishDef> = if let Some(p) = pool {
        fish.iter()
            .filter(|f| f.pool.iter().any(|tag| tag.eq_ignore_ascii_case(p)))
            .collect()
    } else {
        fish.iter()
            .filter(|f| f.pool.is_empty() && f.matches(biome, water) && gated(f))
            .collect()
    };
    let pool_vec = if eligible.is_empty() {
        // Bottom-fallback: ungated common fish so the player always hooks
        // *something* even on a wrong-biome cast at low level.
        fish.iter()
            .filter(|f| f.pool.is_empty() && f.difficulty <= 2)
            .collect::<Vec<_>>()
    } else {
        eligible
    };
    // Early-game ramp: at 0 catches, factor = 1.0; at 100, factor = 0.0.
    let early_factor = (1.0 - (catches as f32) / 100.0).clamp(0.0, 1.0);
    let weight_of = |f: &FishDef| -> f32 {
        let mut w = f.rarity;
        if rare_boost && f.rarity > 0.0 && f.rarity < 0.01 {
            w *= 10.0;
        }
        if let Some(weather_name) = weather {
            if f.preferred_weather
                .iter()
                .any(|s| s.eq_ignore_ascii_case(weather_name))
            {
                w *= 3.0;
            }
        }
        // Easy-fish boost: difficulty 1 → 6x at 0 catches, decaying to 1x.
        // Difficulty 2 → 4x decaying. Difficulty 3 → 2x decaying. >=4 → no
        // boost. So newbies catch carp/bluegill/sunfish instead of leviathans.
        let ease_boost = match f.difficulty {
            1 => 1.0 + early_factor * 5.0,
            2 => 1.0 + early_factor * 3.0,
            3 => 1.0 + early_factor * 1.0,
            _ => 1.0,
        };
        // Offshore boost: the further from shore on the ocean, the rarer
        // and meaner the bite. Only non-zero when caller passes a depth
        // (i.e. ocean cast). Common difficulty 1-3 fish and junk get
        // suppressed; rare fish (rarity < 0.1) get amplified; junk gets
        // hammered extra hard since nobody dumps a soda can 20 tiles out.
        let depth_mult = if ocean_depth > 0 {
            let d = ocean_depth as f32;
            let mut m = 1.0;
            if f.joke {
                m *= 1.0 / (1.0 + d * 0.4);
            }
            if f.difficulty <= 3 {
                m *= 1.0 / (1.0 + d * 0.12);
            }
            if f.rarity > 0.0 && f.rarity < 0.1 {
                m *= 1.0 + d * 0.18;
            }
            m
        } else {
            1.0
        };
        let bait_pull_mult = match bait_pool_pull {
            Some((tag, mult)) if mult > 0.0 => {
                if f.effective_pool_tags()
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(tag))
                {
                    1.0 + mult
                } else {
                    1.0
                }
            }
            _ => 1.0,
        };
        w * ease_boost * depth_mult * bait_pull_mult
    };
    // Trash gate. Junk fish (boots, cans, soda bottles) now live in a
    // separate pre-roll sub-pool instead of competing on weight with real
    // fish — gives an explicit, dial-able trash chance. Override pools
    // (cosmic / divine / pyramid / etc.) skip this: their fish list is
    // already curated and joke items aren't tagged into them.
    //
    // Curve: 33% at zero stats, decaying linearly to 1% once
    // (fishing_level + rod_tier) crosses 200. Tracks the rest of the
    // progression gates (rod tier 200 = late endgame).
    let candidates: Vec<&'a FishDef> = if pool.is_none() {
        let trash: Vec<&'a FishDef> = pool_vec
            .iter()
            .copied()
            .filter(|f| f.joke && !f.unique)
            .collect();
        let real: Vec<&'a FishDef> = pool_vec
            .iter()
            .copied()
            .filter(|f| !f.joke || f.unique)
            .collect();
        let progress = ((fishing_level + rod_tier) as f32 / 200.0).clamp(0.0, 1.0);
        let trash_chance = if force_no_trash {
            0.0
        } else {
            0.33 - 0.32 * progress
        };
        let want_trash = next_rand_f32(rng) < trash_chance;
        match (want_trash, trash.is_empty(), real.is_empty()) {
            (true, false, _) => trash,
            (false, _, false) => real,
            // Requested side is empty → use whatever the other side has.
            (true, true, false) => real,
            (false, _, true) => trash,
            _ => pool_vec.clone(),
        }
    } else {
        pool_vec.clone()
    };
    let total: f32 = candidates.iter().map(|f| weight_of(f)).sum();
    if total <= 0.0 || candidates.is_empty() {
        return &fish[0];
    }
    let r = next_rand_f32(rng) * total;
    let mut acc = 0.0;
    for f in &candidates {
        acc += weight_of(f);
        if r <= acc {
            return f;
        }
    }
    candidates[candidates.len() - 1]
}

/// Pass-through alias for explicit pool_tags entries. Returns a static slice
/// matching the input if it's one of the known tags; falls back to None for
/// unrecognized strings so typos in fish.json don't silently mis-pull.
fn auto_tag_name(s: &str) -> Option<&'static str> {
    match s.to_ascii_lowercase().as_str() {
        "freshwater_surface" => Some("freshwater_surface"),
        "freshwater_shaded" => Some("freshwater_shaded"),
        "river" => Some("river"),
        "warm_water" => Some("warm_water"),
        "oasis" => Some("oasis"),
        "cold_water" => Some("cold_water"),
        "swamp" => Some("swamp"),
        "saltwater" => Some("saltwater"),
        "cavern" => Some("cavern"),
        "lakebed" => Some("lakebed"),
        "cathedral" => Some("cathedral"),
        "abyssal" => Some("abyssal"),
        "infernal" => Some("infernal"),
        "divine" => Some("divine"),
        _ => None,
    }
}

pub fn next_rand_f32(s: &mut u32) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    (x as f32) / (u32::MAX as f32)
}
