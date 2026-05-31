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
        let d = self.difficulty as u64;
        10 + d * d * 4
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

pub fn pick_fish<'a>(
    rng: &mut u32,
    fish: &'a [FishDef],
    biome: &str,
    water: &str,
) -> &'a FishDef {
    pick_fish_with_pool(rng, fish, biome, water, None)
}

/// Pick a fish honouring an optional pool override. When `pool` is `Some`,
/// only fish whose `pool` tag list contains that name are eligible. When it
/// is `None`, the normal biome/water filter is used and fish carrying any
/// pool tag are excluded — variant fish (cosmic/divine/mineral) only
/// appear when their pool is explicitly chosen.
pub fn pick_fish_with_pool<'a>(
    rng: &mut u32,
    fish: &'a [FishDef],
    biome: &str,
    water: &str,
    pool: Option<&str>,
) -> &'a FishDef {
    pick_fish_weighted(rng, fish, biome, water, pool, false)
}

/// Like `pick_fish_with_pool`, but if `rare_boost` is true, fish with very
/// low rarity (< 0.01) get a 10x multiplier applied to their weight — used
/// for the Dusk and Midnight time-of-day windows where rare fish surface.
pub fn pick_fish_weighted<'a>(
    rng: &mut u32,
    fish: &'a [FishDef],
    biome: &str,
    water: &str,
    pool: Option<&str>,
    rare_boost: bool,
) -> &'a FishDef {
    pick_fish_full(rng, fish, biome, water, pool, rare_boost, None)
}

/// Most general pick. `preferred_weather` (if provided) applies a 3x
/// weight boost to fish that list it in `preferred_weather`.
pub fn pick_fish_full<'a>(
    rng: &mut u32,
    fish: &'a [FishDef],
    biome: &str,
    water: &str,
    pool: Option<&str>,
    rare_boost: bool,
    weather: Option<&str>,
) -> &'a FishDef {
    let eligible: Vec<&'a FishDef> = if let Some(p) = pool {
        fish.iter()
            .filter(|f| f.pool.iter().any(|tag| tag.eq_ignore_ascii_case(p)))
            .collect()
    } else {
        fish.iter()
            .filter(|f| f.pool.is_empty() && f.matches(biome, water))
            .collect()
    };
    let pool_vec = if eligible.is_empty() {
        fish.iter().collect::<Vec<_>>()
    } else {
        eligible
    };
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
        w
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
