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
        }
    }
}

impl FishDef {
    fn t(&self) -> f32 {
        ((self.difficulty as f32 - 1.0) / 9.0).clamp(0.0, 1.0)
    }

    pub fn rect_h(&self) -> f32 {
        7.0 - self.t() * 4.0
    }

    pub fn fish_speed(&self) -> f32 {
        0.25 + self.t() * 0.55
    }

    pub fn target_change_ticks(&self) -> u32 {
        50 - (self.t() * 30.0) as u32
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
        w * ease_boost
    };
    let total: f32 = pool_vec.iter().map(|f| weight_of(f)).sum();
    if total <= 0.0 || pool_vec.is_empty() {
        return &fish[0];
    }
    let r = next_rand_f32(rng) * total;
    let mut acc = 0.0;
    for f in &pool_vec {
        acc += weight_of(f);
        if r <= acc {
            return f;
        }
    }
    pool_vec[pool_vec.len() - 1]
}

pub fn next_rand_f32(s: &mut u32) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    (x as f32) / (u32::MAX as f32)
}
