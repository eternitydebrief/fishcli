use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use std::cell::RefCell;

// Direct-mapped, per-thread caches. ~10ns per lookup vs ~300ns for the
// HashMap path we used to use. We checked: for a wide terminal a frame
// makes ~50k cache calls, so the HashMap version was burning ~15ms/frame
// (30% of the 50ms 20fps budget) on cache overhead alone. Direct-mapped
// arrays drop that to ~0.5ms.
//
// Slot is (packed_xy, value). MISS = recompute and overwrite (no eviction
// chain, just a one-cell ring). Memory: 2^17 × 16 bytes ≈ 2 MB per thread
// per cache, times rayon worker count and 2 caches = a few MB total.
// Acceptable; the speedup is worth it.
const CACHE_BITS: u32 = 17;
const CACHE_SIZE: usize = 1 << CACHE_BITS;
const CACHE_MASK: usize = CACHE_SIZE - 1;

#[derive(Clone, Copy)]
struct BiomeSlot {
    key: u64,
    biome: Biome,
}

#[derive(Clone, Copy)]
struct WaterSlot {
    key: u64,
    info: CellWaterInfo,
}

// "no entry" sentinel: u64::MAX as packed key. compute_packed never
// produces it for any real (i32, i32).
const EMPTY_KEY: u64 = u64::MAX;

thread_local! {
    static BIOME_CACHE: RefCell<Vec<BiomeSlot>> = RefCell::new(
        vec![BiomeSlot { key: EMPTY_KEY, biome: Biome::Meadow }; CACHE_SIZE]
    );
    static WATER_CACHE: RefCell<Vec<WaterSlot>> = RefCell::new(
        vec![WaterSlot { key: EMPTY_KEY, info: CellWaterInfo {
            in_water: false, island_grass: false, island_sand: false,
            in_ring: false, in_shore: false,
        } }; CACHE_SIZE]
    );
}

#[inline(always)]
fn pack_xy(x: i32, y: i32) -> u64 {
    ((x as u32 as u64) << 32) | (y as u32 as u64)
}

#[inline(always)]
fn cache_index(packed: u64) -> usize {
    // mix bits then mask
    let mut h = packed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (h as usize) & CACHE_MASK
}

#[derive(Clone, Copy, Default)]
struct CellWaterInfo {
    in_water: bool,
    island_grass: bool,
    island_sand: bool,
    in_ring: bool,
    /// water cells very close to the inside edge of the ellipse - trees may
    /// anchor here so their roots stand at the shoreline
    in_shore: bool,
}

fn cached_biome_at(x: i32, y: i32, seed: u32) -> Biome {
    let key = pack_xy(x, y);
    let idx = cache_index(key);
    BIOME_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let slot = &c[idx];
        if slot.key == key {
            return slot.biome;
        }
        let biome = biome_at(x, y, seed);
        c[idx] = BiomeSlot { key, biome };
        biome
    })
}

fn cached_water_info(x: i32, y: i32, seed: u32) -> CellWaterInfo {
    let key = pack_xy(x, y);
    let idx = cache_index(key);
    WATER_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let slot = &c[idx];
        if slot.key == key {
            return slot.info;
        }
        let info = compute_water_info(x, y, seed);
        c[idx] = WaterSlot { key, info };
        info
    })
}

fn cached_water_body_at(x: i32, y: i32, seed: u32) -> bool {
    cached_water_info(x, y, seed).in_water
}

/// Which plane of existence the player currently inhabits.
/// Surface = Sentinel (the dry land + ocean). Mines = caverns under Sentinel.
/// Atlantis = the underwater plane where the fish civilizations live.
/// Same (x, y) maps to the same procedural cell in every dimension, so a
/// mine entrance at (10, -3) on Surface drops you onto (10, -3) in Mines.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Dimension {
    #[default]
    Surface,
    Mines,
    Atlantis,
    /// Hellish reflection of the Mines, reached after 100 well casts. Lava
    /// instead of water; only infernal-variant fish bite.
    Inferno,
    // ---- procedurally generated specialty dims ----
    Sewer,
    HotSpring,
    Pyramid,
    SwampCave,
    BogCathedral,
    MirrorLake,
    Iceshelf,
    Wreckage,
    Crater,
    Colosseum,
    AllBlue,
    /// Flooded subterranean cavern reached via an A-frame entrance on a
    /// lake island. Eyeless, stygobitic fauna; only the lakebed fish pool
    /// rolls here.
    Lakebed,
}

impl Dimension {
    /// Rod tier required to enter this dim via the `:travel` command.
    /// Surface/Mines/Atlantis/Inferno keep their existing gates (handled
    /// elsewhere); the new dims slot into the rod-tier curve.
    pub fn min_rod_tier(self) -> u32 {
        match self {
            Dimension::Surface => 0,
            Dimension::Sewer => 1,
            Dimension::HotSpring => 5,
            Dimension::Mines => 3,
            Dimension::Pyramid => 15,
            Dimension::SwampCave => 20,
            Dimension::Lakebed => 25,
            Dimension::Wreckage => 30,
            Dimension::BogCathedral => 40,
            Dimension::Atlantis => 50,
            Dimension::MirrorLake => 60,
            Dimension::Iceshelf => 75,
            Dimension::Inferno => 75,
            Dimension::Colosseum => 90,
            Dimension::Crater => 130,
            Dimension::AllBlue => 180,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dimension::Surface => "Surface",
            Dimension::Mines => "Mines",
            Dimension::Atlantis => "Atlantis",
            Dimension::Inferno => "Inferno",
            Dimension::Sewer => "Sewer",
            Dimension::HotSpring => "Hot Spring",
            Dimension::Pyramid => "Pyramid",
            Dimension::SwampCave => "Swamp Cave",
            Dimension::BogCathedral => "Bog Cathedral",
            Dimension::MirrorLake => "Mirror Lake",
            Dimension::Iceshelf => "Iceshelf",
            Dimension::Wreckage => "Wreckage",
            Dimension::Crater => "Crater",
            Dimension::Colosseum => "Colosseum",
            Dimension::AllBlue => "All Blue",
            Dimension::Lakebed => "Lakebed Caves",
        }
    }

    /// Match a user-typed dim name (case-insensitive, allows spaces or
    /// dashes). Returns None for "Surface" or unknown.
    pub fn from_name(s: &str) -> Option<Dimension> {
        let n = s.trim().to_lowercase().replace(['-', '_'], " ");
        Some(match n.as_str() {
            "mines" => Dimension::Mines,
            "atlantis" => Dimension::Atlantis,
            "inferno" => Dimension::Inferno,
            "sewer" => Dimension::Sewer,
            "hot spring" | "hotspring" => Dimension::HotSpring,
            "pyramid" => Dimension::Pyramid,
            "swamp cave" | "swampcave" => Dimension::SwampCave,
            "bog cathedral" | "bogcathedral" | "cathedral" => Dimension::BogCathedral,
            "mirror lake" | "mirrorlake" | "mirror" => Dimension::MirrorLake,
            "iceshelf" | "ice shelf" => Dimension::Iceshelf,
            "wreckage" | "wreck" => Dimension::Wreckage,
            "crater" => Dimension::Crater,
            "colosseum" | "coliseum" => Dimension::Colosseum,
            "all blue" | "allblue" | "deep" => Dimension::AllBlue,
            "lakebed" | "lakebed caves" | "lakebed cave" => Dimension::Lakebed,
            "surface" => Dimension::Surface,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Meadow,
    Forest,
    Rocky,
    Scrub,
    Desert,
    Tundra,
    Swamp,
}

impl Biome {
    pub fn label(self) -> &'static str {
        match self {
            Biome::Meadow => "Meadow",
            Biome::Forest => "Forest",
            Biome::Rocky => "Rocky Plain",
            Biome::Scrub => "Scrubland",
            Biome::Desert => "Desert",
            Biome::Tundra => "Tundra",
            Biome::Swamp => "Swamp",
        }
    }
}

#[allow(dead_code)] // puddle_bonus is a planned hook for swamp puddles
struct BiomeParams {
    tree: f32,
    big_rock: f32,
    medium_rock: f32,
    rock: f32,
    pebble: f32,
    flower: f32,
    cactus: f32,
    puddle_bonus: f32,
}

fn biome_params(b: Biome) -> BiomeParams {
    match b {
        Biome::Meadow => BiomeParams {
            tree: 0.025, big_rock: 0.0, medium_rock: 0.0, rock: 0.0015,
            pebble: 0.040, flower: 0.012, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Forest => BiomeParams {
            tree: 0.090, big_rock: 0.0, medium_rock: 0.0, rock: 0.0010,
            pebble: 0.020, flower: 0.003, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Rocky => BiomeParams {
            tree: 0.008, big_rock: 0.0, medium_rock: 0.0, rock: 0.022,
            pebble: 0.120, flower: 0.001, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Scrub => BiomeParams {
            tree: 0.005, big_rock: 0.0, medium_rock: 0.0, rock: 0.0010,
            pebble: 0.020, flower: 0.002, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Desert => BiomeParams {
            tree: 0.0, big_rock: 0.0, medium_rock: 0.0, rock: 0.0070,
            pebble: 0.110, flower: 0.0, cactus: 0.012, puddle_bonus: 0.0,
        },
        Biome::Tundra => BiomeParams {
            tree: 0.012, big_rock: 0.0, medium_rock: 0.0, rock: 0.0070,
            pebble: 0.080, flower: 0.001, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Swamp => BiomeParams {
            tree: 0.050, big_rock: 0.0, medium_rock: 0.0, rock: 0.0005,
            pebble: 0.015, flower: 0.006, cactus: 0.0, puddle_bonus: 0.18,
        },
    }
}

pub fn biome_at(x: i32, y: i32, seed: u32) -> Biome {
    // Procedural villages always force their own provenance biome over
    // their footprint — so a desert town is desert throughout, never a
    // meadow patch in the middle.
    if let Some(b) = village_biome_override(x, y, seed) {
        return b;
    }
    // Frequencies halved → biomes roughly 2x larger in each dimension
    // (~4x the area). Player crosses fewer boundaries per session.
    let fx = x as f32 * 0.022;
    let fy = y as f32 * 0.028;
    let s = (seed as f32) * 0.00007;

    let warp_x = (fx * 0.42 + fy * 0.31 + s).sin() * 3.5;
    let warp_y = (fx * 0.33 - fy * 0.47 + s * 1.3).sin() * 3.5;
    let wx = fx + warp_x;
    let wy = fy + warp_y;

    let temp = (wx * 0.18 + wy * 0.07 + s).sin();
    let moist = (wx * 0.13 - wy * 0.21 + s * 1.7).sin();
    let veg = (wx * 0.08 + wy * 0.06 - s * 0.9).sin();

    if temp > 0.55 && moist < -0.1 {
        Biome::Desert
    } else if temp < -0.55 {
        Biome::Tundra
    } else if moist > 0.55 {
        Biome::Swamp
    } else if veg > 0.45 {
        Biome::Forest
    } else if moist < -0.3 && veg < 0.0 {
        Biome::Scrub
    } else if veg < -0.4 {
        Biome::Rocky
    } else {
        Biome::Meadow
    }
}

/// If (x, y) is inside a procedural village's footprint, force the biome
/// to that village's provenance biome (computed from the village's anchor).
/// Returns None for the origin Home Village (which sits at the seed) and
/// for cells outside any village footprint.
fn village_biome_override(x: i32, y: i32, seed: u32) -> Option<Biome> {
    let v = village_anchor_for(x, y, seed)?;
    // Sample biome at the village anchor (using the no-recursion variant)
    // so the whole village footprint shares its anchor's provenance biome.
    Some(biome_at_noise(v.ax, v.ay, seed))
}

/// Same as `biome_at` but does NOT consult the village override. Used by
/// village placement to pick a village's provenance biome.
pub fn biome_at_noise(x: i32, y: i32, seed: u32) -> Biome {
    let fx = x as f32 * 0.022;
    let fy = y as f32 * 0.028;
    let s = (seed as f32) * 0.00007;
    let warp_x = (fx * 0.42 + fy * 0.31 + s).sin() * 3.5;
    let warp_y = (fx * 0.33 - fy * 0.47 + s * 1.3).sin() * 3.5;
    let wx = fx + warp_x;
    let wy = fy + warp_y;
    let temp = (wx * 0.18 + wy * 0.07 + s).sin();
    let moist = (wx * 0.13 - wy * 0.21 + s * 1.7).sin();
    let veg = (wx * 0.08 + wy * 0.06 - s * 0.9).sin();
    if temp > 0.55 && moist < -0.1 {
        Biome::Desert
    } else if temp < -0.55 {
        Biome::Tundra
    } else if moist > 0.55 {
        Biome::Swamp
    } else if veg > 0.45 {
        Biome::Forest
    } else if moist < -0.3 && veg < 0.0 {
        Biome::Scrub
    } else if veg < -0.4 {
        Biome::Rocky
    } else {
        Biome::Meadow
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Grass,
    Wall,
    Roof,
    DoorRod,
    DoorSchool,
    /// Plain dwelling door — leads to a procedural one-room interior keyed
    /// on the door's world position.
    DoorHouse,
    /// Specialty dim portal (sparse). The destination is recomputed from
    /// the cell's (x, y, seed) hash at interact time — no extra state.
    DimPortal,
    /// Decorative stone arch wrapped around a DimPortal anchor (5w x 4h
    /// gateway). Non-walkable; the player approaches the anchor cell from
    /// the south and presses `f` to travel. Glyph + tint vary with the
    /// destination dim.
    PortalFrame,
    Water,
    Dock,
    Sand,
    TreeTrunk,
    TreeCanopy,
    Rock,
    MediumRock,
    BigRock,
    Pebble,
    Flower,
    Cactus,
    Well,
    Path,
    Lamppost,
    Bench,
    // --- mine entrances on the surface ---
    /// the wooden A-frame entrance you interact with (`f`) to descend
    MineEntrance,
    /// 3-wide, 2-tall structural frame around a MineEntrance (visual only,
    /// blocks movement). Glyph picked by render based on offset from anchor.
    MineFrame,
    // --- mines plane ---
    CaveFloor,
    CaveWall,
    Stalactite,
    Stalagmite,
    OreRock,
    /// an underground pond/lake inside the mines. fishing here yields the
    /// mineral fish pool (when wired up).
    MineralWater,
    /// the way back to the surface from inside the mines, placed at the
    /// same coord as the surface MineEntrance that brought you down.
    MineExit,
    /// red-hot inferno wall (charred basalt)
    InfernoWall,
    /// inferno floor (cracked, hot)
    InfernoFloor,
    /// lava pool — fishable like MineralWater but in the inferno dim
    Lava,
    /// generic landmark wall (Atlantean castle, Crypt, Fallen Fish's keep).
    /// Coloring is dim-specific in render_tile.
    LandmarkWall,
    /// generic landmark door — walkable, no scene transition.
    LandmarkDoor,
    /// tombstone (mines crypt)
    Tombstone,
    /// blacksmith's smelter — non-walkable. `f` opens the smelt minigame.
    /// Spawned next to every Blacksmith NPC (static + procedural).
    Smelter,
    /// blacksmith's forge — non-walkable. `f` opens the forge minigame.
    /// Spawned next to every Blacksmith NPC (static + procedural).
    Forge,
    /// the Chef's cooking pot — non-walkable. `f` opens the cookbook menu
    /// directly so cooking has a discoverable physical home next to the
    /// chef NPC.
    CookingPot,
    /// Sparse inspectable curio. The specific curio (id + glyph + color)
    /// is derived from `curio_at(x, y, dim, seed)` so the same cell always
    /// hosts the same object. Non-walkable — players stand adjacent and
    /// use `x` while facing it to read its description. Lore lives in
    /// `assets/inspect.json` under `curio:<id>` keys.
    Curio,
    // --- atlantis plane ---
    Seabed,
    /// trunk-equivalent for coral; pairs with CoralCanopy in a 4-cell shape
    CoralTrunk,
    CoralCanopy,
    Kelp,
    /// the dark deep-water background tiles you walk on in atlantis
    DeepWater,
    Anemone,
}

impl Tile {
    pub fn walkable(self) -> bool {
        matches!(
            self,
            Tile::Grass
                | Tile::Sand
                | Tile::Pebble
                | Tile::Flower
                | Tile::Path
                | Tile::Dock
                | Tile::CaveFloor
                | Tile::Seabed
                | Tile::DeepWater
                | Tile::Kelp
                | Tile::InfernoFloor
                | Tile::LandmarkDoor
        )
    }

    pub fn id_str(self) -> &'static str {
        match self {
            Tile::Grass => "Grass",
            Tile::Wall => "Wall",
            Tile::Roof => "Roof",
            Tile::DoorRod => "DoorRod",
            Tile::DoorSchool => "DoorSchool",
            Tile::DoorHouse => "DoorHouse",
            Tile::Water => "Water",
            Tile::Dock => "Dock",
            Tile::Sand => "Sand",
            Tile::TreeTrunk => "TreeTrunk",
            Tile::TreeCanopy => "TreeCanopy",
            Tile::Rock => "Rock",
            Tile::MediumRock => "MediumRock",
            Tile::BigRock => "BigRock",
            Tile::Pebble => "Pebble",
            Tile::Flower => "Flower",
            Tile::Cactus => "Cactus",
            Tile::Well => "Well",
            Tile::Path => "Path",
            Tile::Lamppost => "Lamppost",
            Tile::Bench => "Bench",
            Tile::MineEntrance => "MineEntrance",
            Tile::MineFrame => "MineFrame",
            Tile::CaveFloor => "CaveFloor",
            Tile::CaveWall => "CaveWall",
            Tile::Stalactite => "Stalactite",
            Tile::Stalagmite => "Stalagmite",
            Tile::OreRock => "OreRock",
            Tile::MineralWater => "MineralWater",
            Tile::MineExit => "MineExit",
            Tile::Seabed => "Seabed",
            Tile::CoralTrunk => "CoralTrunk",
            Tile::CoralCanopy => "CoralCanopy",
            Tile::Kelp => "Kelp",
            Tile::DeepWater => "DeepWater",
            Tile::Anemone => "Anemone",
            Tile::InfernoWall => "InfernoWall",
            Tile::InfernoFloor => "InfernoFloor",
            Tile::Lava => "Lava",
            Tile::LandmarkWall => "LandmarkWall",
            Tile::LandmarkDoor => "LandmarkDoor",
            Tile::Tombstone => "Tombstone",
            Tile::DimPortal => "DimPortal",
            Tile::PortalFrame => "PortalFrame",
            Tile::Smelter => "Smelter",
            Tile::Forge => "Forge",
            Tile::CookingPot => "CookingPot",
            Tile::Curio => "Curio",
        }
    }

    pub fn describe(self) -> &'static str {
        crate::inspect_text::get(&format!("tile:{}", self.id_str()))
    }
}

pub struct World {
    pub seed: u32,
    pub dim: Dimension,
    /// Chopped-tree state. Key = the tree's anchor cell (the trunk
    /// origin). Value = unix-secs timestamp at which the tree is back.
    /// Trees inside `chopped` whose timestamp hasn't elapsed yet skip
    /// rendering: their trunk/canopy cells fall through to underlying
    /// grass. Mirrors the vein cooldown map in spirit.
    pub chopped: std::collections::HashMap<(i32, i32), u64>,
}

pub struct WorldView<'a> {
    pub world: &'a World,
    pub player: (i32, i32),
    pub player_facing: (i32, i32),
    pub tick: u64,
    pub player_on_boat: bool,
    pub player_swimming: bool,
    /// Wandering faceless figures in the Mines (Borin's "ones with no faces").
    /// Empty in every other dim. Painted as dim `o` glyphs.
    pub faceless: &'a [(i32, i32)],
    /// Current in-game day index. Used to seed deterministic bug spawns so
    /// a cell hosts the same bug all day and a fresh roll the next.
    pub day_id: u64,
    /// True during the Night / Midnight time-of-day phases. Drives whether
    /// nocturnal bugs render.
    pub is_night: bool,
    /// Cells where a bug has already been picked today (filtered to the
    /// current dim). Suppresses the bug glyph so a caught bug doesn't pop
    /// right back.
    pub bugs_picked: &'a [(i32, i32)],
    /// Cells where a soil patch has already been dug today (filtered to the
    /// current dim). Suppresses the soil overlay until tomorrow.
    pub soil_dug: &'a [(i32, i32)],
}

impl<'a> Widget for WorldView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use rayon::prelude::*;
        if area.width == 0 || area.height == 0 {
            return;
        }
        let half_w = (area.width as i32) / 2;
        let half_h = (area.height as i32) / 2;
        let player_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let area_w = area.width as usize;
        let n = area_w * area.height as usize;

        // compute (glyph, style) for every cell in parallel; thread-local
        // caches inside render_tile keep biome/water lookups cheap per worker
        let computed: Vec<(char, Style)> = (0..n)
            .into_par_iter()
            .map(|i| {
                let sx = (i % area_w) as i32;
                let sy = (i / area_w) as i32;
                if sx == half_w && sy == half_h {
                    let g = if self.player_on_boat {
                        '8'
                    } else if self.player_swimming {
                        'o'
                    } else {
                        match self.player_facing {
                            (0, -1) => '^',
                            (0, 1) => 'v',
                            (-1, 0) => '<',
                            (1, 0) => '>',
                            _ => '@',
                        }
                    };
                    return (g, player_style);
                }
                let wx = self.player.0 - half_w + sx;
                let wy = self.player.1 - half_h + sy;
                // Place-aware visibility: the perimeter walls of the spawn
                // village block sight in both directions. Cells the player
                // can't see render as pitch black space.
                if self.world.dim == Dimension::Surface
                    && !cell_visible_from(self.player.0, self.player.1, wx, wy)
                {
                    return (' ', Style::default());
                }
                // NPCs are per-dim now (atlantean citizens, crypt ghouls,
                // infernal imps each live only in their own dim).
                if let Some(npc) = crate::npc::npc_at_dim(wx, wy, self.world.dim, self.world.seed) {
                    return (
                        npc.render_char(),
                        Style::default()
                            .fg(npc.render_color())
                            .add_modifier(Modifier::BOLD),
                    );
                }
                if self.faceless.iter().any(|&(fx, fy)| fx == wx && fy == wy) {
                    return (
                        'o',
                        Style::default()
                            .fg(Color::Rgb(70, 60, 60))
                            .add_modifier(Modifier::BOLD),
                    );
                }
                // Bug overlay: deterministic per-day spawn on host-eligible
                // tiles. The bug sits on top of the natural tile glyph so
                // the player sees a `,` / `*` / `v` etc dotted across the
                // biome.
                let tile = self.world.get(wx, wy);
                if crate::bugs::tile_hosts_bugs(tile)
                    && !self.bugs_picked.iter().any(|&(px, py)| px == wx && py == wy)
                {
                    let biome = self.world.biome(wx, wy);
                    if let Some(bug) = crate::bugs::bug_at(
                        wx,
                        wy,
                        self.world.dim,
                        biome,
                        self.is_night,
                        self.day_id,
                        self.world.seed,
                    ) {
                        return (
                            bug.render_char(),
                            Style::default()
                                .fg(bug.render_color())
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
                if crate::bugs::tile_hosts_soil(tile)
                    && !self.soil_dug.iter().any(|&(px, py)| px == wx && py == wy)
                {
                    let biome = self.world.biome(wx, wy);
                    if crate::bugs::soil_at(wx, wy, self.world.dim, biome, self.world.seed) {
                        return (
                            ':',
                            Style::default()
                                .fg(Color::Rgb(120, 80, 50))
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
                self.world.render_tile(wx, wy, self.tick)
            })
            .collect();

        // sequential write into the ratatui buffer
        for (i, (g, s)) in computed.into_iter().enumerate() {
            let sx = (i % area_w) as u16;
            let sy = (i / area_w) as u16;
            let cx = area.x + sx;
            let cy = area.y + sy;
            buf[(cx, cy)].set_char(g).set_style(s);
        }
    }
}


impl World {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            dim: Dimension::Surface,
            chopped: std::collections::HashMap::new(),
        }
    }

    /// True when the tree anchored at (ax, ay) is currently chopped down
    /// (waiting for its respawn timer). Used by the renderer + by chop
    /// validation so a chopped tree can't be re-chopped before respawn.
    pub fn is_tree_chopped(&self, ax: i32, ay: i32) -> bool {
        match self.chopped.get(&(ax, ay)) {
            Some(&until) => crate::mining::now_secs() < until,
            None => false,
        }
    }

    /// Mark the tree at anchor (ax, ay) as chopped. Respawns in
    /// `secs` seconds of real wall-clock.
    pub fn chop_tree(&mut self, ax: i32, ay: i32, secs: u64) {
        let when = crate::mining::now_secs() + secs;
        self.chopped.insert((ax, ay), when);
    }

    /// Drop entries whose respawn time has elapsed. Called occasionally so
    /// the map doesn't grow unbounded over a long session.
    pub fn prune_chopped(&mut self) {
        let now = crate::mining::now_secs();
        self.chopped.retain(|_, until| *until > now);
    }

    pub fn get(&self, x: i32, y: i32) -> Tile {
        let base = match self.dim {
            Dimension::Surface => self.surface_get(x, y),
            Dimension::Mines => self.mines_get(x, y),
            Dimension::Atlantis => self.atlantis_get(x, y),
            Dimension::Inferno => self.inferno_get(x, y),
            Dimension::Sewer => sewer_get(x, y),
            Dimension::HotSpring => hot_spring_get(x, y, self.seed),
            Dimension::Pyramid => pyramid_get(x, y),
            Dimension::SwampCave => swamp_cave_get(x, y, self.seed),
            Dimension::BogCathedral => bog_cathedral_get(x, y),
            Dimension::MirrorLake => mirror_lake_get(x, y),
            Dimension::Iceshelf => iceshelf_get(x, y, self.seed),
            Dimension::Wreckage => wreckage_get(x, y),
            Dimension::Crater => crater_get(x, y),
            Dimension::Colosseum => colosseum_get(x, y),
            Dimension::AllBlue => all_blue_get(x, y, self.seed),
            Dimension::Lakebed => lakebed_get(x, y, self.seed),
        };
        // Overlay a sparse curio when the underlying tile is open floor.
        // Curios block movement; the player stands adjacent and presses
        // `x` while facing one to read its lore.
        if base.walkable() && curio_at(x, y, self.dim, self.seed).is_some() {
            return Tile::Curio;
        }
        base
    }

    fn surface_get(&self, x: i32, y: i32) -> Tile {
        // Bespoke portals first: sewer manhole inside the home-village plaza,
        // wreckage portal floating south of the pier in deep ocean. These
        // sit at fixed coords (independent of seed) so the player can find
        // them after enough exploration.
        if (x, y) == SEWER_PORTAL_XY || (x, y) == WRECKAGE_PORTAL_XY {
            return Tile::DimPortal;
        }
        // Blacksmith stations next to every Blacksmith NPC. Static
        // home-village smith sits at (-12, 1); proc-village smiths at
        // (ax+3, ay). Smelter is north of the smith, Forge south.
        if let Some(t) = blacksmith_station_at(x, y, self.seed) {
            return t;
        }
        if let Some(t) = cooking_pot_at(x, y) {
            return t;
        }
        if let Some(t) = village_tile(x, y) {
            return t;
        }
        if dim_portal_for(x, y, self.seed).is_some() {
            return Tile::DimPortal;
        }
        if portal_frame_at(x, y, self.seed).is_some() {
            return Tile::PortalFrame;
        }
        if pier_cell(x, y) {
            return Tile::Dock;
        }
        // procedural village structures sit on top of water for floating towns
        if let Some(t) = procedural_village_tile(x, y, self.seed) {
            return t;
        }
        if y >= 6 {
            return Tile::Water;
        }
        if y == 5 {
            return Tile::Sand;
        }
        let winfo = cached_water_info(x, y, self.seed);
        if winfo.island_grass {
            return Tile::Grass;
        }
        if winfo.island_sand {
            return Tile::Sand;
        }
        // mine entrances anchor on rocky/rugged surface tiles; check before
        // trees/rocks so the structure wins the cell.
        if let Some(t) = mine_entrance_tile_at(x, y, self.seed) {
            return t;
        }
        // trees first: shoreline trees can plant their roots in the very
        // edge of a lake, and their canopies project over the water
        if !in_village_zone(x, y) {
            let biome = cached_biome_at(x, y, self.seed);
            let p = biome_params(biome);
            if let Some(part) = tree_at(x, y, self.seed, p.tree) {
                // Chopped-tree gate: skip rendering this trunk/canopy if
                // the anchor's respawn timer hasn't elapsed. Fall through
                // to whatever else this cell would have been (grass/etc).
                let still_there = find_tree_anchor(x, y, self.seed)
                    .map(|(ax, ay, _, _)| !self.is_tree_chopped(ax, ay))
                    .unwrap_or(true);
                if still_there {
                    return part;
                }
            }
            if winfo.in_water {
                return Tile::Water;
            }
            if p.cactus > 0.0 {
                let rc = hash2(x, y, self.seed.wrapping_add(0xCAC7_CAC7)) as f32 / u32::MAX as f32;
                if rc < p.cactus {
                    return Tile::Cactus;
                }
            }
            if big_rock_at(x, y, self.seed, p.big_rock) {
                return Tile::BigRock;
            }
            if medium_rock_at(x, y, self.seed, p.medium_rock) {
                return Tile::MediumRock;
            }
            let r = hash2(x, y, self.seed.wrapping_add(0x1234_5678)) as f32 / u32::MAX as f32;
            if r < p.rock {
                return Tile::Rock;
            }
            if r < p.rock + p.pebble {
                return Tile::Pebble;
            }
            if r < p.rock + p.pebble + p.flower {
                return Tile::Flower;
            }
        }
        if winfo.in_water {
            return Tile::Water;
        }
        if well_at(x, y, self.seed) {
            return Tile::Well;
        }
        Tile::Grass
    }

    /// Subterranean cave layer. Carved out by domain-warped noise: open cave
    /// floor where the noise is above a threshold, solid rock otherwise.
    /// Mine exits sit at the same (x, y) as the corresponding surface entrance.
    fn mines_get(&self, x: i32, y: i32) -> Tile {
        // The Crypt occupies a small area around (0, 0) in the mines.
        if let Some(t) = mines_crypt_at(x, y) {
            return t;
        }
        // The exit is wherever the surface had an entrance, so the player
        // can always climb back the way they came.
        if is_mine_entrance_anchor(x, y, self.seed) || is_lakebed_entrance_anchor(x, y, self.seed) {
            return Tile::MineExit;
        }
        // Carve a safe 3x3 pocket of CaveFloor around every exit so the
        // player isn't immediately walled in when they drop down.
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let ax = x + dx;
                let ay = y + dy;
                if is_mine_entrance_anchor(ax, ay, self.seed)
                    || is_lakebed_entrance_anchor(ax, ay, self.seed)
                {
                    return Tile::CaveFloor;
                }
            }
        }
        // Lakebed cave zone: mostly mineral water with the very occasional
        // stone island. No ores in the water — they only live on wall margins.
        if lakebed_region(x, y, self.seed) {
            let r = hash2(x, y, self.seed.wrapping_add(0x1A4E_BED0)) % 1000;
            if r < 20 {
                return Tile::Stalagmite;
            }
            if r < 35 {
                return Tile::Rock;
            }
            return Tile::MineralWater;
        }
        let open = cave_open_at(x, y, self.seed);
        let r = hash2(x, y, self.seed.wrapping_add(0xCAFE_C0DE)) % 1000;
        if !open {
            // Ores ONLY on wall cells that touch an open neighbor — i.e.
            // the very borders of wall masses. Walls deep inside a mass
            // never produce ore (and render invisible elsewhere).
            if r < 80 && is_mines_wall_margin(x, y, self.seed) {
                return Tile::OreRock;
            }
            if r < 55 {
                return Tile::Stalactite;
            }
            return Tile::CaveWall;
        }
        // Small underground pools — check FIRST so we don't sprinkle
        // stalagmites and rocks in the water.
        if mineral_pool_at(x, y, self.seed) {
            return Tile::MineralWater;
        }
        // open floor: scattered decoration (now only on dry cave floor)
        if r < 10 {
            return Tile::Stalagmite;
        }
        if r < 25 {
            return Tile::Rock;
        }
        if r < 40 {
            return Tile::Pebble;
        }
        Tile::CaveFloor
    }

    /// Atlantis: the wet plane. Open deep water everywhere, with patches of
    /// seabed, coral structures, and kelp forests. The player can fish from
    /// any cell — there is no surface here.
    fn atlantis_get(&self, x: i32, y: i32) -> Tile {
        // The Five Elders' castle anchored at (0, 0)
        if let Some(t) = atlantis_castle_at(x, y) {
            return t;
        }
        let r = hash2(x, y, self.seed.wrapping_add(0x0A71_A715)) % 1000;
        // Coral "trees" — same 4-cell structure as surface trees, but coral.
        if let Some(part) = coral_at(x, y, self.seed) {
            return part;
        }
        // sand bars / seabed patches
        if seabed_patch_at(x, y, self.seed) {
            if r < 30 {
                return Tile::Anemone;
            }
            return Tile::Seabed;
        }
        if r < 25 {
            return Tile::Kelp;
        }
        Tile::DeepWater
    }

    /// The Inferno: reskinned mines with lava pockets instead of mineral
    /// pools, and a much higher density of fishable lava. Reached after
    /// 100 lifetime well casts.
    fn inferno_get(&self, x: i32, y: i32) -> Tile {
        // The Fallen Fish's castle at (0, 0)
        if let Some(t) = inferno_castle_at(x, y) {
            return t;
        }
        // The exit mirrors the mines: same anchor positions act as exits.
        if is_mine_entrance_anchor(x, y, self.seed) {
            return Tile::MineExit;
        }
        let inferno_seed = self.seed.wrapping_add(0x1AFE_5A00);
        let open = cave_open_at(x, y, inferno_seed);
        let r = hash2(x, y, self.seed.wrapping_add(0xF1AE_F1AE)) % 1000;
        if !open {
            // Inferno ores also only on wall margins, never deep inside.
            if r < 60 && is_inferno_wall_margin(x, y, inferno_seed) {
                return Tile::OreRock;
            }
            if r < 50 {
                return Tile::Stalactite;
            }
            return Tile::InfernoWall;
        }
        if r < 25 {
            return Tile::Stalagmite;
        }
        if r < 45 {
            return Tile::Rock;
        }
        // Lava is much more common here than mineral water in the mines.
        if lava_pool_at(x, y, self.seed) {
            return Tile::Lava;
        }
        Tile::InfernoFloor
    }

    #[allow(dead_code)]
    pub fn biome(&self, x: i32, y: i32) -> Biome {
        biome_at(x, y, self.seed)
    }

    pub fn render_tile(&self, x: i32, y: i32, tick: u64) -> (char, Style) {
        // In dims with cave-shaped walls, any cell fully buried inside a
        // wall mass renders as pure black — the player can't see through
        // stone. This catches CaveWall, Stalactite, and anything else the
        // wall-zone procedural noise may have placed there.
        match self.dim {
            Dimension::Mines => {
                if is_buried_wall(self, x, y, self.seed) {
                    return (' ', Style::default());
                }
            }
            Dimension::Inferno => {
                if is_buried_wall(self, x, y, self.seed.wrapping_add(0x1AFE_5A00)) {
                    return (' ', Style::default());
                }
            }
            Dimension::HotSpring => {
                if is_buried_wall(self, x, y, self.seed.wrapping_add(0x4075_5E5E)) {
                    return (' ', Style::default());
                }
            }
            Dimension::SwampCave => {
                if is_buried_wall(self, x, y, self.seed.wrapping_add(0x5AA9_0CA1)) {
                    return (' ', Style::default());
                }
            }
            _ => {}
        }
        match self.get(x, y) {
            Tile::Wall => {
                // Player can't see inside walls — buried wall cells go
                // pitch-black regardless of dim. Only the outer surface
                // of any wall mass is rendered.
                if wall_buried(self, x, y) {
                    return (' ', Style::default());
                }
                match self.dim {
                    Dimension::Pyramid => sandstone_wall_glyph(x, y),
                    Dimension::Wreckage => wood_hull_glyph(x, y),
                    Dimension::Colosseum => roman_wall_glyph(x, y),
                    Dimension::BogCathedral => gothic_wall_glyph(x, y),
                    Dimension::Sewer => sewer_brick_glyph(x, y),
                    _ => perimeter_glyph(x, y).unwrap_or_else(|| {
                        // Per-house variant if this wall belongs to one.
                        if let Some(door) = house_seed_at(x, y, self.seed) {
                            wall_glyph_for_house(x, y, door)
                        } else {
                            wall_glyph(x, y)
                        }
                    }),
                }
            }
            Tile::Roof => {
                if self.dim == Dimension::Surface {
                    if let Some(door) = house_seed_at(x, y, self.seed) {
                        if house_chimney_at(x, y, door) {
                            return chimney_glyph(x, y, door);
                        }
                        return roof_glyph_for_house(x, y, door);
                    }
                }
                roof_glyph(x, y)
            }
            Tile::DoorRod => (
                'D',
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::DoorSchool => (
                'D',
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::DoorHouse => (
                'D',
                Style::default()
                    .fg(Color::Rgb(60, 40, 25))
                    .bg(Color::Rgb(180, 150, 110))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Dock => ('=', Style::default().fg(Color::LightYellow)),
            Tile::Grass => grass_anim(x, y, tick, cached_biome_at(x, y, self.seed)),
            Tile::Water => {
                // Specialty dims override the standard ocean blue with a
                // themed tint so each one reads at a glance.
                let glyph = match self.dim {
                    Dimension::Sewer => sewer_water_glyph(x, y, tick),
                    Dimension::SwampCave => swamp_water_glyph(x, y, tick),
                    Dimension::Wreckage => wreckage_water_glyph(x, y, tick),
                    Dimension::BogCathedral => cathedral_water_glyph(x, y, tick),
                    Dimension::Pyramid => tomb_pool_glyph(x, y, tick),
                    _ => {
                        if matches!(self.get(x, y - 1), Tile::Sand) {
                            shore_anim(x, 1, tick)
                        } else {
                            water_anim(x, y, tick)
                        }
                    }
                };
                // Surface ocean: bg darkens with distance from shore and
                // flips foggy past FOG_DEPTH for the endgame "Fog Sea".
                // Specialty water dims still use their themed tint.
                let bg = if matches!(self.dim, Dimension::Surface) {
                    ocean_depth_color(ocean_depth_at(self, x, y))
                } else {
                    water_bg_for(self.dim)
                };
                with_fishable_bg(glyph, bg)
            }
            Tile::Sand => {
                // Repurposed per dim: iceshelf = white snow, pyramid = gold
                // sand, colosseum = pale stone, others = beach sand.
                match self.dim {
                    Dimension::Iceshelf => snow_glyph(x, y),
                    Dimension::Pyramid => pyramid_sand_glyph(x, y),
                    _ => {
                        let shore = matches!(self.get(x, y + 1), Tile::Water);
                        if shore {
                            shore_anim(x, 0, tick)
                        } else {
                            let g = match hash2(x, y, 0x5A1D_5A1D) % 3 {
                                0 => ',',
                                1 => '.',
                                _ => '`',
                            };
                            (g, Style::default().fg(shade((198, 182, 132), x, y, 0x5A1D_5A1D, 14)))
                        }
                    }
                }
            }
            Tile::TreeTrunk | Tile::TreeCanopy => {
                if let Some(g) = village_oak_glyph(x, y) {
                    g
                } else {
                    tree_render(x, y, self.seed)
                }
            }
            Tile::Rock => rock_glyph(x, y),
            Tile::MediumRock => medium_rock_glyph(x, y, self.seed),
            Tile::BigRock => big_rock_glyph(x, y, self.seed),
            Tile::Pebble => pebble_glyph(x, y),
            Tile::Flower => flower_glyph(x, y),
            Tile::Cactus => cactus_glyph(x, y),
            Tile::Well => (
                'O',
                Style::default()
                    .fg(Color::Rgb(170, 170, 180))
                    .bg(Color::Rgb(10, 10, 14))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Path => {
                match self.dim {
                    Dimension::Colosseum => roman_floor_glyph(x, y),
                    Dimension::Sewer => sewer_walk_glyph(x, y),
                    Dimension::BogCathedral => cathedral_floor_glyph(x, y),
                    _ => {
                        let g = match hash2(x, y, 0xDADA_BABE) % 3 {
                            0 => '.',
                            1 => ',',
                            _ => '.',
                        };
                        (g, Style::default().fg(Color::Rgb(150, 135, 105)))
                    }
                }
            }
            Tile::Lamppost => (
                'i',
                Style::default()
                    .fg(Color::Rgb(220, 200, 120))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Bench => (
                '=',
                Style::default()
                    .fg(Color::Rgb(140, 95, 55))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::MineEntrance => {
                // Lakebed entrances re-use the MineEntrance tile but live
                // on a lake island and drop into Dimension::Lakebed.
                // Differentiate the render so the player can tell them
                // apart: dry shaft is rusty-brown '#', lakebed is a watery
                // blue 'V' (A-frame seen from above).
                if is_lakebed_entrance_anchor(x, y, self.seed) {
                    (
                        'V',
                        Style::default()
                            .fg(Color::Rgb(110, 200, 240))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        '#',
                        Style::default()
                            .fg(Color::Rgb(60, 40, 25))
                            .add_modifier(Modifier::BOLD),
                    )
                }
            }
            Tile::MineFrame => mine_frame_glyph(x, y, self.seed),
            Tile::CaveFloor => match self.dim {
                Dimension::HotSpring => hot_spring_floor_glyph(x, y),
                Dimension::SwampCave => swamp_floor_glyph(x, y),
                Dimension::Crater => crater_floor_glyph(x, y),
                Dimension::Lakebed => lakebed_floor_glyph(x, y),
                _ => cave_floor_glyph(x, y),
            },
            Tile::CaveWall => {
                // Walls fully buried inside a mass render pitch black so
                // the cave reads as solid stone, not a wall of hashes.
                let buried_seed = match self.dim {
                    Dimension::HotSpring => self.seed.wrapping_add(0x4075_5E5E),
                    Dimension::SwampCave => self.seed.wrapping_add(0x5AA9_0CA1),
                    _ => self.seed,
                };
                if is_buried_wall(self, x, y, buried_seed) {
                    return (' ', Style::default());
                }
                match self.dim {
                    Dimension::HotSpring => hot_spring_wall_glyph(x, y),
                    Dimension::SwampCave => swamp_wall_glyph(x, y),
                    Dimension::Crater => crater_wall_glyph(x, y),
                    _ => cave_wall_glyph(x, y),
                }
            }
            Tile::Stalactite => {
                let h = hash2(x, y, 0x57AC_1117);
                let g = match h % 3 {
                    0 => 'V',
                    1 => 'v',
                    _ => 'y',
                };
                let shade = 140 + (h % 50) as u8;
                (g, Style::default().fg(Color::Rgb(shade, shade - 10, shade - 25)))
            }
            Tile::Stalagmite => {
                let h = hash2(x, y, 0x57A6_A177);
                let g = match h % 3 {
                    0 => 'A',
                    1 => '^',
                    _ => 'T',
                };
                let shade = 145 + (h % 45) as u8;
                (g, Style::default().fg(Color::Rgb(shade - 5, shade - 15, shade - 30)))
            }
            Tile::OreRock => ore_rock_glyph(x, y, self.dim, self.seed),
            Tile::MineralWater => with_fishable_bg(
                mineral_water_glyph_with(x, y, tick, mineral_palette_for(self.dim)),
                mineral_bg_for(self.dim),
            ),
            Tile::MineExit => (
                '<',
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Seabed => seabed_glyph(x, y),
            Tile::CoralTrunk => coral_trunk_glyph(x, y),
            Tile::CoralCanopy => coral_canopy_glyph(x, y),
            Tile::Kelp => kelp_glyph(x, y, tick),
            Tile::DeepWater => with_fishable_bg(
                deep_water_glyph(x, y, tick),
                Color::Rgb(4, 6, 16),
            ),
            Tile::Anemone => {
                let h = hash2(x, y, 0xA1E_A0AE);
                let g = match h % 3 {
                    0 => 'o',
                    1 => 'O',
                    _ => 'Q',
                };
                let (r, gc, b) = match h % 4 {
                    0 => (255, 150, 90),
                    1 => (255, 90, 130),
                    2 => (180, 90, 220),
                    _ => (255, 200, 110),
                };
                (g, Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD))
            }
            Tile::InfernoWall => {
                if is_buried_wall(self, x, y, self.seed.wrapping_add(0x1AFE_5A00)) {
                    (' ', Style::default())
                } else {
                    inferno_wall_glyph(x, y)
                }
            }
            Tile::InfernoFloor => inferno_floor_glyph(x, y),
            Tile::Lava => with_fishable_bg(
                lava_glyph(x, y, tick),
                Color::Rgb(18, 4, 4),
            ),
            Tile::LandmarkWall => landmark_wall_glyph(x, y, self.dim),
            Tile::LandmarkDoor => landmark_door_glyph(self.dim),
            // Curio: distinct glyph + tinted bg so it stands out against
            // the underlying floor. Pool is dim-specific; (x, y, dim, seed)
            // picks the exact entry deterministically.
            Tile::Curio => {
                if let Some((entry, idx)) = curio_at(x, y, self.dim, self.seed) {
                    let g = entry.1.chars().nth(idx).unwrap_or('?');
                    let (r, gc, b) = entry.2;
                    (
                        g,
                        Style::default()
                            .fg(Color::Rgb(r, gc, b))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    // Defensive fallback (shouldn't happen in practice).
                    ('?', Style::default().fg(Color::Magenta))
                }
            }
            // Smelter: chunky furnace glyph with a low orange glow.
            Tile::Smelter => (
                'S',
                Style::default()
                    .fg(Color::Rgb(255, 140, 60))
                    .bg(Color::Rgb(40, 12, 6))
                    .add_modifier(Modifier::BOLD),
            ),
            // Forge: anvil glyph with red-hot tint.
            Tile::Forge => (
                'F',
                Style::default()
                    .fg(Color::Rgb(255, 90, 60))
                    .bg(Color::Rgb(40, 6, 6))
                    .add_modifier(Modifier::BOLD),
            ),
            // Cooking pot: round vessel atop coals; warm orange on near-black.
            Tile::CookingPot => (
                'O',
                Style::default()
                    .fg(Color::Rgb(255, 200, 120))
                    .bg(Color::Rgb(40, 18, 6))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Tombstone => {
                let g = match hash2(x, y, 0x7070_85_70) % 3 {
                    0 => 'T',
                    1 => '|',
                    _ => '+',
                };
                (g, Style::default().fg(Color::Rgb(170, 170, 180)).add_modifier(Modifier::BOLD))
            }
            Tile::DimPortal => {
                // Glyph + tint hints at the destination dim so portals
                // are distinguishable across biomes.
                let dest = dim_portal_for(x, y, self.seed)
                    .unwrap_or(Dimension::Surface);
                let (g, c) = match dest {
                    Dimension::Pyramid => ('Δ', Color::Rgb(230, 200, 120)),
                    Dimension::HotSpring => ('§', Color::Rgb(220, 180, 200)),
                    Dimension::Iceshelf => ('❄', Color::Rgb(200, 230, 255)),
                    Dimension::SwampCave => ('Ω', Color::Rgb(110, 180, 110)),
                    Dimension::BogCathedral => ('†', Color::Rgb(150, 140, 170)),
                    Dimension::MirrorLake => ('☼', Color::Rgb(220, 230, 255)),
                    Dimension::Crater => ('☄', Color::Rgb(200, 170, 255)),
                    Dimension::Colosseum => ('∞', Color::Rgb(240, 240, 230)),
                    Dimension::Sewer => ('Ψ', Color::Rgb(120, 130, 110)),
                    Dimension::Wreckage => ('Φ', Color::Rgb(100, 140, 170)),
                    Dimension::AllBlue => ('◊', Color::Rgb(120, 200, 255)),
                    _ => ('¤', Color::Rgb(220, 200, 200)),
                };
                (g, Style::default().fg(c).add_modifier(Modifier::BOLD))
            }
            Tile::PortalFrame => portal_frame_glyph(x, y, self.seed),
        }
    }
}

fn cave_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xCA7E_F100);
    let g = match h % 7 {
        0 => '.',
        1 => ',',
        2 => '`',
        3 => '\'',
        4 => '_',
        5 => ':',
        _ => ' ',
    };
    // dark dirt floor — keeps the walls feeling tall
    let shade = 30 + (h % 18) as u8;
    (
        g,
        Style::default().fg(Color::Rgb(shade + 12, shade, shade.saturating_sub(6))),
    )
}

fn cave_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xCAFE_5A1F);
    let g = match h % 9 {
        0 => '#',
        1 => '%',
        2 => '&',
        3 => 'M',
        4 => 'N',
        5 => 'W',
        6 => '@',
        7 => '8',
        _ => 'B',
    };
    let shade = 48 + (h % 38) as u8;
    let r = shade + 18;
    let gc = shade.saturating_sub(4);
    let b = shade.saturating_sub(18);
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
}

fn ore_rock_glyph(x: i32, y: i32, dim: Dimension, seed: u32) -> (char, Style) {
    let ore = crate::mining::ore_at_vein(x, y, dim, seed);
    // glyph varies a bit per cell for texture, but color = the actual ore.
    let ch = match hash2(x, y, 0x09E5_EED1) % 4 {
        0 => '*',
        1 => '+',
        2 => 'X',
        _ => '%',
    };
    (
        ch,
        Style::default().fg(ore.color).add_modifier(Modifier::BOLD),
    )
}

#[derive(Clone, Copy)]
enum MineralPalette {
    Cyan,
    HotSpring,
    Crater,
    MirrorLake,
}

fn mineral_palette_for(dim: Dimension) -> MineralPalette {
    match dim {
        Dimension::HotSpring => MineralPalette::HotSpring,
        Dimension::Crater => MineralPalette::Crater,
        Dimension::MirrorLake => MineralPalette::MirrorLake,
        _ => MineralPalette::Cyan,
    }
}

fn mineral_water_glyph_with(x: i32, y: i32, tick: u64, pal: MineralPalette) -> (char, Style) {
    // Three overlapping sines drive a height field; the glyph follows
    // the local height. `pal` picks the color ramp so each dim's pools
    // read as a distinct material.
    let t = tick as f32 * 0.045;
    let fx = x as f32;
    let fy = y as f32;
    let w1 = (fx * 0.42 + fy * 0.23 + t * 1.0).sin();
    let w2 = (fx * 0.28 - fy * 0.51 + t * 0.7).sin();
    let w3 = (fx * 0.17 + fy * 0.34 - t * 0.5).sin() * 0.7;
    let h = w1 + w2 + w3;
    let ramps_cyan = [(160,210,225),(110,170,200),(85,145,180),(65,115,160),(50,95,140),(40,80,120),(30,65,100)];
    let ramps_hot  = [(255,210,180),(250,160,120),(230,110,75),(190,75,55),(150,45,40),(115,30,30),(85,20,25)];
    let ramps_crater = [(220,180,255),(190,135,235),(160,95,215),(125,65,190),(95,45,160),(70,30,130),(50,20,100)];
    let ramps_mirror = [(225,235,245),(190,210,225),(160,185,210),(135,160,190),(110,135,170),(90,115,150),(70,95,130)];
    let ramp = match pal {
        MineralPalette::Cyan => &ramps_cyan,
        MineralPalette::HotSpring => &ramps_hot,
        MineralPalette::Crater => &ramps_crater,
        MineralPalette::MirrorLake => &ramps_mirror,
    };
    let idx = if h > 2.0 { 0 } else if h > 1.1 { 1 } else if h > 0.3 { 2 }
              else if h > -0.4 { 3 } else if h > -1.1 { 4 } else if h > -1.9 { 5 } else { 6 };
    let glyph = match idx { 0 | 1 | 2 => '~', 3 => '-', 4 => '_', 5 => '.', _ => ',' };
    let base = ramp[idx];
    // Occasional bright sparkle so the surface twinkles.
    let sparkle = hash2(x, y, 0x9A7E_5A1E).wrapping_add((tick / 5) as u32) % 90 == 0;
    if sparkle && h > 0.5 {
        let (br, bg, bb) = ramp[0];
        return ('*', Style::default().fg(Color::Rgb(br, bg, bb)).add_modifier(Modifier::BOLD));
    }
    let mut style = Style::default().fg(shade(base, x, y, 0x9A7E_5A1E, 4));
    if h > 1.1 {
        style = style.add_modifier(Modifier::BOLD);
    }
    (glyph, style)
}

fn seabed_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5EAB_ED01);
    let g = match h % 9 {
        0 => '.',
        1 => ',',
        2 => '`',
        3 => ':',
        4 => '\'',
        5 => '_',
        6 => '~',
        7 => '*',
        _ => ' ',
    };
    let blue = 130 + (h % 70) as u8;
    (
        g,
        Style::default().fg(Color::Rgb(
            blue.saturating_sub(30),
            blue.saturating_sub(5),
            blue.saturating_add(20).min(240),
        )),
    )
}

fn coral_trunk_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xC0A1_7A77);
    let g = match h % 5 {
        0 => 'Y',
        1 => 'V',
        2 => '#',
        3 => '+',
        _ => 'I',
    };
    let (r, gc, b) = match h % 6 {
        0 => (240, 130, 150), // pink
        1 => (255, 180, 80),  // orange
        2 => (180, 90, 220),  // purple
        3 => (255, 220, 130), // yellow
        4 => (230, 80, 90),   // red
        _ => (130, 220, 200), // teal
    };
    (
        g,
        Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD),
    )
}

fn coral_canopy_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xC0A1_CA70);
    let g = match h % 6 {
        0 => '*',
        1 => '\u{B0}', // degree mark
        2 => 'o',
        3 => '%',
        4 => '#',
        _ => '+',
    };
    // brighter cousin of the trunk color
    let (r, gc, b) = match h % 6 {
        0 => (255, 170, 200),
        1 => (255, 220, 130),
        2 => (220, 140, 240),
        3 => (255, 240, 180),
        4 => (255, 130, 130),
        _ => (170, 240, 220),
    };
    (
        g,
        Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD),
    )
}

fn kelp_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    // gentle sway: pick glyph based on time so the kelp leans
    let sway = ((tick / 10) as i32 + y).rem_euclid(5);
    let g = match sway {
        0 | 4 => '/',
        1 | 3 => 'i',
        _ => '\\',
    };
    let h = hash2(x, y, 0xCE7_C001);
    let shade = 160 + (h % 60) as u8;
    (
        g,
        Style::default().fg(Color::Rgb(40, shade, 90)),
    )
}

/// Caustic underwater light: shimmering bright bands that drift over the
/// deep. Combines two sine "rays" with a slow drift so the light pattern
/// flows like sunlight refracting through surface waves.
fn deep_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    let t = tick as f32 * 0.06;
    let fx = x as f32;
    let fy = y as f32;
    // two crossing wave rays produce caustic intersections
    let ray1 = (fx * 0.35 + fy * 0.18 + t).sin();
    let ray2 = (fx * 0.22 - fy * 0.31 + t * 1.3).sin();
    let intensity = ray1 + ray2;
    if intensity > 1.55 {
        // bright caustic peak
        let g = match (x + y).rem_euclid(3) {
            0 => '*',
            1 => '+',
            _ => '`',
        };
        return (g, Style::default().fg(Color::Rgb(220, 240, 255)).add_modifier(Modifier::BOLD));
    }
    if intensity > 0.95 {
        let g = match (x * 3 + y).rem_euclid(4) {
            0 => '~',
            1 => '-',
            2 => '`',
            _ => '.',
        };
        return (g, Style::default().fg(Color::Rgb(150, 200, 240)));
    }
    if intensity > 0.2 {
        let g = if (x + y * 2).rem_euclid(7) == 0 { '~' } else { ' ' };
        return (g, Style::default().fg(Color::Rgb(80, 140, 210)));
    }
    // dark deep: scattered tiny particulates
    let g = if hash2(x, y, 0xDEE_BEE5).wrapping_add(tick as u32 / 40) % 200 == 0 {
        '.'
    } else {
        ' '
    };
    (g, Style::default().fg(Color::Rgb(60, 110, 180)))
}

/// The Five Elders' castle in Atlantis. 16-wide x 9-tall rectangle around
/// (0, 0). Interior is open seabed. Door at south-center (0, 4).
fn atlantis_castle_at(x: i32, y: i32) -> Option<Tile> {
    // Satellite outpost — small fortified ring around 23, 16 (south-east)
    if let Some(t) = small_hut_at(x, y, 21, 14)
        .or_else(|| small_hut_at(x, y, 25, 14))
        .or_else(|| small_hut_at(x, y, 19, 17))
        .or_else(|| small_hut_at(x, y, 23, 18))
    {
        return Some(t);
    }
    // First check the central castle.
    let in_x = (-8..=8).contains(&x);
    let in_y = (-4..=4).contains(&y);
    if in_x && in_y {
        let is_perim = x == -8 || x == 8 || y == -4 || y == 4;
        let is_door = (x == 0 || x == -1 || x == 1) && y == 4;
        if is_door {
            return Some(Tile::LandmarkDoor);
        }
        if is_perim {
            return Some(Tile::LandmarkWall);
        }
        return Some(Tile::Seabed);
    }
    // Outlying citizen cottages around the castle. Each is a 4x3 hut with
    // a south-facing door. Positions are hand-placed so the city has
    // an actual shape.
    const ATLANTIS_HUTS: &[(i32, i32)] = &[
        (-13, 4), (-13, 8), (10, 4), (10, 8), (-6, 8), (5, 8),
    ];
    for &(cx, cy) in ATLANTIS_HUTS {
        if let Some(t) = small_hut_at(x, y, cx, cy) {
            return Some(t);
        }
    }
    // ---- satellite cities far from the central castle -----------------
    //
    // Each entry is one city: (cx, cy, temple_half_w, temple_half_h,
    // door_y_offset, hut_pattern_id). The hut pattern is chosen by id so
    // each city has a recognisable layout silhouette. Coords were placed
    // by hand to sit in deep water away from the central cluster.
    //
    // Player teleports in at (0, 7) just south of the central castle door;
    // wandering N, NE, NW, S, SE or SW eventually surfaces one of these.
    if let Some(t) = atlantis_satellite_city(x, y) {
        return Some(t);
    }
    None
}

/// Render one of the satellite Atlantis cities if (x, y) falls within
/// any. Each has a small central temple (6×4) plus a ring of huts. The
/// temple acts as a landmark; the huts are walkable interiors so the
/// player can step in.
fn atlantis_satellite_city(x: i32, y: i32) -> Option<Tile> {
    // (city centre, temple half-extents, hut offset list)
    // hut offsets are relative to the city centre.
    struct City {
        cx: i32,
        cy: i32,
        thw: i32, // temple half-width
        thh: i32, // temple half-height
        huts: &'static [(i32, i32)],
    }
    const CITIES: &[City] = &[
        // North-east — "the Crested Reef" city
        City {
            cx: 60, cy: -45, thw: 5, thh: 3,
            huts: &[(-12, 4), (-7, 6), (0, 7), (7, 6), (12, 4),
                    (-8, -6), (8, -6), (-3, -8), (3, -8)],
        },
        // South-west — "the Trench" city, tighter layout
        City {
            cx: -70, cy: 35, thw: 6, thh: 3,
            huts: &[(-13, 5), (-6, 6), (0, 7), (6, 6), (13, 5),
                    (-10, -5), (10, -5)],
        },
        // South-east — "the Open Pearl" city, larger temple
        City {
            cx: 85, cy: 55, thw: 7, thh: 4,
            huts: &[(-14, 6), (-7, 7), (7, 7), (14, 6),
                    (-9, -7), (0, -8), (9, -7)],
        },
        // North-west — "the Cold Bell" city
        City {
            cx: -55, cy: -55, thw: 4, thh: 3,
            huts: &[(-9, 5), (-3, 6), (3, 6), (9, 5),
                    (-7, -5), (7, -5)],
        },
        // Far-east outpost — "the Long Drift", just a handful of huts
        City {
            cx: 130, cy: 0, thw: 4, thh: 2,
            huts: &[(-8, 4), (-3, 5), (3, 5), (8, 4),
                    (-6, -4), (6, -4)],
        },
    ];
    for city in CITIES {
        // Temple first.
        let dx = x - city.cx;
        let dy = y - city.cy;
        if dx.abs() <= city.thw && dy.abs() <= city.thh {
            let is_perim = dx.abs() == city.thw || dy.abs() == city.thh;
            let is_door = dx == 0 && dy == city.thh;
            if is_door {
                return Some(Tile::LandmarkDoor);
            }
            if is_perim {
                return Some(Tile::LandmarkWall);
            }
            return Some(Tile::Seabed);
        }
        // Hut ring.
        for &(ox, oy) in city.huts {
            if let Some(t) = small_hut_at(x, y, city.cx + ox, city.cy + oy) {
                return Some(t);
            }
        }
    }
    None
}

/// The Crypt in the Mines. 11x7 rectangle around (0, 0), interior dotted
/// with tombstones. Door at south. A smaller secondary crypt sits east.
fn mines_crypt_at(x: i32, y: i32) -> Option<Tile> {
    // Second crypt — east of central, smaller (7x5)
    let sx = x - 22;
    let sy = y;
    if (-3..=3).contains(&sx) && (-2..=2).contains(&sy) {
        let is_perim = sx == -3 || sx == 3 || sy == -2 || sy == 2;
        let is_door = sx == 0 && sy == 2;
        if is_door {
            return Some(Tile::LandmarkDoor);
        }
        if is_perim {
            return Some(Tile::LandmarkWall);
        }
        if sy == 0 && sx.abs() == 2 {
            return Some(Tile::Tombstone);
        }
        return Some(Tile::CaveFloor);
    }
    let in_x = (-5..=5).contains(&x);
    let in_y = (-3..=3).contains(&y);
    if !in_x || !in_y {
        return None;
    }
    let is_perim = x == -5 || x == 5 || y == -3 || y == 3;
    let is_door = x == 0 && y == 3;
    if is_door {
        return Some(Tile::LandmarkDoor);
    }
    if is_perim {
        return Some(Tile::LandmarkWall);
    }
    // tombstones on a sparse pattern inside, avoiding the central row
    // where NPCs stand
    if (x % 2 == 0) && y == 1 && x != 0 {
        return Some(Tile::Tombstone);
    }
    if (x % 2 != 0) && y == -1 && x != 0 {
        return Some(Tile::Tombstone);
    }
    Some(Tile::CaveFloor)
}

/// The Fallen Fish's keep in the Inferno. Larger castle than the Crypt,
/// smaller than the Atlantean palace. Door at south. Hovels of his
/// infernal subjects scattered around.
fn inferno_castle_at(x: i32, y: i32) -> Option<Tile> {
    let in_x = (-7..=7).contains(&x);
    let in_y = (-4..=4).contains(&y);
    if in_x && in_y {
        let is_perim = x == -7 || x == 7 || y == -4 || y == 4;
        let is_door = x == 0 && y == 4;
        if is_door {
            return Some(Tile::LandmarkDoor);
        }
        if is_perim {
            return Some(Tile::LandmarkWall);
        }
        return Some(Tile::InfernoFloor);
    }
    const INFERNO_HOVELS: &[(i32, i32)] = &[
        (-11, 4), (8, 4), (-5, 8), (5, 8),
    ];
    for &(cx, cy) in INFERNO_HOVELS {
        if let Some(t) = small_hut_at(x, y, cx, cy) {
            return Some(t);
        }
    }
    None
}

/// 4-wide x 3-tall hut anchored at (cx, cy) (top-left). Door at south-center.
fn small_hut_at(x: i32, y: i32, cx: i32, cy: i32) -> Option<Tile> {
    let dx = x - cx;
    let dy = y - cy;
    if !(0..=3).contains(&dx) || !(0..=2).contains(&dy) {
        return None;
    }
    let is_perim = dx == 0 || dx == 3 || dy == 0 || dy == 2;
    let is_door = dx == 1 && dy == 2;
    if is_door {
        return Some(Tile::LandmarkDoor);
    }
    if is_perim {
        return Some(Tile::LandmarkWall);
    }
    Some(Tile::CaveFloor)
}

fn landmark_wall_glyph(x: i32, y: i32, dim: Dimension) -> (char, Style) {
    let h = hash2(x, y, 0x1A0D_F00D);
    let g = match h % 5 {
        0 => '#',
        1 => 'H',
        2 => '%',
        3 => '@',
        _ => '8',
    };
    let fg = match dim {
        Dimension::Atlantis => Color::Rgb(200, 230, 255), // bone-white pearl
        Dimension::Mines => Color::Rgb(140, 130, 120),    // crypt granite
        Dimension::Inferno => Color::Rgb(200, 70, 40),    // basalt + ember
        Dimension::Surface => Color::Rgb(180, 145, 95),
        // specialty dims use sensible defaults for now
        _ => Color::Rgb(170, 160, 150),
    };
    (g, Style::default().fg(fg).add_modifier(Modifier::BOLD))
}

fn landmark_door_glyph(dim: Dimension) -> (char, Style) {
    let fg = match dim {
        Dimension::Atlantis => Color::Rgb(255, 230, 130),
        Dimension::Mines => Color::Rgb(110, 100, 90),
        Dimension::Inferno => Color::Rgb(255, 130, 50),
        Dimension::Surface => Color::Rgb(210, 175, 110),
        _ => Color::Rgb(200, 175, 130),
    };
    ('D', Style::default().fg(fg).add_modifier(Modifier::BOLD))
}

fn inferno_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x1FF_F100);
    let g = match h % 7 {
        0 => '.',
        1 => ',',
        2 => '`',
        3 => '\'',
        4 => '_',
        5 => '~',
        _ => ' ',
    };
    // dim red glow with seam variation
    let r = 90 + (h % 60) as u8;
    let gc = 30 + (h % 25) as u8;
    let b = 20 + (h % 15) as u8;
    (g, Style::default().fg(Color::Rgb(r + 30, gc, b)))
}

fn inferno_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x1FF_5A11);
    let g = match h % 8 {
        0 => '#',
        1 => '%',
        2 => '&',
        3 => 'M',
        4 => 'N',
        5 => 'W',
        6 => '8',
        _ => 'B',
    };
    let shade = 90 + (h % 50) as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(shade + 30, shade.saturating_sub(40), 30))
            .add_modifier(Modifier::BOLD),
    )
}

fn lava_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    // pulsing lava: glyph + color shift over time so the pool moves
    let phase = ((tick / 6) as i32 + x * 2 + y).rem_euclid(4);
    let g = match (hash2(x, y, 0x1A0A_1A0A) + tick as u32 / 8) % 5 {
        0 => '~',
        1 => '*',
        2 => '.',
        3 => '&',
        _ => '~',
    };
    let (r, gc, b) = match phase {
        0 => (255, 110, 30),
        1 => (255, 140, 50),
        2 => (220, 80, 20),
        _ => (240, 100, 25),
    };
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
}

fn mine_frame_glyph(x: i32, y: i32, seed: u32) -> (char, Style) {
    for dx in -1..=1i32 {
        for dy in -1..=0i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let ax = x - dx;
            let ay = y - dy;
            if is_mine_entrance_anchor(ax, ay, seed) {
                let glyph = match (dx, dy) {
                    (-1, -1) => '/',
                    (0, -1) => '=',
                    (1, -1) => '\\',
                    (-1, 0) | (1, 0) => '|',
                    _ => '#',
                };
                return (
                    glyph,
                    Style::default()
                        .fg(Color::Rgb(140, 95, 55))
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
    ('#', Style::default().fg(Color::Rgb(60, 45, 30)))
}

fn is_big_rock_anchor(x: i32, y: i32, seed: u32, density: f32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if cached_water_body_at(x, y, seed) {
        return false;
    }
    let r = hash2(x, y, seed.wrapping_add(0xBEEF_FACE)) as f32 / u32::MAX as f32;
    r < density
}

fn big_rock_at(x: i32, y: i32, seed: u32, density: f32) -> bool {
    for dx in 0..2i32 {
        for dy in 0..2i32 {
            let ax = x - dx;
            let ay = y - dy;
            if is_big_rock_anchor(ax, ay, seed, density) {
                return true;
            }
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeSpecies {
    Round,
    Pine,
    Bush,
}

#[derive(Clone, Copy)]
enum TreePart {
    Trunk,
    Canopy,
}

fn tree_species(ax: i32, ay: i32, seed: u32) -> TreeSpecies {
    match hash2(ax, ay, seed.wrapping_add(0xDEAD_F00D)) % 5 {
        0 | 1 => TreeSpecies::Round,
        2 | 3 => TreeSpecies::Pine,
        _ => TreeSpecies::Bush,
    }
}

fn tree_offsets(sp: TreeSpecies) -> &'static [(i32, i32, TreePart)] {
    match sp {
        TreeSpecies::Round => &[
            (0, 0, TreePart::Trunk),
            (-1, -1, TreePart::Canopy),
            (0, -1, TreePart::Canopy),
            (1, -1, TreePart::Canopy),
        ],
        TreeSpecies::Pine => &[
            (0, 0, TreePart::Trunk),
            (0, -1, TreePart::Canopy),
            (-1, -1, TreePart::Canopy),
            (1, -1, TreePart::Canopy),
            (0, -2, TreePart::Canopy),
        ],
        TreeSpecies::Bush => &[(0, 0, TreePart::Trunk)],
    }
}

// At most one tree per (TREE_GRID_W x TREE_GRID_H) cell. The winner is the
// candidate inside the grid block with the smallest hash that also passes
// its density roll. Spacing is generous so trees never overlap.
const TREE_GRID_W: i32 = 6;
const TREE_GRID_H: i32 = 3;

// Ring trees are gated by an extra coarse quantization so a lake gets a
// sparse handful of perimeter trees, not a continuous wall.
const RING_PATCH_W: i32 = 8;
const RING_PATCH_H: i32 = 3;

fn tree_density_at(x: i32, y: i32, seed: u32, base: f32) -> f32 {
    let info = cached_water_info(x, y, seed);
    if info.island_grass || info.island_sand {
        return 0.0;
    }
    if info.in_water {
        // only the very edge of the lake can host a tree, with the same
        // patch gate as the ring so it stays sparse
        if !info.in_shore {
            return 0.0;
        }
        let px = x.div_euclid(RING_PATCH_W);
        let py = y.div_euclid(RING_PATCH_H);
        let ph = hash2(px, py, seed.wrapping_add(0x5E0E_F00D));
        if ph % 2 != 0 {
            return 0.0;
        }
        return 0.7;
    }
    if info.in_ring {
        let px = x.div_euclid(RING_PATCH_W);
        let py = y.div_euclid(RING_PATCH_H);
        let ph = hash2(px, py, seed.wrapping_add(0x4E11_F00D));
        if ph % 2 != 0 {
            return 0.0;
        }
        return 0.85;
    }
    base
}

fn tree_roll(x: i32, y: i32, seed: u32) -> f32 {
    hash2(x, y, seed.wrapping_add(0xC0DE_C0DE)) as f32 / u32::MAX as f32
}

fn is_tree_anchor(x: i32, y: i32, seed: u32, density: f32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 || y <= -1000 {
        return false;
    }
    let my_density = tree_density_at(x, y, seed, density);
    if my_density <= 0.0 {
        return false;
    }
    let my_roll = tree_roll(x, y, seed);
    if my_roll >= my_density {
        return false;
    }

    // grid-cell collision: only one anchor wins per grid block
    let gx = x.div_euclid(TREE_GRID_W);
    let gy = y.div_euclid(TREE_GRID_H);
    for oy in 0..TREE_GRID_H {
        for ox in 0..TREE_GRID_W {
            let cx = gx * TREE_GRID_W + ox;
            let cy = gy * TREE_GRID_H + oy;
            if (cx, cy) == (x, y) {
                continue;
            }
            if in_village_zone(cx, cy) || cy >= 4 || cy <= -1000 {
                continue;
            }
            let other_density = tree_density_at(cx, cy, seed, density);
            if other_density <= 0.0 {
                continue;
            }
            let other_roll = tree_roll(cx, cy, seed);
            if other_roll < other_density && other_roll < my_roll {
                return false; // someone in this grid cell beats me
            }
        }
    }
    true
}

const WATER_CELL_W: i32 = 36;
const WATER_CELL_H: i32 = 22;
const RING_OUTER: f32 = 1.40;

fn compute_water_info(x: i32, y: i32, seed: u32) -> CellWaterInfo {
    let mut info = CellWaterInfo::default();
    if in_village_zone(x, y) {
        return info;
    }
    if y >= 5 {
        return info;
    }
    // procedural villages can FORCE water around themselves: towns get a
    // lake at their east side, floating towns sit in a big disc of water
    if let Some(v) = village_anchor_for(x, y, seed) {
        match v.kind {
            VillageKind::Town => {
                let lx = v.ax + 26;
                let ly = v.ay;
                let dxf = (x - lx) as f32 / 14.0;
                let dyf = (y - ly) as f32 / 6.0;
                let d = dxf * dxf + dyf * dyf;
                if d <= 1.0 {
                    info.in_water = true;
                    return info;
                } else if d <= RING_OUTER * RING_OUTER {
                    info.in_ring = true;
                }
            }
            VillageKind::Floating => {
                let dxf = (x - v.ax) as f32 / 24.0;
                let dyf = (y - v.ay) as f32 / 18.0;
                let d = dxf * dxf + dyf * dyf;
                if d <= 1.0 {
                    info.in_water = true;
                    return info;
                } else if d <= RING_OUTER * RING_OUTER {
                    info.in_ring = true;
                }
            }
            _ => {}
        }
    }
    let cx = x.div_euclid(WATER_CELL_W);
    let cy = y.div_euclid(WATER_CELL_H);
    for dcy in -1..=1 {
        for dcx in -1..=1 {
            let ccx = cx + dcx;
            let ccy = cy + dcy;
            let h = hash2(ccx, ccy, seed.wrapping_add(0xF00D_BEEF));
            if h % 8 != 0 {
                continue;
            }
            let ox = ((h >> 4) as i32).rem_euclid(WATER_CELL_W);
            let oy = ((h >> 12) as i32).rem_euclid(WATER_CELL_H);
            let ax = ccx * WATER_CELL_W + ox;
            let ay = ccy * WATER_CELL_H + oy;
            // 5 size classes including a HUGE lake that can host an island
            let (rx, ry, is_huge): (i32, i32, bool) = match (h >> 20) % 12 {
                0..=2 => (4, 2, false),     // puddle
                3..=6 => (10, 4, false),    // pond
                7..=8 => (16, 6, false),    // lake
                9..=10 => (24, 8, false),   // long lake
                _ => (40, 14, true),        // huge lake (with island)
            };
            if ay + ry >= 5 {
                continue;
            }
            let dxf = (x - ax) as f32 / rx.max(1) as f32;
            let dyf = (y - ay) as f32 / ry.max(1) as f32;
            let d = dxf * dxf + dyf * dyf;
            if d <= 1.0 {
                info.in_water = true;
                if d > 0.82 {
                    info.in_shore = true;
                }
                if is_huge {
                    // island position derived from anchor hash, slightly off-center
                    let iox = ((h >> 4) as i32 % 10) - 5;
                    let ioy = ((h >> 8) as i32 % 6) - 3;
                    let i_ax = ax + iox;
                    let i_ay = ay + ioy;
                    let i_rx = 5;
                    let i_ry = 2;
                    let idx = (x - i_ax) as f32 / i_rx as f32;
                    let idy = (y - i_ay) as f32 / i_ry as f32;
                    let id = idx * idx + idy * idy;
                    if id <= 0.55 {
                        info.island_grass = true;
                    } else if id <= 1.0 {
                        info.island_sand = true;
                    }
                }
                // an island cell still has in_water=true (so it overrides
                // ring/tree generation) but island flags take priority
                // in the World::get dispatch below.
            } else if d <= RING_OUTER * RING_OUTER {
                info.in_ring = true;
            }
        }
    }
    info
}

fn tree_at(x: i32, y: i32, seed: u32, density: f32) -> Option<Tile> {
    for dy in 0..=2i32 {
        for dx in -1..=1i32 {
            let ax = x + dx;
            let ay = y + dy;
            let local_density = biome_params(cached_biome_at(ax, ay, seed)).tree;
            // anchor uses biome's own density (an anchor is local to its own biome)
            // density param is for the cell-of-interest; not used here
            let _ = density;
            if !is_tree_anchor(ax, ay, seed, local_density) {
                continue;
            }
            let sp = tree_species(ax, ay, seed);
            for &(ox, oy, part) in tree_offsets(sp) {
                if ax + ox == x && ay + oy == y {
                    return Some(match part {
                        TreePart::Trunk => Tile::TreeTrunk,
                        TreePart::Canopy => Tile::TreeCanopy,
                    });
                }
            }
        }
    }
    None
}

/// Public wrapper for the internal anchor finder. App needs it to mark
/// the right tree as chopped from outside this module.
pub fn find_tree_anchor_pub(x: i32, y: i32, seed: u32) -> Option<(i32, i32)> {
    find_tree_anchor(x, y, seed).map(|(ax, ay, _, _)| (ax, ay))
}

/// Wood-yield multiplier for the tree at this cell. Bushes give half,
/// round trees baseline, pines half-again, village oaks double — bigger
/// trees, more logs.
pub fn tree_yield_mult_at(x: i32, y: i32, seed: u32) -> f32 {
    // Hand-placed village oaks have no procedural anchor; resolve them
    // first via the VILLAGE_OAKS list since they're the tallest trees in
    // the world and deserve the biggest payout.
    if village_oak_at(x, y).is_some() {
        return 2.0;
    }
    match find_tree_anchor(x, y, seed) {
        Some((_, _, TreeSpecies::Bush, _)) => 0.5,
        Some((_, _, TreeSpecies::Round, _)) => 1.0,
        Some((_, _, TreeSpecies::Pine, _)) => 1.5,
        None => 1.0,
    }
}

fn find_tree_anchor(x: i32, y: i32, seed: u32) -> Option<(i32, i32, TreeSpecies, TreePart)> {
    for dy in 0..=2i32 {
        for dx in -1..=1i32 {
            let ax = x + dx;
            let ay = y + dy;
            let density = biome_params(cached_biome_at(ax, ay, seed)).tree;
            if !is_tree_anchor(ax, ay, seed, density) {
                continue;
            }
            let sp = tree_species(ax, ay, seed);
            for &(ox, oy, part) in tree_offsets(sp) {
                if ax + ox == x && ay + oy == y {
                    return Some((ax, ay, sp, part));
                }
            }
        }
    }
    None
}

fn in_village_zone(x: i32, y: i32) -> bool {
    x.abs() <= 50 && (-18..=5).contains(&y)
}

// procedural village system: coarse grid of anchors, three village kinds.
// Origin village is hand-coded (the "Home Village"); procedural villages
// spawn far enough away to avoid colliding with it.

/// Home-village plaza coords for the Sewer manhole. Picked to sit on a
/// path cell south-east of the well so it's noticeable but doesn't block
/// any standard movement lane.
pub const SEWER_PORTAL_XY: (i32, i32) = (7, 3);
/// Open-ocean coords for the Wreckage portal. Deep past the southern
/// pier tip (pier ends at y=12); requires a boat to reach.
pub const WRECKAGE_PORTAL_XY: (i32, i32) = (0, 22);

const PV_CELL_W: i32 = 160;
const PV_CELL_H: i32 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VillageKind {
    Hamlet,
    Town,
    Floating,
}

#[derive(Clone, Copy)]
pub struct PVillage {
    pub ax: i32,
    pub ay: i32,
    pub kind: VillageKind,
    pub hash: u32,
}

const CONSONANTS: &[char] = &[
    'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'n', 'p', 'r', 's', 't', 'v', 'w', 'z',
];
const VOWELS: &[char] = &['a', 'e', 'i', 'o', 'u'];

/// Village/biome name at the given world coords. Returns the home village
/// name when inside the origin village zone, else a procedural village name,
/// else None (caller can fall back to biome).
pub fn location_name_at(x: i32, y: i32, seed: u32) -> Option<String> {
    if in_village_zone(x, y) {
        return Some("Home Village".to_string());
    }
    village_anchor_for(x, y, seed).map(|v| village_name(v.hash))
}

/// Generates a name like "Karovi" or "Telosa" from the anchor hash.
pub fn village_name(h: u32) -> String {
    let syllables = 2 + ((h >> 28) % 2); // 2 or 3 syllables
    let mut s = String::with_capacity(7);
    let mut x = h;
    for i in 0..syllables {
        let c = CONSONANTS[(x as usize) % CONSONANTS.len()];
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let v = VOWELS[(x as usize) % VOWELS.len()];
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        if i == 0 {
            s.push(c.to_ascii_uppercase());
        } else {
            s.push(c);
        }
        s.push(v);
    }
    s
}

/// All village anchors whose footprints touch the rectangle defined by
/// (cx_min..=cx_max, cy_min..=cy_max) of world coordinates.
pub fn villages_in_rect(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    seed: u32,
) -> Vec<PVillage> {
    let cx_min = x0.div_euclid(PV_CELL_W) - 1;
    let cx_max = x1.div_euclid(PV_CELL_W) + 1;
    let cy_min = y0.div_euclid(PV_CELL_H) - 1;
    let cy_max = y1.div_euclid(PV_CELL_H) + 1;
    let mut out = Vec::new();
    for ccy in cy_min..=cy_max {
        for ccx in cx_min..=cx_max {
            let h = hash2(ccx, ccy, seed.wrapping_add(0xC17F_C17F));
            if h % 3 != 0 {
                continue;
            }
            let ox = ((h >> 4) as i32).rem_euclid(PV_CELL_W);
            let oy = ((h >> 12) as i32).rem_euclid(PV_CELL_H);
            let ax = ccx * PV_CELL_W + ox;
            let ay = ccy * PV_CELL_H + oy;
            if ax.abs() < 100 && ay > -40 {
                continue;
            }
            if ay > -8 {
                continue;
            }
            let kind = match (h >> 24) % 10 {
                0..=4 => VillageKind::Hamlet,
                5..=7 => VillageKind::Town,
                _ => VillageKind::Floating,
            };
            out.push(PVillage { ax, ay, kind, hash: h });
        }
    }
    out
}

/// Static curio pools per dimension. Each entry is (id, glyph, rgb).
/// Density is gated by `curio_at` (1-in-~700 hash hit). Players see them
/// as standout cells against the floor; `x` while facing one reads the
/// flavor text from `inspect.json` (key `curio:<id>`). User writes the
/// prose; engine just places the objects.
/// (id, glyph_string, rgb_color). `glyph_string` is 1-N pure-ASCII chars
/// — single chars for tiny objects, multi-char ASCII art for richer
/// constructions (anchors / sarcophagi / chains / etc). The string's
/// length defines the curio's horizontal footprint.
type CurioEntry = (&'static str, &'static str, (u8, u8, u8));

/// Hard cap on curio width. `curio_at` scans up to this many cells
/// westward to find an anchor whose footprint covers the queried cell,
/// so this directly drives the render hot-path cost. Keep small.
const MAX_CURIO_WIDTH: i32 = 5;

const CURIOS_SURFACE: &[CurioEntry] = &[
    ("driftwood",      "~/~",  (160, 130, 90)),
    ("seaglass",       "<*>",  (140, 210, 200)),
    ("rusted-reel",    "[O]",  (140, 80, 50)),
    ("cairn",          "/A\\", (170, 165, 150)),
    ("broken-buoy",    "(o)",  (220, 150, 80)),
    ("memorial-stone", "[+]",  (180, 175, 165)),
    ("crow-feather",   "v",    (60, 55, 70)),
    ("salt-crust",     ".:.",  (220, 220, 230)),
];

const CURIOS_MINES: &[CurioEntry] = &[
    ("rust-helmet",    "(h)",  (110, 80, 55)),
    ("broken-pick",    "/=,",  (140, 130, 110)),
    ("coal-heap",      ":.:",  (40, 35, 40)),
    ("buried-lantern", "(*)",  (220, 190, 90)),
    ("skeleton-bits",  "x:x",  (220, 215, 200)),
    ("tally-marks",    "||/",  (150, 145, 130)),
    ("mineshaft-sign", "[T]",  (180, 140, 90)),
];

const CURIOS_ATLANTIS: &[CurioEntry] = &[
    ("sunken-statue",   "(@)",  (180, 200, 215)),
    ("ancient-amphora", "[=]",  (190, 165, 110)),
    ("pearl-bed",       "oOo",  (240, 235, 215)),
    ("whale-bone",      "<_>",  (230, 225, 210)),
    ("coral-skull",     "@^@",  (240, 180, 175)),
    ("sea-shrine",      "/^\\", (170, 200, 210)),
    ("ship-anchor",     "+|+",  (130, 140, 150)),
];

const CURIOS_INFERNO: &[CurioEntry] = &[
    ("obsidian-shard", "<*>",  (40, 35, 45)),
    ("bone-pile",      "x:x",  (220, 200, 170)),
    ("brimstone-vent", "(*)",  (240, 200, 90)),
    ("charred-banner", "|=|",  (180, 90, 50)),
    ("lava-glass",     "*o*",  (255, 130, 60)),
    ("rusted-chain",   "===",  (150, 90, 55)),
];

const CURIOS_SEWER: &[CurioEntry] = &[
    ("broken-crate",   "[=]",  (140, 110, 75)),
    ("rat-skull",      "<x>",  (200, 195, 180)),
    ("pipe-fragment",  "(o)",  (110, 100, 90)),
    ("coin-pile",      "$$$",  (220, 200, 120)),
    ("graffiti-slab",  "#%#",  (130, 150, 110)),
    ("soggy-paper",    ":._:", (160, 150, 130)),
];

const CURIOS_HOTSPRING: &[CurioEntry] = &[
    ("bath-stone",     "(O)",  (200, 200, 210)),
    ("mineral-cake",   "(.)",  (240, 220, 180)),
    ("bamboo-basin",   "{=}",  (160, 180, 120)),
    ("offering-bowl",  "(_)",  (220, 200, 170)),
    ("steam-shrine",   "/^\\", (210, 220, 230)),
];

const CURIOS_PYRAMID: &[CurioEntry] = &[
    ("sand-sarcophagus", "[==]", (210, 180, 110)),
    ("hieroglyph-slab",  "#=#",  (220, 195, 140)),
    ("broken-urn",       "_/_",  (180, 145, 95)),
    ("keystone",         "<O>",  (230, 200, 145)),
    ("mummy-wrap",       "~~~",  (215, 200, 170)),
    ("scarab-husk",      "<@>",  (90, 130, 70)),
];

const CURIOS_SWAMPCAVE: &[CurioEntry] = &[
    ("bog-bone",     "x_x",   (200, 190, 170)),
    ("sunken-log",   "====",  (90, 75, 55)),
    ("swamp-totem",  "[I]",   (110, 90, 70)),
    ("mossy-stone",  "(o)",   (110, 140, 80)),
    ("witch-bundle", ":~:",   (140, 110, 130)),
];

const CURIOS_BOGCATHEDRAL: &[CurioEntry] = &[
    ("toppled-candle", "_|_",  (220, 200, 150)),
    ("stained-glass",  "<#>",  (170, 130, 200)),
    ("reliquary",      "[+]",  (200, 180, 220)),
    ("wax-pool",       "(_)",  (230, 215, 165)),
    ("cracked-pew",    "===",  (110, 90, 75)),
    ("hymnal-scrap",   "_._",  (180, 170, 160)),
];

const CURIOS_MIRRORLAKE: &[CurioEntry] = &[
    ("polished-stone",  "(O)",  (220, 230, 240)),
    ("silver-coin",     "(o)",  (220, 225, 230)),
    ("glass-shard",     "/*\\", (180, 210, 230)),
    ("mirror-frame",    "[ ]",  (190, 200, 220)),
    ("reflection-pool", "[o]",  (210, 225, 240)),
];

const CURIOS_ICESHELF: &[CurioEntry] = &[
    ("frozen-fish",     "<o>",  (180, 210, 240)),
    ("ice-pick",        "Y|.",  (220, 230, 240)),
    ("frost-flower",    "*.*",  (220, 235, 250)),
    ("snowdrift-cairn", "/A\\", (240, 245, 250)),
    ("walrus-tusk",     "/|\\", (230, 215, 190)),
];

const CURIOS_WRECKAGE: &[CurioEntry] = &[
    ("ship-bell",       "(B)",  (200, 170, 90)),
    ("porthole",        "(o)",  (160, 180, 190)),
    ("figurehead",      "[+]",  (180, 140, 95)),
    ("wave-worn-chest", "[_]",  (130, 100, 75)),
    ("rotted-rigging",  "~~~",  (140, 130, 110)),
    ("captain-skull",   "<X>",  (220, 215, 200)),
];

const CURIOS_CRATER: &[CurioEntry] = &[
    ("star-iron",       "*o*",  (220, 220, 255)),
    ("impact-glass",    "<*>",  (180, 200, 240)),
    ("meteor-fragment", ".o.",  (160, 130, 200)),
    ("cosmic-sigil",    "[+]",  (220, 180, 255)),
    ("crystal-bloom",   "/^\\", (200, 220, 255)),
];

const CURIOS_COLOSSEUM: &[CurioEntry] = &[
    ("cracked-column", "|||", (220, 215, 200)),
    ("gladiator-helm", "(h)", (190, 170, 130)),
    ("marble-bust",    "[O]", (235, 230, 215)),
    ("worn-trophy",    "[Y]", (200, 175, 120)),
    ("banner-pole",    "_|_", (180, 60, 60)),
];

const CURIOS_ALLBLUE: &[CurioEntry] = &[
    ("mystery-jelly",  "(@)",  (140, 200, 230)),
    ("lost-lantern",   "(*)",  (220, 200, 120)),
    ("empty-bottle",   "_U_",  (160, 200, 220)),
    ("single-sandal",  "[_]",  (200, 170, 130)),
    ("apex-fang",      "/V\\", (240, 230, 220)),
];

fn curio_pool_for(dim: Dimension) -> &'static [CurioEntry] {
    match dim {
        Dimension::Surface => CURIOS_SURFACE,
        Dimension::Mines => CURIOS_MINES,
        Dimension::Atlantis => CURIOS_ATLANTIS,
        Dimension::Inferno => CURIOS_INFERNO,
        Dimension::Sewer => CURIOS_SEWER,
        Dimension::HotSpring => CURIOS_HOTSPRING,
        Dimension::Pyramid => CURIOS_PYRAMID,
        Dimension::SwampCave => CURIOS_SWAMPCAVE,
        Dimension::BogCathedral => CURIOS_BOGCATHEDRAL,
        Dimension::MirrorLake => CURIOS_MIRRORLAKE,
        Dimension::Iceshelf => CURIOS_ICESHELF,
        Dimension::Wreckage => CURIOS_WRECKAGE,
        Dimension::Crater => CURIOS_CRATER,
        Dimension::Colosseum => CURIOS_COLOSSEUM,
        Dimension::AllBlue => CURIOS_ALLBLUE,
        // Lakebed shares the cave-flavour curio pool with the Mines until
        // it gets its own lore drops.
        Dimension::Lakebed => CURIOS_MINES,
    }
}

/// Anchor check — returns the curio whose footprint *begins* at (x, y).
/// Density: 1 in 5000 cells. Calibrated so a typical ~5600-cell viewport
/// surfaces ~1 curio at a time on screen — rare enough that finding one
/// feels like a discovery, not background clutter.
fn curio_anchor_at(x: i32, y: i32, dim: Dimension, seed: u32) -> Option<&'static CurioEntry> {
    let h = hash2(x, y, seed.wrapping_add(0xC0_71_05_03) ^ (dim as u32).wrapping_mul(0x9E37_79B1));
    if h % 5000 != 31 {
        return None;
    }
    let pool = curio_pool_for(dim);
    if pool.is_empty() {
        return None;
    }
    Some(&pool[(h as usize / 5000) % pool.len()])
}

/// Returns Some((entry, char_index)) when (x, y) is part of any curio's
/// horizontal footprint. Scans up to `MAX_CURIO_WIDTH` cells westward
/// for an anchor whose width covers (x, y). The char index lets the
/// renderer pick the correct character of the curio's multi-char glyph
/// string.
pub fn curio_at(x: i32, y: i32, dim: Dimension, seed: u32) -> Option<(&'static CurioEntry, usize)> {
    for k in 0..MAX_CURIO_WIDTH {
        if let Some(entry) = curio_anchor_at(x - k, y, dim, seed) {
            let w = entry.1.chars().count() as i32;
            if k < w {
                return Some((entry, k as usize));
            }
        }
    }
    None
}

/// Cooking pot tile: anchored one cell north of the Chef NPC at (22, 3)
/// so it shows as a warm orange 'O' atop the village path right next to
/// the chef. Interacting with `f` opens the cookbook menu directly.
pub fn cooking_pot_at(x: i32, y: i32) -> Option<Tile> {
    const CHEF: (i32, i32) = (22, 3);
    if (x, y) == (CHEF.0, CHEF.1 - 1) {
        return Some(Tile::CookingPot);
    }
    None
}

/// Smelter / Forge tile placement: one of each is anchored to every
/// Blacksmith NPC (static home-village smith + every procedural village's
/// templated smith). The smelter sits one cell north of the smith, the
/// forge one cell south. `f` on either opens the matching minigame.
pub fn blacksmith_station_at(x: i32, y: i32, seed: u32) -> Option<Tile> {
    // Static home-village smith at (-12, 1). Cheapest check first.
    const HOME_SMITH: (i32, i32) = (-12, 1);
    if (x, y) == (HOME_SMITH.0, HOME_SMITH.1 - 1) {
        return Some(Tile::Smelter);
    }
    if (x, y) == (HOME_SMITH.0, HOME_SMITH.1 + 1) {
        return Some(Tile::Forge);
    }
    // Proc-village smelter/forge sit at smith.y ± 1, and proc village
    // anchors are constrained to `ay <= -8`. So smelter.y <= -9 and
    // forge.y <= -7. If y > -7 there's no chance of a proc station —
    // skip the expensive village_anchor_for call entirely. This is
    // hit once per tile per frame, so the early-out matters.
    if y > -7 {
        return None;
    }
    if let Some(v) = village_anchor_for(x, y, seed) {
        let smith = (v.ax + 3, v.ay);
        if (x, y) == (smith.0, smith.1 - 1) {
            return Some(Tile::Smelter);
        }
        if (x, y) == (smith.0, smith.1 + 1) {
            return Some(Tile::Forge);
        }
    }
    None
}

/// Resolve which procedural-village merchant (if any) stands at this
/// world cell. Returns the template id ("blacksmith-template" or
/// "fishmonger-template"). Each procedural village hosts exactly one of
/// each merchant at fixed offsets from its anchor — blacksmith two cells
/// east of the well, fishmonger two cells west — both on the village's
/// path so the player can walk up and press f.
pub fn proc_village_merchant_id_at(x: i32, y: i32, seed: u32) -> Option<&'static str> {
    // Proc village anchors are constrained to ay <= -8, and both merchants
    // stand at exactly smith.y == anchor.y. So y > -8 has no merchants;
    // skip the expensive village_anchor_for call.
    if y > -8 {
        return None;
    }
    let v = village_anchor_for(x, y, seed)?;
    if (x, y) == (v.ax + 3, v.ay) {
        return Some("blacksmith-template");
    }
    if (x, y) == (v.ax - 3, v.ay) {
        return Some("fishmonger-template");
    }
    None
}

fn village_anchor_for(x: i32, y: i32, seed: u32) -> Option<PVillage> {
    let cx = x.div_euclid(PV_CELL_W);
    let cy = y.div_euclid(PV_CELL_H);
    for dcy in -1..=1 {
        for dcx in -1..=1 {
            let ccx = cx + dcx;
            let ccy = cy + dcy;
            let h = hash2(ccx, ccy, seed.wrapping_add(0xC17F_C17F));
            if h % 3 != 0 {
                continue;
            }
            let ox = ((h >> 4) as i32).rem_euclid(PV_CELL_W);
            let oy = ((h >> 12) as i32).rem_euclid(PV_CELL_H);
            let ax = ccx * PV_CELL_W + ox;
            let ay = ccy * PV_CELL_H + oy;
            // skip if too close to the home village or to the ocean
            if ax.abs() < 100 && ay > -40 {
                continue;
            }
            if ay > -8 {
                continue;
            }
            let kind = match (h >> 24) % 10 {
                0..=4 => VillageKind::Hamlet,
                5..=7 => VillageKind::Town,
                _ => VillageKind::Floating,
            };
            let radius = match kind {
                VillageKind::Hamlet => 18,
                VillageKind::Town => 35,
                VillageKind::Floating => 28,
            };
            if (x - ax).abs() <= radius && (y - ay).abs() <= radius {
                return Some(PVillage { ax, ay, kind, hash: h });
            }
        }
    }
    None
}

fn procedural_village_tile(x: i32, y: i32, seed: u32) -> Option<Tile> {
    let v = village_anchor_for(x, y, seed)?;
    let dx = x - v.ax;
    let dy = y - v.ay;
    match v.kind {
        VillageKind::Hamlet => hamlet_tile(dx, dy),
        VillageKind::Town => town_tile(dx, dy),
        VillageKind::Floating => floating_tile(dx, dy),
    }
}

fn hamlet_tile(dx: i32, dy: i32) -> Option<Tile> {
    // 3 small huts around a central well — all are someone's house.
    let huts = &[
        ((-10, -7), (-6, -5), (-8, -5), Tile::DoorHouse),
        ((6, -7), (10, -5), (8, -5), Tile::DoorHouse),
        ((-2, 5), (2, 7), (0, 7), Tile::DoorHouse),
    ];
    for &((xa, ya), (xb, yb), (dxx, dyy), door) in huts {
        if (xa..=xb).contains(&dx) && (ya..=yb).contains(&dy) {
            if (dx, dy) == (dxx, dyy) {
                return Some(door);
            }
            if dy == ya {
                return Some(Tile::Roof);
            }
            return Some(Tile::Wall);
        }
    }
    if (dx, dy) == (0, 0) {
        return Some(Tile::Well);
    }
    // path: short corridors to each hut
    if dx.abs() <= 1 && (-4..=4).contains(&dy) {
        return Some(Tile::Path);
    }
    if dy.abs() <= 1 && (-8..=8).contains(&dx) {
        return Some(Tile::Path);
    }
    None
}

fn town_tile(dx: i32, dy: i32) -> Option<Tile> {
    // walled town: rectangle from (-18, -10) to (18, 10)
    if dx == -18 || dx == 18 {
        if (-10..=10).contains(&dy) && !(-2..=2).contains(&dy) {
            return Some(Tile::Wall);
        }
    }
    if dy == -10 || dy == 10 {
        if (-18..=18).contains(&dx) && !(-2..=2).contains(&dx) {
            return Some(Tile::Wall);
        }
    }
    // 5 houses inside — one rod shop + one school per town, rest are homes.
    let huts = &[
        ((-15, -7), (-11, -5), (-13, -5), Tile::DoorRod),
        ((-5, -7), (-1, -5), (-3, -5), Tile::DoorHouse),
        ((5, -7), (9, -5), (7, -5), Tile::DoorSchool),
        ((-9, 5), (-5, 7), (-7, 7), Tile::DoorHouse),
        ((5, 5), (9, 7), (7, 7), Tile::DoorHouse),
    ];
    for &((xa, ya), (xb, yb), (dxx, dyy), door) in huts {
        if (xa..=xb).contains(&dx) && (ya..=yb).contains(&dy) {
            if (dx, dy) == (dxx, dyy) {
                return Some(door);
            }
            if dy == ya {
                return Some(Tile::Roof);
            }
            return Some(Tile::Wall);
        }
    }
    if (dx, dy) == (0, 0) {
        return Some(Tile::Well);
    }
    // main cross paths
    if dx.abs() <= 1 && (-9..=9).contains(&dy) {
        return Some(Tile::Path);
    }
    if dy.abs() <= 1 && (-17..=17).contains(&dx) {
        return Some(Tile::Path);
    }
    None
}

fn floating_tile(dx: i32, dy: i32) -> Option<Tile> {
    // dock platform forming a + with houses at the ends
    let on_pier = (dx.abs() <= 2 && (-18..=18).contains(&dy))
        || (dy.abs() <= 2 && (-18..=18).contains(&dx));
    let on_plaza = dx.abs() <= 4 && dy.abs() <= 4;
    let pier = on_pier || on_plaza;
    // small floating houses at the four cardinal ends
    let huts = &[
        ((-16, -1), (-12, 1), (-12, 1), Tile::DoorRod),    // west = rod shop
        ((12, -1), (16, 1), (12, 1), Tile::DoorHouse),     // east = home
        ((-1, -16), (1, -12), (0, -12), Tile::DoorSchool), // north = school
        ((-1, 12), (1, 16), (0, 12), Tile::DoorHouse),     // south = home
    ];
    for &((xa, ya), (xb, yb), (dxx, dyy), door) in huts {
        if (xa..=xb).contains(&dx) && (ya..=yb).contains(&dy) {
            if (dx, dy) == (dxx, dyy) {
                return Some(door);
            }
            if (dx, dy) == ((xa + xb) / 2, (ya + yb) / 2) {
                // center tile of the small hut
                return Some(Tile::Wall);
            }
            return Some(Tile::Wall);
        }
    }
    if pier {
        return Some(Tile::Dock);
    }
    None
}

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = seed.wrapping_add((x as u32).wrapping_mul(374_761_393));
    h = h.wrapping_add((y as u32).wrapping_mul(668_265_263));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

// --- multi-dimension helpers ---------------------------------------------

/// Sparse hash-noise: about one mine entrance per ~3000 surface tiles, only
/// outside the village zone and never inside water. The anchor cell becomes
/// the interactable MineEntrance; the 5 surrounding cells render as MineFrame.
fn is_mine_entrance_anchor(x: i32, y: i32, seed: u32) -> bool {
    // Hash test FIRST — only ~1/12000 cells pass it. The expensive water
    // / village / neighbor checks below run for a vanishingly small slice
    // of cells instead of all of them.
    let h = hash2(x, y, seed.wrapping_add(0xE17E_ED01));
    if h % 12000 != 7 {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if in_village_zone(x, y) {
        return false;
    }
    if cached_water_body_at(x, y, seed) {
        return false;
    }
    // Also reject if any of the 5 frame cells (3 wide x 2 tall above the
    // anchor) would sit on water — that's how entrances were spawning
    // half-in-a-lake and the player couldn't reach them.
    for dx in -1..=1i32 {
        for dy in -1..=0i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if cached_water_body_at(x + dx, y + dy, seed) {
                return false;
            }
        }
    }
    // And reject the southern approach lane too, so you can always walk
    // up to the entrance from the south.
    if cached_water_body_at(x, y + 1, seed) {
        return false;
    }
    // Underground openness check: the same (x, y) in the Mines dim must
    // sit in mostly-open cave, not buried inside a wall mass. Without this
    // the player drops in and immediately stares at pitch-black walls on
    // every side past the carved 3x3 pocket.
    if !cave_open_at(x, y, seed) {
        return false;
    }
    let mut open_neighbors = 0;
    for (dx, dy) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if cave_open_at(x + dx, y + dy, seed) {
            open_neighbors += 1;
        }
    }
    if open_neighbors < 3 {
        return false;
    }
    true
}

/// Manhattan-distance scan to the nearest non-water tile, capped at 24.
/// Used by the fish picker (offshore weight bonus), boat depth gate,
/// HUD readout, and the Fog Sea routing. Cheap enough to run several
/// times per frame: expanding-ring over noise-driven tile lookups,
/// exits the moment it hits land.
pub fn ocean_depth_at(world: &World, x: i32, y: i32) -> u32 {
    let cap: i32 = 24;
    let is_water = |t: Tile| {
        matches!(
            t,
            Tile::Water
                | Tile::DeepWater
                | Tile::MineralWater
                | Tile::Seabed
                | Tile::Kelp
                | Tile::Anemone
                | Tile::Dock
        )
    };
    for r in 1..=cap {
        for dx in -r..=r {
            if !is_water(world.get(x + dx, y - r)) {
                return r as u32;
            }
            if !is_water(world.get(x + dx, y + r)) {
                return r as u32;
            }
        }
        for dy in (-r + 1)..r {
            if !is_water(world.get(x - r, y + dy)) {
                return r as u32;
            }
            if !is_water(world.get(x + r, y + dy)) {
                return r as u32;
            }
        }
    }
    cap as u32
}

/// Depth-darkened ocean tint. Shore stays the standard blue; each tile
/// of offshore depth blends in toward near-black; past `FOG_DEPTH` the
/// tile flips to the foggy ghost-water palette so the Fog Sea reads at
/// a glance.
pub const FOG_DEPTH: u32 = 32;
pub fn ocean_depth_color(depth: u32) -> ratatui::style::Color {
    use ratatui::style::Color;
    if depth >= FOG_DEPTH {
        // Cool foggy gray-violet for the Fog Sea.
        return Color::Rgb(28, 28, 44);
    }
    // Linear darken from (8, 22, 42) at depth 0 down toward (2, 4, 10) at
    // FOG_DEPTH. Stays in low-bg territory so glyphs still pop.
    let t = (depth as f32 / FOG_DEPTH as f32).clamp(0.0, 1.0);
    let r = (8.0 + (2.0 - 8.0) * t) as u8;
    let g = (22.0 + (4.0 - 22.0) * t) as u8;
    let b = (42.0 + (10.0 - 42.0) * t) as u8;
    Color::Rgb(r, g, b)
}

/// Region noise that marks "lakebed cave zones" — patches of the world
/// where the underground is mostly flooded. Cheap to evaluate (2 sines).
pub fn lakebed_region(x: i32, y: i32, seed: u32) -> bool {
    let fx = x as f32;
    let fy = y as f32;
    let s = (fx * 0.030 + fy * 0.040 + (seed as f32 * 0.0023)).sin();
    let t = (fx * 0.050 - fy * 0.027 + (seed as f32 * 0.0017)).cos();
    s + t > 1.2
}

/// A lakebed entrance: a wooden A-frame that sits on a lake island and
/// descends into flooded lakebed caves. Requirements:
///   - sparse hash gate (~1/1200 cells pass)
///   - anchor + 3x2 frame + southern approach all sit on island land
///     (grass or sand). Without this the frame dangles over water and
///     the entrance is unreachable.
///   - underground (x,y) must fall inside a lakebed_region so descending
///     opens flooded caves instead of dry stone.
pub fn is_lakebed_entrance_anchor(x: i32, y: i32, seed: u32) -> bool {
    let h = hash2(x, y, seed.wrapping_add(0x1A4E_BED0));
    if h % 1200 != 3 {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if in_village_zone(x, y) {
        return false;
    }
    if !lakebed_region(x, y, seed) {
        return false;
    }
    let on_island = |x: i32, y: i32| -> bool {
        let i = cached_water_info(x, y, seed);
        i.island_grass || i.island_sand
    };
    if !on_island(x, y) {
        return false;
    }
    for dx in -1..=1i32 {
        for dy in -1..=0i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if !on_island(x + dx, y + dy) {
                return false;
            }
        }
    }
    if !on_island(x, y + 1) {
        return false;
    }
    true
}

fn mine_entrance_tile_at(x: i32, y: i32, seed: u32) -> Option<Tile> {
    if is_mine_entrance_anchor(x, y, seed) || is_lakebed_entrance_anchor(x, y, seed) {
        return Some(Tile::MineEntrance);
    }
    // frame cells: anchor is at (ax, ay) with frame at the 5 cells of the
    // 3-wide, 2-tall box (excluding the anchor itself which is the opening).
    for dx in -1..=1i32 {
        for dy in -1..=0i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if is_mine_entrance_anchor(x - dx, y - dy, seed)
                || is_lakebed_entrance_anchor(x - dx, y - dy, seed)
            {
                return Some(Tile::MineFrame);
            }
        }
    }
    // Rocky halo: surround the entrance with a stone outcrop so it looks
    // like a mineshaft cut into rock instead of standing in grass. Halo
    // is a 7x5 ellipse-ish area around the anchor, minus the frame cells.
    for dx in -3..=3i32 {
        for dy in -3..=1i32 {
            let in_frame = (-1..=1).contains(&dx) && (-1..=0).contains(&dy);
            if in_frame {
                continue;
            }
            // skip the cells directly south (player's approach lane)
            if dx.abs() <= 1 && dy == 1 {
                continue;
            }
            let ax = x - dx;
            let ay = y - dy;
            if is_mine_entrance_anchor(ax, ay, seed) {
                return Some(Tile::Rock);
            }
        }
    }
    None
}

/// Cellular cave noise: combines two coarse sines + jitter to carve organic
/// open/closed patches in the mines.
fn cave_open_at(x: i32, y: i32, seed: u32) -> bool {
    let fx = x as f32;
    let fy = y as f32;
    let s1 = (fx * 0.12 + fy * 0.09 + (seed as f32 * 0.0001)).sin();
    let s2 = (fx * 0.07 - fy * 0.11 + (seed as f32 * 0.0003)).sin();
    let s3 = (fx * 0.21 + fy * 0.17 + (seed as f32 * 0.0007)).sin();
    let v = s1 + s2 * 0.7 + s3 * 0.4;
    let jitter = (hash2(x, y, seed.wrapping_add(0xCAFE_5A1A)) as f32 / u32::MAX as f32) * 0.6
        - 0.3;
    v + jitter > -0.2
}

/// True when (x,y) is a mines wall (closed) cell that touches at least one
/// open cell. Ores spawn only here so they hug the borders of wall masses.
fn is_mines_wall_margin(x: i32, y: i32, seed: u32) -> bool {
    if cave_open_at(x, y, seed) {
        return false;
    }
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if cave_open_at(x + dx, y + dy, seed) {
            return true;
        }
    }
    false
}

/// Same idea as `is_mines_wall_margin` but for the inferno's cave_open noise.
fn is_inferno_wall_margin(x: i32, y: i32, seed: u32) -> bool {
    if cave_open_at(x, y, seed) {
        return false;
    }
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if cave_open_at(x + dx, y + dy, seed) {
            return true;
        }
    }
    false
}

/// True when (x,y) is a wall cell with NO open neighbor in any of the 8
/// directions — i.e. fully buried inside a wall mass. Rendered pitch black
/// so the cave reads as solid stone instead of a sea of tiled hashes.
/// Water counts as air here: a neighbor that renders as water (lakebed
/// pools in the mines, hot-spring water spread by the 1-cell render
/// offset, etc.) means the player can see through to this wall, so don't
/// blank it out.
fn is_buried_wall(world: &World, x: i32, y: i32, seed: u32) -> bool {
    if cave_open_at(x, y, seed) {
        return false;
    }
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if cave_open_at(nx, ny, seed) {
                return false;
            }
            if matches!(
                world.get(nx, ny),
                Tile::Water | Tile::MineralWater | Tile::DeepWater | Tile::Lava
            ) {
                return false;
            }
        }
    }
    true
}

fn mineral_pool_at(x: i32, y: i32, seed: u32) -> bool {
    let fx = x as f32;
    let fy = y as f32;
    let s = (fx * 0.18 + fy * 0.15 + (seed as f32 * 0.0011)).sin();
    let t = (fx * 0.09 - fy * 0.22 + (seed as f32 * 0.0013)).cos();
    s + t > 1.4
}

/// Lava pockets in the inferno: noticeably more common than mineral pools.
fn lava_pool_at(x: i32, y: i32, seed: u32) -> bool {
    let fx = x as f32;
    let fy = y as f32;
    let s = (fx * 0.14 + fy * 0.16 + (seed as f32 * 0.0019)).sin();
    let t = (fx * 0.11 - fy * 0.19 + (seed as f32 * 0.0023)).cos();
    s + t > 0.8
}

/// Sand bars under the ocean — rounded patches of light seabed in the deep.
fn seabed_patch_at(x: i32, y: i32, seed: u32) -> bool {
    let fx = x as f32;
    let fy = y as f32;
    let s = (fx * 0.04 + fy * 0.05 + (seed as f32 * 0.0017)).sin();
    let t = (fx * 0.08 - fy * 0.03 + (seed as f32 * 0.0021)).cos();
    s * 0.7 + t * 0.6 > 0.4
}

/// Coral: same 4-cell anchor system as trees, but on the seabed.
fn coral_at(x: i32, y: i32, seed: u32) -> Option<Tile> {
    for dx in -1..=1i32 {
        for dy in 0..=1i32 {
            let ax = x - dx;
            let ay = y - dy;
            if !is_coral_anchor(ax, ay, seed) {
                continue;
            }
            // anchor (dx=0, dy=0) is the trunk; dy=1 above is canopy
            if dx == 0 && dy == 0 {
                return Some(Tile::CoralTrunk);
            }
            if dy == 1 && dx.abs() <= 1 {
                return Some(Tile::CoralCanopy);
            }
        }
    }
    None
}

fn is_coral_anchor(x: i32, y: i32, seed: u32) -> bool {
    // grid-based winner-takes-cell selection so coral never overlaps.
    let gx = x.div_euclid(5);
    let gy = y.div_euclid(3);
    let base = gx * 5;
    let base_y = gy * 3;
    let mut best = u32::MAX;
    let mut best_xy = (base, base_y);
    for cx in base..base + 5 {
        for cy in base_y..base_y + 3 {
            let h = hash2(cx, cy, seed.wrapping_add(0xC0_5A_1A_1A));
            if h < best {
                best = h;
                best_xy = (cx, cy);
            }
        }
    }
    // sparse coral: only ~18% of grid cells actually grow a coral structure
    best_xy == (x, y) && (best % 100) < 18
}

fn village_oak_glyph(x: i32, y: i32) -> Option<(char, Style)> {
    for &(ax, ay) in VILLAGE_OAKS {
        let dx = x - ax;
        let dy = y - ay;
        let anchor_hash = hash2(ax, ay, 0xCACE_F00D);
        // trunk - paired [ ] stacked two rows tall; upper row darker so
        // the trunk reads as receding shadow under the canopy.
        if (dy == 0 || dy == -1) && (dx == 0 || dx == 1) {
            let g = if dx == 0 { '[' } else { ']' };
            let r = 145 + (anchor_hash % 25) as u8;
            let gc = 100 + (anchor_hash % 22) as u8;
            let b = 60 + (anchor_hash % 18) as u8;
            let (r, gc, b) = if dy == -1 {
                (
                    r.saturating_sub(45),
                    gc.saturating_sub(30),
                    b.saturating_sub(20),
                )
            } else {
                (r, gc, b)
            };
            return Some((
                g,
                Style::default()
                    .fg(Color::Rgb(r, gc, b))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        // canopy cells get one of 4 leaf bases per-cell for variety
        let cell_hash = hash2(x, y, anchor_hash.wrapping_add(0x9AAA));
        let base = match cell_hash % 5 {
            0 => (95, 160, 85),   // bright green
            1 => (115, 145, 60),  // yellow-green
            2 => (70, 130, 70),   // deep green
            3 => (135, 155, 70),  // olive
            _ => (90, 145, 95),   // muted teal-green
        };
        // wide canopy
        if (dy == -2 || dy == -3) && (-2..=3).contains(&dx) {
            let g = match cell_hash % 6 {
                0 => '%',
                1 => '@',
                2 => '#',
                3 => '&',
                4 => '*',
                _ => 'o',
            };
            return Some(leaf_style(g, anchor_hash, base, x, y));
        }
        // top canopy
        if dy == -4 && (-1..=2).contains(&dx) {
            let g = match dx {
                -1 => '/',
                2 => '\\',
                _ => match cell_hash % 3 {
                    0 => '#',
                    1 => '%',
                    _ => '&',
                },
            };
            return Some(leaf_style(g, anchor_hash, base, x, y));
        }
    }
    None
}

fn tree_render(x: i32, y: i32, seed: u32) -> (char, Style) {
    let Some((ax, ay, sp, part)) = find_tree_anchor(x, y, seed) else {
        return (' ', Style::default());
    };
    let anchor_hash = hash2(ax, ay, seed.wrapping_add(0xAA55_AA55));
    match (sp, part) {
        (TreeSpecies::Round, TreePart::Trunk) => trunk_style(anchor_hash, '|'),
        (TreeSpecies::Round, TreePart::Canopy) => {
            let dx = x - ax;
            let g = match dx {
                -1 => match anchor_hash & 1 {
                    0 => '(',
                    _ => 'C',
                },
                1 => match anchor_hash & 1 {
                    0 => ')',
                    _ => 'O',
                },
                _ => match anchor_hash & 1 {
                    0 => 'O',
                    _ => '8',
                },
            };
            leaf_style(g, anchor_hash, (90, 145, 80), x, y)
        }
        (TreeSpecies::Pine, TreePart::Trunk) => trunk_style(anchor_hash, 'I'),
        (TreeSpecies::Pine, TreePart::Canopy) => {
            let dx = x - ax;
            let dy = y - ay;
            let g = if dy == -2 {
                '^'
            } else if dy == -1 {
                if dx == 0 {
                    'A'
                } else {
                    '/'
                }
            } else {
                '/'
            };
            leaf_style(g, anchor_hash, (70, 125, 75), x, y)
        }
        (TreeSpecies::Bush, _) => {
            let g = match anchor_hash % 3 {
                0 => 'o',
                1 => '*',
                _ => 'q',
            };
            leaf_style(g, anchor_hash, (115, 150, 85), x, y)
        }
    }
}

fn trunk_style(anchor_hash: u32, g: char) -> (char, Style) {
    let r = 130 + (anchor_hash % 30) as u8;
    let gc = 88 + (anchor_hash % 22) as u8;
    let b = 55 + (anchor_hash % 18) as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
}

fn leaf_style(g: char, anchor_hash: u32, base: (u8, u8, u8), x: i32, y: i32) -> (char, Style) {
    let tint_r = (anchor_hash % 18) as i32 - 9;
    let tint_g = ((anchor_hash >> 8) % 18) as i32 - 9;
    let tint_b = ((anchor_hash >> 16) % 18) as i32 - 9;
    let base = (
        (base.0 as i32 + tint_r).clamp(0, 255) as u8,
        (base.1 as i32 + tint_g).clamp(0, 255) as u8,
        (base.2 as i32 + tint_b).clamp(0, 255) as u8,
    );
    (g, Style::default().fg(shade(base, x, y, 0xAA55_AA56, 10)))
}

fn rock_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xF00D_F00D) % 2;
    let (g, base) = match v {
        0 => ('o', (121, 121, 121)),
        _ => ('O', (143, 143, 143)),
    };
    (g, Style::default().fg(shade(base, x, y, 0xF00D_F00D, 12)))
}

fn is_medium_rock_anchor(x: i32, y: i32, seed: u32, density: f32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if cached_water_body_at(x, y, seed) {
        return false;
    }
    let h = hash2(x, y, seed.wrapping_add(0xDEAF_BEAD)) as f32 / u32::MAX as f32;
    h < density
}

fn medium_rock_at(x: i32, y: i32, seed: u32, density: f32) -> bool {
    is_medium_rock_anchor(x, y, seed, density)
        || is_medium_rock_anchor(x - 1, y, seed, density)
}

fn medium_rock_glyph(x: i32, y: i32, seed: u32) -> (char, Style) {
    let p = biome_params(cached_biome_at(x, y, seed));
    let density = p.medium_rock;
    let (anchor_x, _) = if is_medium_rock_anchor(x, y, seed, density) {
        (x, y)
    } else {
        (x - 1, y)
    };
    let dx = x - anchor_x;
    let template = hash2(anchor_x, y, 0xC0DE_BABE) % 2;
    let g = match (template, dx) {
        (0, 0) => '[',
        (0, _) => ']',
        (_, 0) => '/',
        (_, _) => '\\',
    };
    let base = (130, 130, 130);
    (g, Style::default().fg(shade(base, anchor_x, y, 0xDEAF_BEAD, 10)))
}

fn pebble_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xABCD_1234) % 3;
    let g = match v {
        0 => '.',
        1 => ',',
        _ => '`',
    };
    (g, Style::default().fg(shade((127, 116, 99), x, y, 0xABCD_1234, 15)))
}

fn flower_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xFFEE_DD11) % 3;
    let color = match h {
        0 => Color::Rgb(198, 193, 165),
        1 => Color::Rgb(187, 165, 143),
        _ => Color::Rgb(176, 154, 165),
    };
    ('*', Style::default().fg(color))
}

fn big_rock_glyph(x: i32, y: i32, seed: u32) -> (char, Style) {
    let mut anchor = (0, 0);
    let mut found = false;
    'find: for dy in 0..2i32 {
        for dx in 0..2i32 {
            let ax = x - dx;
            let ay = y - dy;
            let density = biome_params(cached_biome_at(ax, ay, seed)).big_rock;
            if is_big_rock_anchor(ax, ay, seed, density) {
                anchor = (ax, ay);
                found = true;
                break 'find;
            }
        }
    }
    if !found {
        return ('#', Style::default().fg(Color::Rgb(120, 120, 120)));
    }
    let _ = (x, y);
    let g = '#';
    let shade = hash2(anchor.0, anchor.1, 0xCAFE_BABE) % 40;
    let base = 121 + shade as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(base, base, base))
            .add_modifier(Modifier::BOLD),
    )
}

fn village_tile(x: i32, y: i32) -> Option<Tile> {
    // perimeter walls first - they may overlap house corners visually
    if let Some(t) = village_perimeter(x, y) {
        return Some(t);
    }

    // house definitions: (x_range, y_range, door_xy, door_kind)
    // each house is 5 wide x 3 tall (visual ~5 wide x 3 tall = ~5x6 since cells are 2:1)
    type DoorKind = Tile;
    let houses: &[((i32, i32), (i32, i32), (i32, i32), DoorKind)] = &[
        ((-37, -33), (-1, 1), (-35, 1), Tile::DoorHouse),
        ((-20, -16), (-1, 1), (-18, 1), Tile::DoorRod),    // rod shop
        ((-2, 2), (-5, -3), (0, -3), Tile::DoorHouse),
        ((-25, -21), (-7, -5), (-23, -5), Tile::DoorHouse),
        ((21, 25), (-7, -5), (23, -5), Tile::DoorHouse),
        ((16, 20), (-1, 1), (18, 1), Tile::DoorSchool),    // fishing school
        ((33, 37), (-1, 1), (35, 1), Tile::DoorHouse),
    ];

    for &((xa, xb), (ya, yb), (dx, dy), dkind) in houses {
        if (xa..=xb).contains(&x) && (ya..=yb).contains(&y) {
            if (x, y) == (dx, dy) {
                return Some(dkind);
            }
            if y == ya {
                return Some(Tile::Roof);
            }
            return Some(Tile::Wall);
        }
    }

    // pier and well
    if pier_cell(x, y) {
        return Some(Tile::Dock);
    }
    if (x, y) == (0, -1) {
        return Some(Tile::Well);
    }
    // hand-placed oaks
    if let Some(t) = village_oak_at(x, y) {
        return Some(t);
    }
    // lampposts and benches
    if let Some(t) = village_decor(x, y) {
        return Some(t);
    }
    // pathways inside the walls
    if village_path(x, y) {
        return Some(Tile::Path);
    }
    None
}

// anchor positions chosen so the 4-tall canopy (dy=-4) stays clear of
// the top wall (y=-17) and the trunk (dy=0) stays clear of the bottom
// wall row (y=4). horizontal span dx in [-2, 3] kept clear of walls and
// houses too.
const VILLAGE_OAKS: &[(i32, i32)] = &[
    (-44, 3), (44, 3),
    (-30, 3), (30, 3),
    // Was (-14, 3) — its 5-wide canopy at y=1,0 covered the home
    // Blacksmith at (-12, 1) and Smelter at (-12, 0). Shifted east so
    // the canopy ends at x=-5 and the smithy is in the clear.
    (-8, 3),  (14, 3),
    (-40, -10), (40, -10),
    (-12, -10), (12, -10),
];

fn village_oak_at(x: i32, y: i32) -> Option<Tile> {
    for &(ax, ay) in VILLAGE_OAKS {
        let dx = x - ax;
        let dy = y - ay;
        // trunk: 2 wide, 2 tall (rows 0 and -1)
        if (dy == 0 || dy == -1) && (dx == 0 || dx == 1) {
            return Some(Tile::TreeTrunk);
        }
        // wide canopy: 5 wide, rows -2 and -3
        if (dy == -2 || dy == -3) && (-2..=3).contains(&dx) {
            return Some(Tile::TreeCanopy);
        }
        // top canopy: 3 wide, row -4
        if dy == -4 && (-1..=2).contains(&dx) {
            return Some(Tile::TreeCanopy);
        }
    }
    None
}

const VILLAGE_LAMPS: &[(i32, i32)] = &[
    (-3, -5), (3, -5),
    (-3, 3), (3, 3),
    (-15, -2), (15, -2),
    (-15, 2), (15, 2),
    (-30, -2), (30, -2),
    (-30, 2), (30, 2),
];

const VILLAGE_BENCHES: &[(i32, i32)] = &[
    (-4, 0), (4, 0),
    (-4, 1), (4, 1),
];

fn village_decor(x: i32, y: i32) -> Option<Tile> {
    if VILLAGE_LAMPS.contains(&(x, y)) {
        return Some(Tile::Lamppost);
    }
    if VILLAGE_BENCHES.contains(&(x, y)) {
        return Some(Tile::Bench);
    }
    None
}

// village wall geometry (matches the user's model):
//   y = -9       shadow row of underscores (no corners)
//   y = -8       top edge: / _____ \
//   y = -7..=3   side columns: || on each side, interior empty
//   y =  4       bottom cap row: || ___ ||
//   y =  5       bottom edge: \ _____ /
// dock gap punches a hole in the bottom two rows for x in [-6, 5]
const WALL_L_OUT: i32 = -50;
const WALL_L_IN: i32 = -49;
const WALL_R_IN: i32 = 49;
const WALL_R_OUT: i32 = 50;
const WALL_TOP_SHADOW: i32 = -18;
const WALL_TOP_EDGE: i32 = -17;
const WALL_BOT_CAP: i32 = 4;
const WALL_BOT_EDGE: i32 = 5;

fn dock_gap_x(x: i32) -> bool {
    (-3..=4).contains(&x)
}

fn pier_cell(x: i32, y: i32) -> bool {
    // main column 8 wide x 8 deep
    if (-3..=4).contains(&x) && (5..=12).contains(&y) {
        return true;
    }
    // left arm at far end - 3 tall
    if (-13..=-4).contains(&x) && (10..=12).contains(&y) {
        return true;
    }
    // right arm at far end - 3 tall
    if (5..=14).contains(&x) && (10..=12).contains(&y) {
        return true;
    }
    false
}

fn north_gate_x(x: i32) -> bool {
    (-2..=2).contains(&x)
}

fn side_gate_y(y: i32) -> bool {
    (-9..=-7).contains(&y)
}

fn village_perimeter(x: i32, y: i32) -> Option<Tile> {
    let in_box_x = x >= WALL_L_OUT && x <= WALL_R_OUT;
    let in_side_y = y >= WALL_TOP_EDGE && y <= WALL_BOT_CAP;

    // top shadow row (no corners), skip north gate
    if y == WALL_TOP_SHADOW
        && x >= WALL_L_IN
        && x <= WALL_R_IN
        && !north_gate_x(x)
    {
        return Some(Tile::Wall);
    }
    // top edge row, skip north gate
    if y == WALL_TOP_EDGE && in_box_x && !north_gate_x(x) {
        return Some(Tile::Wall);
    }
    // side columns, skip east/west gates
    if in_side_y && (x == WALL_L_OUT || x == WALL_L_IN || x == WALL_R_IN || x == WALL_R_OUT) {
        if side_gate_y(y) {
            return None;
        }
        return Some(Tile::Wall);
    }
    // bottom cap and bottom edge skip dock gap
    if y == WALL_BOT_CAP && in_box_x && !dock_gap_x(x) {
        return Some(Tile::Wall);
    }
    if y == WALL_BOT_EDGE && in_box_x && !dock_gap_x(x) {
        return Some(Tile::Wall);
    }
    None
}

fn village_path(x: i32, y: i32) -> bool {
    // central square (paved area around the well)
    if (-4..=4).contains(&x) && (-3..=2).contains(&y) {
        return true;
    }
    // north corridor: from gate down to square
    if (-2..=2).contains(&x) && (-16..=-3).contains(&y) {
        return true;
    }
    // south corridor: from square to dock gap
    if (-3..=3).contains(&x) && (3..=4).contains(&y) {
        return true;
    }
    // east corridor: from gate to square
    if (5..=48).contains(&x) && (-1..=1).contains(&y) {
        return true;
    }
    // west corridor: from gate to square
    if (-48..=-5).contains(&x) && (-1..=1).contains(&y) {
        return true;
    }
    // small spurs to each house door
    // rod shop door (-33, 1) -> west corridor connects already
    // school door (33, 1) -> east corridor connects
    // inn door (-18, 1), cottage (18, 1): connected via central corridors
    // bakery (0, -3) spur from north corridor
    false
}

fn perimeter_glyph(x: i32, y: i32) -> Option<(char, Style)> {
    let style = || {
        Style::default()
            .fg(Color::Rgb(160, 130, 90))
            .add_modifier(Modifier::BOLD)
    };
    let g = match (x, y) {
        // top shadow row: only underscores
        (_, WALL_TOP_SHADOW) => '_',
        // top edge corners
        (WALL_L_OUT, WALL_TOP_EDGE) => '/',
        (WALL_R_OUT, WALL_TOP_EDGE) => '\\',
        (_, WALL_TOP_EDGE) => '_',
        // bottom cap: pipes in the wall-thickness columns, underscores between
        (WALL_L_OUT, WALL_BOT_CAP) | (WALL_L_IN, WALL_BOT_CAP) => '|',
        (WALL_R_IN, WALL_BOT_CAP) | (WALL_R_OUT, WALL_BOT_CAP) => '|',
        (_, WALL_BOT_CAP) => '_',
        // bottom edge corners
        (WALL_L_OUT, WALL_BOT_EDGE) => '\\',
        (WALL_R_OUT, WALL_BOT_EDGE) => '/',
        (_, WALL_BOT_EDGE) => '_',
        // side columns (everything else under perimeter)
        _ => '|',
    };
    Some((g, style()))
}

const WELL_CELL: i32 = 60;

fn well_at(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if cached_water_body_at(x, y, seed) {
        return false;
    }
    let cx = x.div_euclid(WELL_CELL);
    let cy = y.div_euclid(WELL_CELL);
    let h = hash2(cx, cy, seed.wrapping_add(0xDEAD_BABE));
    // ~25% of WELL_CELL cells host a well
    if h % 4 != 0 {
        return false;
    }
    let ox = ((h >> 4) as i32).rem_euclid(WELL_CELL);
    let oy = ((h >> 12) as i32).rem_euclid(WELL_CELL);
    cx * WELL_CELL + ox == x && cy * WELL_CELL + oy == y
}

fn jitter(x: i32, y: i32, salt: u32, range: i32) -> i32 {
    let h = hash2(x, y, salt);
    (h as i32 % (range * 2 + 1)) - range
}

fn shade(base: (u8, u8, u8), x: i32, y: i32, salt: u32, range: i32) -> Color {
    let dr = jitter(x, y, salt, range);
    let dg = jitter(x, y, salt.wrapping_add(1), range);
    let db = jitter(x, y, salt.wrapping_add(2), range);
    Color::Rgb(
        (base.0 as i32 + dr).clamp(0, 255) as u8,
        (base.1 as i32 + dg).clamp(0, 255) as u8,
        (base.2 as i32 + db).clamp(0, 255) as u8,
    )
}

fn water_anim(x: i32, y: i32, tick: u64) -> (char, Style) {
    let t = tick as f32 * 0.012;
    let fx = x as f32;
    let fy = y as f32;
    // sines give flow direction, the noise layer changes phase slowly so the
    // shimmer reads as moving water without strobing.
    let w1 = (fx * 0.731 + fy * 1.117 + t * 1.27).sin() * 0.4;
    let w2 = (fx * 1.289 - fy * 0.583 + t * 0.94).sin() * 0.3;
    let slow_noise =
        (hash2(x, y, 0xA11_BABE) as f32 / u32::MAX as f32 - 0.5) * 1.2;
    let fast_noise = (hash2(
        x.wrapping_add((tick as i32 / 14).wrapping_mul(7919)),
        y.wrapping_add((tick as i32 / 18).wrapping_mul(6113)),
        0xBAD_C0DE,
    ) as f32
        / u32::MAX as f32
        - 0.5)
        * 1.6;
    let h = w1 + w2 + slow_noise + fast_noise;
    let (glyph, base) = if h > 1.6 {
        ('~', (110, 135, 155))
    } else if h > 0.8 {
        ('~', (85, 110, 135))
    } else if h > 0.2 {
        ('-', (70, 90, 115))
    } else if h > -0.4 {
        ('-', (60, 80, 105))
    } else if h > -1.0 {
        ('.', (50, 70, 95))
    } else if h > -1.6 {
        (',', (40, 60, 85))
    } else {
        ('`', (30, 50, 75))
    };
    (
        glyph,
        Style::default()
            .fg(shade(base, x, y, 0xA11_BABE, 6))
            .bg(Color::Rgb(0, 2, 16)),
    )
}

fn grass_anim(x: i32, y: i32, _tick: u64, biome: Biome) -> (char, Style) {
    let base = match biome {
        Biome::Meadow => (72, 116, 72),
        Biome::Forest => (50, 88, 55),
        Biome::Rocky => (105, 110, 77),
        Biome::Scrub => (121, 116, 83),
        Biome::Desert => (187, 160, 105),
        Biome::Tundra => (187, 193, 193),
        Biome::Swamp => (66, 83, 55),
    };
    ('.', Style::default().fg(shade(base, x, y, 0x6C00_6C00, 14)))
}

fn wall_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0x1A11_F00D) % 4;
    let g = match v {
        0 => '|',
        1 => 'H',
        2 => '|',
        _ => '#',
    };
    (
        g,
        Style::default()
            .fg(shade((125, 90, 55), x, y, 0x1A11_F00D, 12))
            .add_modifier(Modifier::BOLD),
    )
}

/// Per-house wall variants. The house's door coords seed the variant
/// so every house renders with a distinct style — timber, brick, stone,
/// adobe, driftwood, etc. Cell-level noise still adds within-house
/// variation; the house seed only picks the palette + glyph family.
fn wall_glyph_for_house(x: i32, y: i32, door: (i32, i32)) -> (char, Style) {
    const FAMILIES: &[(&[char], (u8, u8, u8))] = &[
        // 0 — warm timber (vertical planks)
        (&['|', 'I', '|', '|'], (140, 95, 55)),
        // 1 — brick + mortar
        (&['#', '=', '#', 'H'], (155, 85, 70)),
        // 2 — fieldstone (jumbled stones)
        (&['o', 'O', '@', '0'], (140, 130, 115)),
        // 3 — adobe/sandstone
        (&['#', '%', '#', '#'], (190, 150, 95)),
        // 4 — bleached driftwood
        (&['|', '/', '\\', '|'], (180, 165, 140)),
        // 5 — dark slate
        (&['H', 'M', 'H', '#'], (80, 90, 110)),
        // 6 — mossy green-tinted
        (&['#', 'V', 'Y', '#'], (95, 130, 80)),
        // 7 — whitewashed cottage
        (&['|', ':', '|', '|'], (215, 210, 195)),
    ];
    let h = hash2(door.0, door.1, 0x5E5E_C0DE);
    let fam = &FAMILIES[(h as usize) % FAMILIES.len()];
    let g = fam.0[(hash2(x, y, h.wrapping_add(0xC011_C011)) as usize) % fam.0.len()];
    (
        g,
        Style::default()
            .fg(shade(fam.1, x, y, h ^ 0x1A11_F00D, 12))
            .add_modifier(Modifier::BOLD),
    )
}

fn roof_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0x720F_720F) % 3;
    let g = match v {
        0 => '#',
        1 => '%',
        _ => '#',
    };
    (
        g,
        Style::default()
            .fg(shade((160, 75, 55), x, y, 0x720F_720F, 12))
            .add_modifier(Modifier::BOLD),
    )
}

/// Per-house roof palettes. Same seeding scheme as wall_glyph_for_house.
fn roof_glyph_for_house(x: i32, y: i32, door: (i32, i32)) -> (char, Style) {
    const PALETTES: &[(&[char], (u8, u8, u8))] = &[
        // 0 — red clay tile
        (&['#', '%', '#'], (170, 70, 50)),
        // 1 — grey shingle
        (&['#', '=', '#'], (110, 110, 120)),
        // 2 — slate blue
        (&['#', '~', '#'], (75, 95, 130)),
        // 3 — mossy green
        (&['%', '#', '%'], (90, 130, 70)),
        // 4 — straw thatch
        (&['/', '\\', '/'], (200, 165, 95)),
        // 5 — dark tar
        (&['#', '#', '%'], (55, 50, 55)),
        // 6 — sun-bleached cream
        (&['#', '%', '#'], (220, 200, 160)),
        // 7 — copper-green patina
        (&['~', '%', '#'], (95, 150, 130)),
    ];
    let h = hash2(door.0, door.1, 0x720F_720F);
    let pal = &PALETTES[(h as usize) % PALETTES.len()];
    let g = pal.0[(hash2(x, y, h.wrapping_add(0xA10B_C0DE)) as usize) % pal.0.len()];
    (
        g,
        Style::default()
            .fg(shade(pal.1, x, y, h, 12))
            .add_modifier(Modifier::BOLD),
    )
}

/// Chimney location for a house — picked by hashing the door coords,
/// always sitting on one of the roof cells. Returns true if (x, y) is
/// the chimney cell.
fn house_chimney_at(x: i32, y: i32, door: (i32, i32)) -> bool {
    // door.0 is the door's x; door.1 is door y. The roof line is y == ya
    // (the top of the house). We don't know ya precisely without scanning,
    // but every house's roof is at door.y - 4 .. door.y - 1 across the
    // known layouts. Pick a chimney offset within (-2..=+2) of the door
    // column, on a roof row two cells above the door.
    let h = hash2(door.0, door.1, 0xCD11_25E5);
    let off = ((h as i32) % 5) - 2; // -2..=2
    let chimney_y = door.1 - 2;
    x == door.0 + off && y == chimney_y
}

/// Chimney render: thin tall glyph with a tiny smoke wisp tinted by the
/// house's roof palette.
fn chimney_glyph(_x: i32, _y: i32, door: (i32, i32)) -> (char, Style) {
    let h = hash2(door.0, door.1, 0xCD11_25E5);
    let g = match h % 3 {
        0 => 'M',
        1 => 'I',
        _ => 'H',
    };
    let palette_h = hash2(door.0, door.1, 0x720F_720F);
    let (r, g_c, b): (u8, u8, u8) = match palette_h % 8 {
        0 => (110, 50, 40),
        1 => (80, 75, 85),
        2 => (60, 65, 85),
        3 => (70, 90, 55),
        4 => (130, 105, 65),
        5 => (50, 40, 45),
        6 => (150, 130, 105),
        _ => (70, 100, 85),
    };
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, g_c, b))
            .add_modifier(Modifier::BOLD),
    )
}

/// Door-seed lookup for any house on the Surface. Returns the door's
/// (x, y) — stable per house — when (x, y) sits inside any home-village
/// or procedural-village hut. Used by render to give every house its
/// own wall + roof palette.
///
/// Early-outs aggressively: most surface tiles aren't in a house, so we
/// fast-fail before doing the village_anchor_for hash sweep.
pub fn house_seed_at(qx: i32, qy: i32, seed: u32) -> Option<(i32, i32)> {
    // Home-village houses, mirroring village_tile's table.
    const HOME_HOUSES: &[((i32, i32, i32, i32), (i32, i32))] = &[
        ((-37, -33, -1, 1), (-35, 1)),
        ((-20, -16, -1, 1), (-18, 1)),
        ((-2, 2, -5, -3), (0, -3)),
        ((-25, -21, -7, -5), (-23, -5)),
        ((21, 25, -7, -5), (23, -5)),
        ((16, 20, -1, 1), (18, 1)),
        ((33, 37, -1, 1), (35, 1)),
    ];
    for &((xa, xb, ya, yb), door) in HOME_HOUSES {
        if (xa..=xb).contains(&qx) && (ya..=yb).contains(&qy) {
            return Some(door);
        }
    }
    // Procedural villages — only relevant when qy is within reach of any
    // proc village. Anchors are at ay <= -8 with radius <= 35, so houses
    // can occupy roughly y in [ay-7, ay+7]. If qy is way south of that
    // (e.g. qy > 8 in the ocean), skip the anchor sweep.
    if qy > 8 {
        return None;
    }
    let v = village_anchor_for(qx, qy, seed)?;
    let dx = qx - v.ax;
    let dy = qy - v.ay;
    let huts: &[((i32, i32), (i32, i32), (i32, i32))] = match v.kind {
        VillageKind::Hamlet => &[
            ((-10, -7), (-6, -5), (-8, -5)),
            ((6, -7), (10, -5), (8, -5)),
            ((-2, 5), (2, 7), (0, 7)),
        ],
        VillageKind::Town => &[
            ((-15, -7), (-11, -5), (-13, -5)),
            ((-5, -7), (-1, -5), (-3, -5)),
            ((5, -7), (9, -5), (7, -5)),
            ((-9, 5), (-5, 7), (-7, 7)),
            ((5, 5), (9, 7), (7, 7)),
        ],
        VillageKind::Floating => &[
            ((-16, -1), (-12, 1), (-12, 1)),
            ((12, -1), (16, 1), (12, 1)),
            ((-1, -16), (1, -12), (0, -12)),
            ((-1, 12), (1, 16), (0, 12)),
        ],
    };
    for &((xa, ya), (xb, yb), (ddx, ddy)) in huts {
        if (xa..=xb).contains(&dx) && (ya..=yb).contains(&dy) {
            return Some((v.ax + ddx, v.ay + ddy));
        }
    }
    None
}

fn cactus_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xCAC7_F00D) % 3;
    let g = match v {
        0 => 'Y',
        1 => 'T',
        _ => 'i',
    };
    (
        g,
        Style::default()
            .fg(shade((94, 132, 72), x, y, 0xCAC7_F00D, 10))
            .add_modifier(Modifier::BOLD),
    )
}

/// Two-row shore wave. `row`: 0 = sand row (upper), 1 = water row (lower).
/// Foam splashes spawn at random shore points at random intervals. Each splash
/// is a chaotic asymmetric blob of multiple foam glyphs in shades of white that
/// fade over ~22 ticks. Between splashes the shore is calm.
fn shore_anim(x: i32, row: i32, tick: u64) -> (char, Style) {
    if let Some(s) = shore_splash(x, row, tick) {
        return s;
    }
    // calm fallback
    if row == 1 {
        water_anim(x, 6, tick)
    } else {
        let g = match hash2(x, 0, 0x5A1D_5A1D) % 3 {
            0 => ',',
            1 => '.',
            _ => '`',
        };
        (
            g,
            Style::default().fg(shade((180, 165, 120), x, 0, 0x5A1D_5A1D, 14)),
        )
    }
}

fn shore_splash(x: i32, row: i32, tick: u64) -> Option<(char, Style)> {
    const SPAWN_EVERY: u64 = 6;
    const REACH: i32 = 4;
    // sand row dies fast so the foam appears to RETREAT toward the water row
    let lifetime: u64 = if row == 0 { 32 } else { 70 };
    let look_back = if row == 1 { lifetime } else { lifetime + 8 };

    let mut active: Option<(u64, i32, u32)> = None;
    let earliest = tick.saturating_sub(look_back);
    let mut t = (earliest / SPAWN_EVERY) * SPAWN_EVERY;
    while t <= tick {
        for dx in -REACH..=REACH {
            let ax = x - dx;
            let h = hash2(ax, t as i32, 0xFA0A_FA0A);
            if h % 70 != 0 {
                continue;
            }
            let age = tick - t;
            if age > lifetime {
                continue;
            }
            // splash reach shrinks as it retreats (only the heart of the wave lingers)
            let max_reach = ((h >> 4) as i32 % 3 + 2).abs();
            let life_frac = age as f32 / lifetime as f32;
            let cur_reach = (max_reach as f32 * (1.0 - life_frac * 0.6)).round() as i32;
            if dx.abs() > cur_reach {
                continue;
            }
            if row == 0 {
                let extend = ((h >> 8) % 2) == 0;
                if !extend {
                    continue;
                }
            }
            if let Some((cur_t, _, _)) = active {
                if t < cur_t {
                    continue;
                }
            }
            active = Some((t, ax, h));
        }
        t = t.saturating_add(SPAWN_EVERY);
    }

    let (spawn_t, anchor_x, _anchor_hash) = active?;
    let age = tick - spawn_t;
    let intensity = 1.0 - (age as f32 / lifetime as f32);

    // glyph swap every 7 ticks per cell -> calmer roil
    let local_dx = x - anchor_x;
    let ch_hash = hash2(
        x.wrapping_add(local_dx * 7),
        (tick / 7) as i32,
        0xCAFE_F00D,
    );
    let glyph = match ch_hash % 8 {
        0 => '*',
        1 => 'o',
        2 => '.',
        3 => ',',
        4 => '`',
        5 => ':',
        6 => '\'',
        _ => '"',
    };

    let lum = (140.0 + intensity * 110.0).clamp(0.0, 255.0) as u8;
    let color = match ch_hash % 4 {
        0 => Color::Rgb(lum, lum, lum),
        1 => Color::Rgb(lum, lum.saturating_sub(8), lum.saturating_sub(20)),
        2 => Color::Rgb(
            lum.saturating_sub(15),
            lum.saturating_sub(8),
            lum,
        ),
        _ => Color::Rgb(lum, lum.saturating_sub(4), lum.saturating_sub(10)),
    };
    Some((glyph, Style::default().fg(color).add_modifier(Modifier::BOLD)))
}

/// Cell offsets (relative to anchor at 0,0) of the 11 stones that make up
/// the 5w × 4h portal arch. Anchor sits at the bottom-center; player
/// approaches from (0, +1) and presses `f` to travel.
///
///     ╔═══╗     y=-3
///     ║   ║     y=-2
///     ║   ║     y=-1
///     ║ A ║     y= 0  (A = anchor)
const PORTAL_FRAME_OFFSETS: &[(i32, i32)] = &[
    (-2, -3), (-1, -3), (0, -3), (1, -3), (2, -3),
    (-2, -2), (2, -2),
    (-2, -1), (2, -1),
    (-2, 0),  (2, 0),
];

/// Hash + biome candidate check for a procedural surface portal. Does NOT
/// validate the surrounding 5x4 footprint — that's `dim_portal_for`'s job
/// once we've decided the cell is a plausible anchor.
fn dim_portal_candidate(x: i32, y: i32, seed: u32) -> Option<Dimension> {
    if in_village_zone(x, y) {
        return None;
    }
    if y >= 4 {
        return None;
    }
    if cached_water_body_at(x, y, seed) {
        return None;
    }
    let b = cached_biome_at(x, y, seed);
    let h = hash2(x, y, seed.wrapping_add(0xD17F_02A1));
    if h % 45000 == 13 && matches!(b, Biome::Desert) {
        return Some(Dimension::Pyramid);
    }
    if h % 55000 == 23 && matches!(b, Biome::Desert | Biome::Scrub) {
        return Some(Dimension::HotSpring);
    }
    if h % 45000 == 17 && matches!(b, Biome::Tundra) {
        return Some(Dimension::Iceshelf);
    }
    if h % 50000 == 19 && matches!(b, Biome::Swamp) {
        return Some(Dimension::SwampCave);
    }
    if h % 60000 == 7 && matches!(b, Biome::Swamp) {
        return Some(Dimension::BogCathedral);
    }
    if h % 55000 == 31 && matches!(b, Biome::Meadow | Biome::Forest) {
        return Some(Dimension::MirrorLake);
    }
    if h % 80000 == 41 {
        return Some(Dimension::Crater);
    }
    if h % 105000 == 51 {
        return Some(Dimension::Colosseum);
    }
    None
}

/// Returns the destination dim if this surface cell is a portal anchor.
/// Sparse hash-gated per dim, with biome filters where it makes sense.
/// Procedural portals additionally require their full 5x4 stone-arch
/// footprint plus southern approach lane to sit on clear land — without
/// this the structure clips into water/villages. None for cells that
/// aren't a portal.
pub fn dim_portal_for(x: i32, y: i32, seed: u32) -> Option<Dimension> {
    // Bespoke first — these escape the village/water early-returns and
    // don't get a stone arch (they're flavor manholes / ocean rifts).
    if (x, y) == SEWER_PORTAL_XY {
        return Some(Dimension::Sewer);
    }
    if (x, y) == WRECKAGE_PORTAL_XY {
        return Some(Dimension::Wreckage);
    }
    let dest = dim_portal_candidate(x, y, seed)?;
    // Whole footprint must be on land, outside any village.
    for &(dx, dy) in PORTAL_FRAME_OFFSETS {
        let fx = x + dx;
        let fy = y + dy;
        if cached_water_body_at(fx, fy, seed) {
            return None;
        }
        if in_village_zone(fx, fy) {
            return None;
        }
    }
    // Player must be able to walk up to the anchor from the south.
    if cached_water_body_at(x, y + 1, seed) {
        return None;
    }
    Some(dest)
}

/// True if `(x, y)` is a procedural arch-style portal anchor — i.e. a
/// validated `dim_portal_for` hit that isn't one of the bespoke fixed
/// coords. Used by `portal_frame_at` so the bespoke manhole/wreckage
/// portals don't grow stone arches.
fn arched_portal_at(x: i32, y: i32, seed: u32) -> Option<Dimension> {
    if (x, y) == SEWER_PORTAL_XY || (x, y) == WRECKAGE_PORTAL_XY {
        return None;
    }
    dim_portal_for(x, y, seed)
}

/// If `(x, y)` is a frame stone of some arched portal, return the
/// destination dim and the cell's (dx, dy) offset from the anchor.
fn portal_frame_at(x: i32, y: i32, seed: u32) -> Option<(Dimension, i32, i32)> {
    for &(dx, dy) in PORTAL_FRAME_OFFSETS {
        if let Some(dest) = arched_portal_at(x - dx, y - dy, seed) {
            return Some((dest, dx, dy));
        }
    }
    None
}

fn portal_frame_color(dest: Dimension) -> Color {
    match dest {
        Dimension::Pyramid => Color::Rgb(190, 150, 90),
        Dimension::HotSpring => Color::Rgb(180, 140, 160),
        Dimension::Iceshelf => Color::Rgb(170, 205, 230),
        Dimension::SwampCave => Color::Rgb(95, 135, 85),
        Dimension::BogCathedral => Color::Rgb(115, 110, 135),
        Dimension::MirrorLake => Color::Rgb(195, 205, 230),
        Dimension::Crater => Color::Rgb(155, 130, 200),
        Dimension::Colosseum => Color::Rgb(220, 215, 200),
        _ => Color::Rgb(170, 160, 165),
    }
}

fn portal_frame_glyph(x: i32, y: i32, seed: u32) -> (char, Style) {
    let (dest, dx, dy) = match portal_frame_at(x, y, seed) {
        Some(t) => t,
        None => return ('#', Style::default().fg(Color::Rgb(150, 145, 140))),
    };
    let g = match (dx, dy) {
        (-2, -3) => '╔',
        (2, -3) => '╗',
        (_, -3) => '═',
        _ => '║',
    };
    (
        g,
        Style::default()
            .fg(portal_frame_color(dest))
            .add_modifier(Modifier::BOLD),
    )
}

// ---- specialty dim generators ---------------------------------------------
//
// Each `<dim>_get(x, y)` returns the tile at world-coords (x, y) inside that
// dim. Minimum-viable but distinct procedural layouts. Wall interiors
// render pitch black via the existing is_buried_wall logic in render_tile.

// ---- Labyrinth-of-rooms primitive -----------------------------------------
//
// Each macro cell (M × M) hosts one *anchor* placed at a hash-derived offset
// inside the cell. The anchor carries a recipe (shape kind + dimensions)
// so adjacent macro cells produce wildly different rooms — small squares
// next to long corridors next to round chambers next to tall halls.
// L-corridors connect every anchor to its right + lower neighbour so the
// whole labyrinth is reachable.

#[derive(Clone, Copy)]
pub(super) enum LabCell {
    /// Cell sits inside a room. `ax, ay` = room centre, `hash` = its recipe.
    Room { ax: i32, ay: i32, hash: u32 },
    /// Cell sits on a corridor between two anchors.
    Corridor,
    /// Cell is solid wall.
    Wall,
}

fn macro_anchor(mcx: i32, mcy: i32, macro_size: i32, seed: u32) -> (i32, i32, u32) {
    let h = hash2(mcx, mcy, seed.wrapping_add(0xA17C_8081));
    let usable = macro_size - 4; // keep anchors a bit away from cell edges
    let off_x = 2 + (h as i32).rem_euclid(usable);
    let off_y = 2 + ((h >> 8) as i32).rem_euclid(usable);
    (mcx * macro_size + off_x, mcy * macro_size + off_y, h)
}

/// Decode a recipe hash into a room shape. Returns
/// (half_width_in_world_cells, half_height_in_world_cells, is_round).
///
/// Terminal cells are ~2:1 tall, so for any shape that should *look*
/// square or round on screen the horizontal half-extent is twice the
/// vertical half-extent.
fn room_shape(h: u32, macro_size: i32) -> (i32, i32, bool) {
    let max_hw = macro_size - 2;       // wide cap (we have lots of horizontal room)
    let max_hh = macro_size / 2 - 1;   // half the cap vertically because cells are 2:1
    let kind = h % 6;
    let r1 = ((h >> 4) as i32).rem_euclid(5);
    let r2 = ((h >> 12) as i32).rem_euclid(4);
    match kind {
        // small square (8×4 .. 14×7 on screen)
        0 => ((8 + r1 * 2).min(max_hw), (3 + r2).min(max_hh), false),
        // long horizontal corridor (very wide, short)
        1 => ((max_hw).min(14 + r1 * 2), (1 + r2 / 2).min(max_hh), false),
        // tall vertical hall (narrow on screen but tall — needs ~6 world hw to look narrow-ish)
        2 => ((4 + r1).min(max_hw), (max_hh).min(7 + r2), false),
        // round room on screen — ellipse hw=2*hh in world coords
        3 => {
            let hh = (3 + r2).min(max_hh);
            ((hh * 2).min(max_hw), hh, true)
        }
        // big oval room (wider than tall on screen)
        4 => {
            let hh = (4 + r2).min(max_hh);
            ((hh * 3).min(max_hw), hh, true)
        }
        // tiny nook
        _ => ((4 + r1).min(max_hw), (2 + r2 / 2).min(max_hh), false),
    }
}

fn in_room_at(x: i32, y: i32, ax: i32, ay: i32, h: u32, macro_size: i32) -> bool {
    let (hw, hh, round) = room_shape(h, macro_size);
    let dx = (x - ax) as i64;
    let dy = (y - ay) as i64;
    if round {
        let hw = hw as i64;
        let hh = hh as i64;
        // ellipse: dx²·hh² + dy²·hw² <= hw²·hh²
        dx * dx * hh * hh + dy * dy * hw * hw <= hw * hw * hh * hh
    } else {
        dx.abs() <= hw as i64 && dy.abs() <= hh as i64
    }
}

/// L-shaped corridor from anchor (ax, ay) to (bx, by). Hash on the pair
/// decides whether the bend goes "horizontal first" or "vertical first".
fn on_l_corridor(x: i32, y: i32, ax: i32, ay: i32, bx: i32, by: i32, seed: u32) -> bool {
    let h_first = (hash2(ax + bx, ay + by, seed.wrapping_add(0xC0_47_E07A)) & 1) == 0;
    let (x0, x1) = (ax.min(bx), ax.max(bx));
    let (y0, y1) = (ay.min(by), ay.max(by));
    if h_first {
        if y == ay && x >= x0 && x <= x1 { return true; }
        if x == bx && y >= y0 && y <= y1 { return true; }
    } else {
        if x == ax && y >= y0 && y <= y1 { return true; }
        if y == by && x >= x0 && x <= x1 { return true; }
    }
    false
}

/// Resolve any cell into a labyrinth state (room, corridor, or wall).
/// Looks at the 3×3 macro neighbourhood so rooms whose footprint spills
/// into a neighbour macro cell still register.
fn labyrinth_cell(x: i32, y: i32, macro_size: i32, seed: u32) -> LabCell {
    let mcx = x.div_euclid(macro_size);
    let mcy = y.div_euclid(macro_size);
    // 1. room membership
    for dmcy in -1..=1i32 {
        for dmcx in -1..=1i32 {
            let mx = mcx + dmcx;
            let my = mcy + dmcy;
            let (ax, ay, h) = macro_anchor(mx, my, macro_size, seed);
            if in_room_at(x, y, ax, ay, h, macro_size) {
                return LabCell::Room { ax, ay, hash: h };
            }
        }
    }
    // 2. corridor membership: each anchor connects to its right + lower
    //    neighbour, so every pair of adjacent anchors gets exactly one
    //    L-corridor and the whole labyrinth stays reachable.
    for dmcy in -1..=1i32 {
        for dmcx in -1..=1i32 {
            let mx = mcx + dmcx;
            let my = mcy + dmcy;
            let (ax, ay, _) = macro_anchor(mx, my, macro_size, seed);
            let (rx, ry, _) = macro_anchor(mx + 1, my, macro_size, seed);
            if on_l_corridor(x, y, ax, ay, rx, ry, seed) {
                return LabCell::Corridor;
            }
            let (dx, dy, _) = macro_anchor(mx, my + 1, macro_size, seed);
            if on_l_corridor(x, y, ax, ay, dx, dy, seed) {
                return LabCell::Corridor;
            }
        }
    }
    LabCell::Wall
}

/// Sewer: brick chambers of every shape and size — small rectangular
/// cisterns, long horizontal aqueducts, tall vertical shafts, round
/// pump rooms — strung together by L-shaped maintenance corridors.
/// Bigger rooms get a small dark river in the centre; smaller ones
/// stay dry path.
fn sewer_get(x: i32, y: i32) -> Tile {
    const M: i32 = 22;
    const SEED: u32 = 0x5E_E5_5E_E5;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            // Anything bigger than a tiny nook gets a river pool that
            // scales with the room: thin sliver in small rooms, broad
            // pool in the big halls.
            let pool_hw = (hw / 3).max(1);
            let pool_hh = (hh / 3).max(1);
            if hw >= 4 && dx.abs() <= pool_hw && dy.abs() <= pool_hh {
                return Tile::Water;
            }
            Tile::Path
        }
        LabCell::Corridor => Tile::Path,
        LabCell::Wall => Tile::Wall,
    }
}

/// Hot Spring: tiny natural cave overflowing with mineral water.
fn hot_spring_get(x: i32, y: i32, seed: u32) -> Tile {
    let hs = seed.wrapping_add(0x4075_5E5E);
    let open = cave_open_at(x, y, hs) || cave_open_at(x + 1, y, hs);
    if !open {
        return Tile::CaveWall;
    }
    let r = hash2(x, y, hs.wrapping_add(0xBA7E)) % 100;
    if r < 5 {
        return Tile::Stalagmite;
    }
    if r < 80 {
        Tile::MineralWater
    } else {
        Tile::CaveFloor
    }
}

/// Pyramid: tomb chambers of every shape — small antechambers, long
/// hieroglyph halls, round burial pits, tall granary shafts — buried in
/// sandstone. Bigger tombs get a sacred pool. Corridors are sand-floored
/// passages between chambers.
fn pyramid_get(x: i32, y: i32) -> Tile {
    const M: i32 = 24;
    const SEED: u32 = 0x5A04_F00D;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            let pool_hw = (hw / 4).max(1);
            let pool_hh = (hh / 4).max(1);
            if hw >= 6 && dx.abs() <= pool_hw && dy.abs() <= pool_hh {
                return Tile::Water;
            }
            Tile::Sand
        }
        LabCell::Corridor => Tile::Sand,
        LabCell::Wall => Tile::Wall,
    }
}

/// Lakebed Caves: a fully flooded cavern. Open cells default to mineral
/// water; sparse stone islands give the player something to stand on
/// while casting. Stalactites accent dry margins; every surface lakebed
/// entrance projects through to a `MineExit` here, so the player always
/// climbs back to the exact island they descended from. Each exit gets
/// a 3x3 cave-floor pocket so they're not immediately walled or drowned.
fn lakebed_get(x: i32, y: i32, seed: u32) -> Tile {
    // Exits project through from the surface entrances — same (x, y).
    if is_lakebed_entrance_anchor(x, y, seed) {
        return Tile::MineExit;
    }
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if is_lakebed_entrance_anchor(x + dx, y + dy, seed) {
                return Tile::CaveFloor;
            }
        }
    }
    let lb = seed.wrapping_add(0x1A4E_BED0);
    let open = cave_open_at(x, y, lb);
    let r = hash2(x, y, lb.wrapping_add(0xCAFE_C0DE)) % 1000;
    if !open {
        // Wall margins seed stalactites and the occasional pebbled rock,
        // but no ore — lakebed is a fishing dim, not a mining dim.
        if r < 60 {
            return Tile::Stalactite;
        }
        return Tile::CaveWall;
    }
    // Open cells: a stone island here and there, otherwise mineral water.
    if r < 25 {
        return Tile::Stalagmite;
    }
    if r < 60 {
        return Tile::CaveFloor;
    }
    if r < 75 {
        return Tile::Rock;
    }
    Tile::MineralWater
}

/// Swamp Cave: dark cave with peat water and twisted roots.
fn swamp_cave_get(x: i32, y: i32, seed: u32) -> Tile {
    let sc = seed.wrapping_add(0x5AA9_0CA1);
    let open = cave_open_at(x, y, sc);
    if !open {
        return Tile::CaveWall;
    }
    let r = hash2(x, y, sc.wrapping_add(0x5EED_5EED)) % 100;
    if r < 8 {
        return Tile::TreeTrunk;
    }
    if r < 60 {
        return Tile::Water;
    }
    Tile::CaveFloor
}

/// Bog Cathedral: flooded chapels of varying scale — confessionals,
/// long naves, round rotundas, tall bell-shafts — joined by stone
/// causeways across dark altar water. Each chapel floods with water
/// around a central stone altar.
fn bog_cathedral_get(x: i32, y: i32) -> Tile {
    const M: i32 = 20;
    const SEED: u32 = 0x6074_C001;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            let altar_hw = 1.max(hw / 4);
            let altar_hh = 1.max(hh / 4);
            // altar column at centre
            if dx.abs() <= altar_hw && dy.abs() <= altar_hh {
                return Tile::Wall;
            }
            // stone walkway cross from altar out to the room edges
            if dx == 0 || dy == 0 {
                return Tile::Path;
            }
            Tile::Water
        }
        LabCell::Corridor => Tile::Path,
        LabCell::Wall => Tile::Wall,
    }
}

/// Mirror Lake: an infinite archipelago of overlapping silver pools.
/// Two layered sine fields select pool / shore / grass per cell so the
/// player can walk endlessly past one pool to the next.
fn mirror_lake_get(x: i32, y: i32) -> Tile {
    let fx = x as f32;
    let fy = y as f32;
    let a = (fx * 0.07 + fy * 0.05).sin();
    let b = (fx * 0.04 - fy * 0.09).cos();
    let n = a + b;
    if n > 0.4 {
        Tile::MineralWater
    } else if n > 0.15 {
        Tile::Sand
    } else {
        Tile::Grass
    }
}

/// Iceshelf: flat snow with sparse fishing holes.
fn iceshelf_get(x: i32, y: i32, seed: u32) -> Tile {
    let h = hash2(x, y, seed.wrapping_add(0x01CE_5E1F));
    if h % 600 < 3 {
        return Tile::Water;
    }
    Tile::Sand
}

/// Wreckage: half-sunken hulls of every size drifting on an open teal
/// sea. The labyrinth's "rooms" are the hull interiors (varied shape +
/// orientation per anchor), and the "corridors" are deck-plank gangways
/// linking neighbouring hulls. Outside any hull/gangway is open water.
fn wreckage_get(x: i32, y: i32) -> Tile {
    const M: i32 = 26;
    const SEED: u32 = 0x8_0D_5_E_A1;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            // hull perimeter
            let on_perim = (dx.abs() == hw && dy.abs() <= hh)
                || (dy.abs() == hh && dx.abs() <= hw);
            if on_perim {
                return Tile::Wall;
            }
            // central deck plank along the long axis
            if hw >= hh {
                if dy == 0 { return Tile::Dock; }
            } else if dx == 0 {
                return Tile::Dock;
            }
            // ribs every few cells across the short axis
            if hw >= hh {
                if dx.rem_euclid(5) == 0 { return Tile::Wall; }
            } else if dy.rem_euclid(3) == 0 {
                return Tile::Wall;
            }
            Tile::Water
        }
        LabCell::Corridor => Tile::Dock,
        LabCell::Wall => Tile::Water,
    }
}

/// Crater: cosmic basins scattered across a starlit plain. Rooms are
/// the basins themselves (varied shape — round impact craters, long
/// trenches, tiny pockmarks), each with a cosmic pool at the centre.
/// Corridors are starlit walkways between basins.
fn crater_get(x: i32, y: i32) -> Tile {
    const M: i32 = 22;
    const SEED: u32 = 0xC0_5A_77_E1;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            let pool_hw = (hw / 2).max(1);
            let pool_hh = (hh / 2).max(1);
            // 2:1-aware ellipse for the pool itself
            let inside_pool = (dx as i64) * (dx as i64) * (pool_hh as i64) * (pool_hh as i64)
                + (dy as i64) * (dy as i64) * (pool_hw as i64) * (pool_hw as i64)
                <= (pool_hw as i64) * (pool_hw as i64) * (pool_hh as i64) * (pool_hh as i64);
            if inside_pool {
                return Tile::MineralWater;
            }
            // rocky shore around the pool
            let outer_hw = pool_hw + 1;
            let outer_hh = pool_hh + 1;
            if dx.abs() <= outer_hw && dy.abs() <= outer_hh {
                return Tile::Rock;
            }
            Tile::CaveFloor
        }
        LabCell::Corridor => Tile::CaveFloor,
        LabCell::Wall => Tile::CaveWall,
    }
}

/// Colosseum: marble amphitheatres of every scale — small training
/// rings, long colonnades, round arenas, tall vomitoria. Larger arenas
/// hold a flooded combat pit at their centre.
fn colosseum_get(x: i32, y: i32) -> Tile {
    const M: i32 = 24;
    const SEED: u32 = 0x0070_5A11;
    match labyrinth_cell(x, y, M, SEED) {
        LabCell::Room { ax, ay, hash } => {
            let (hw, hh, _) = room_shape(hash, M);
            let dx = x - ax;
            let dy = y - ay;
            let pit_hw = (hw / 3).max(1);
            let pit_hh = (hh / 3).max(1);
            if hw >= 6 && dx.abs() <= pit_hw && dy.abs() <= pit_hh {
                return Tile::Water;
            }
            Tile::Path
        }
        LabCell::Corridor => Tile::Path,
        LabCell::Wall => Tile::Wall,
    }
}

/// All Blue: pure open ocean. No clutter — every cell is fishable
/// DeepWater, and the pool dispatch decides what bites.
fn all_blue_get(_x: i32, _y: i32, _seed: u32) -> Tile {
    Tile::DeepWater
}

// ---- specialty-dim wall/floor glyphs --------------------------------------
// Per-dim variants of the generic Wall / Sand / Path renders. Each uses a
// distinct palette so the dim reads at a glance even when sharing the
// underlying tile enum.

// ---- Fishable-tile dark backgrounds --------------------------------------
//
// Every tile the player can fish in (water, deep water, mineral pools,
// lava, sewer rivers, etc.) gets a near-black background tinted toward
// its water hue so fishing spots are unmistakable at a glance. Per the
// spec, no channel may exceed #12 (18 decimal). Keep these values
// hand-picked so the bg complements the fg without competing with it.

fn with_fishable_bg(out: (char, Style), bg: Color) -> (char, Style) {
    (out.0, out.1.bg(bg))
}

fn water_bg_for(dim: Dimension) -> Color {
    match dim {
        Dimension::Sewer => Color::Rgb(4, 14, 4),
        Dimension::SwampCave => Color::Rgb(4, 10, 4),
        Dimension::Wreckage => Color::Rgb(4, 12, 14),
        Dimension::BogCathedral => Color::Rgb(8, 6, 14),
        Dimension::Pyramid => Color::Rgb(14, 10, 4),
        // Lakebed water: drowned cave deep — colder and bluer than the
        // surface ocean baseline so the flooded cavern feels submerged
        // rather than just dark.
        Dimension::Lakebed => Color::Rgb(2, 8, 22),
        _ => Color::Rgb(4, 6, 18),
    }
}

fn mineral_bg_for(dim: Dimension) -> Color {
    match dim {
        Dimension::HotSpring => Color::Rgb(14, 4, 4),
        Dimension::Crater => Color::Rgb(12, 4, 14),
        Dimension::MirrorLake => Color::Rgb(10, 12, 14),
        // Lakebed mineral water reads as deep aquamarine — the cave is
        // mostly water, so this is the dominant tile and deserves a
        // recognisable hue.
        Dimension::Lakebed => Color::Rgb(2, 16, 26),
        _ => Color::Rgb(4, 14, 18),
    }
}

// For the non-sewer dims: one defining hue per tile role (wall / floor /
// water), with small shade jitter for texture and an *occasional* tiny
// accent — never a competing secondary that would scatter the palette.

fn sandstone_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5A04_57E0);
    let g = match h % 4 { 0 => '#', 1 => '%', 2 => '8', _ => 'H' };
    let shade = 175 + (h % 30) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade - 35, shade - 80)).add_modifier(Modifier::BOLD))
}

fn wood_hull_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x80D_517E1);
    let g = match h % 3 { 0 => '|', 1 => '#', _ => '=' };
    let shade = 95 + (h % 25) as u8;
    (g, Style::default().fg(Color::Rgb(shade + 10, shade - 25, shade.saturating_sub(60))).add_modifier(Modifier::BOLD))
}

fn roman_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x0070_5A11);
    let g = match h % 4 { 0 => '#', 1 => 'H', 2 => '%', _ => '8' };
    let shade = 220 + (h % 25) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade, shade.saturating_sub(20))).add_modifier(Modifier::BOLD))
}

fn gothic_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x6074_1C00);
    let g = match h % 4 { 0 => '#', 1 => '%', 2 => '|', _ => '+' };
    let shade = 95 + (h % 25) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade - 10, shade + 5)).add_modifier(Modifier::BOLD))
}

fn sewer_brick_glyph(x: i32, y: i32) -> (char, Style) {
    // KEEP-AS-IS: user has signed off on the sewer palette. Mostly dark
    // muddy brown brick (~80%), olive green mortar (~15%), rare cyan
    // rust pipe (~5%). Do NOT regularise this back to single-tone like
    // the other dims — they wanted the sewer to keep its triadic look.
    let h = hash2(x, y, 0x5E_EB_4_C00);
    let g = match h % 5 { 0 => '%', 1 => '=', 2 => '|', _ => '#' };
    let bucket = (h >> 8) % 100;
    let shade = 75 + (h % 22) as u8;
    let style = if bucket < 5 {
        // cyan rust accent
        Style::default().fg(Color::Rgb(50, 130, 145)).add_modifier(Modifier::BOLD)
    } else if bucket < 20 {
        // olive mortar
        Style::default().fg(Color::Rgb(shade - 10, shade + 10, shade.saturating_sub(40))).add_modifier(Modifier::BOLD)
    } else {
        // dominant: dark muddy brown brick
        Style::default().fg(Color::Rgb(shade + 5, shade.saturating_sub(20), shade.saturating_sub(45))).add_modifier(Modifier::BOLD)
    };
    (g, style)
}

fn snow_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5_001_5_001);
    let g = match h % 4 { 0 => '.', 1 => ',', 2 => '`', _ => ' ' };
    let shade = 215 + (h % 35) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade, 255)))
}

fn pyramid_sand_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5A04_FA90);
    let g = match h % 3 { 0 => ',', 1 => '.', _ => '`' };
    let shade = 210 + (h % 30) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade - 30, shade.saturating_sub(110))))
}

fn roman_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x0070_5F10);
    let g = match h % 4 { 0 => '.', 1 => ',', 2 => '_', _ => ':' };
    let shade = 200 + (h % 30) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade - 5, shade.saturating_sub(40))))
}

fn sewer_walk_glyph(x: i32, y: i32) -> (char, Style) {
    // KEEP-AS-IS triadic palette like sewer_brick_glyph — user signed
    // off on the sewers.
    let h = hash2(x, y, 0x5E_EB_5_EE7);
    let g = match h % 4 { 0 => '.', 1 => ',', 2 => ':', _ => '`' };
    let bucket = (h >> 8) % 100;
    let shade = 90 + (h % 25) as u8;
    let style = if bucket < 5 {
        // rare: cyan rust drip
        Style::default().fg(Color::Rgb(60, 130, 130))
    } else if bucket < 22 {
        // secondary: bright mossy green patch
        Style::default().fg(Color::Rgb(75, 110, 55))
    } else {
        // primary: dark sewer dust
        Style::default().fg(Color::Rgb(shade - 15, shade - 10, shade.saturating_sub(40)))
    };
    (g, style)
}

fn cathedral_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x6074_F100);
    let g = match h % 4 { 0 => '.', 1 => ',', 2 => '_', _ => ':' };
    let shade = 130 + (h % 25) as u8;
    (g, Style::default().fg(Color::Rgb(shade, shade - 5, shade + 10)))
}

// ---- Per-dim CaveWall / CaveFloor variants ---------------------------------

fn hot_spring_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x4075_5_AA1);
    let g = match h % 5 { 0 => '#', 1 => '%', 2 => '&', 3 => 'M', _ => '8' };
    let shade = 95 + (h % 35) as u8;
    let r = shade + 35;
    let gc = shade.saturating_sub(15);
    let b = shade.saturating_sub(20);
    (g, Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD))
}

fn hot_spring_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x4075_5F00);
    let g = match h % 5 { 0 => '.', 1 => ',', 2 => '`', 3 => ':', _ => ';' };
    let shade = 60 + (h % 22) as u8;
    (g, Style::default().fg(Color::Rgb(shade + 25, shade.saturating_sub(8), shade.saturating_sub(15))))
}

fn swamp_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5AA9_5_AA1);
    let g = match h % 5 { 0 => '#', 1 => '%', 2 => '@', 3 => 'W', _ => '$' };
    let shade = 55 + (h % 28) as u8;
    let r = shade.saturating_sub(8);
    let gc = shade + 15;
    let b = shade.saturating_sub(20);
    (g, Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD))
}

fn swamp_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x5AA9_5F00);
    let g = match h % 4 { 0 => '.', 1 => ',', 2 => ';', _ => ':' };
    let shade = 45 + (h % 18) as u8;
    (g, Style::default().fg(Color::Rgb(shade.saturating_sub(5), shade + 8, shade.saturating_sub(15))))
}

fn crater_wall_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xC0_5A_71_E1);
    let g = match h % 5 { 0 => '#', 1 => '%', 2 => '*', 3 => '+', _ => 'M' };
    let shade = 65 + (h % 28) as u8;
    let r = shade + 20;
    let gc = shade.saturating_sub(10);
    let b = shade + 45;
    (g, Style::default().fg(Color::Rgb(r, gc, b)).add_modifier(Modifier::BOLD))
}

fn crater_floor_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xC0_5A_72_F0);
    let g = match h % 5 { 0 => '.', 1 => ',', 2 => '`', 3 => '\'', _ => ':' };
    let shade = 30 + (h % 16) as u8;
    (g, Style::default().fg(Color::Rgb(shade + 10, shade.saturating_sub(2), shade + 25)))
}

fn lakebed_floor_glyph(x: i32, y: i32) -> (char, Style) {
    // Pale silt + the occasional bubble. Cool blue-cyan tint over a near
    // black bg so the flooded cave reads as submerged limestone.
    let h = hash2(x, y, 0x1A_4E_BE_D1);
    let g = match h % 8 {
        0 => 'o',
        1 => '.',
        2 => ':',
        3 => ',',
        _ => '`',
    };
    let shade = 40 + (h % 18) as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(
                shade.saturating_sub(8),
                shade + 4,
                shade + 20,
            ))
            .bg(Color::Rgb(2, 8, 22)),
    )
}

// ---- Per-dim Water tints --------------------------------------------------

/// Standard ocean wave field but recoloured per dim. Glyph follows the
/// same animated height as `water_anim`; only the palette changes.
fn tinted_water_glyph(x: i32, y: i32, tick: u64, ramp: &[(u8, u8, u8); 7]) -> (char, Style) {
    let t = tick as f32 * 0.04;
    let fx = x as f32;
    let fy = y as f32;
    let w1 = (fx * 0.30 + fy * 0.21 + t * 0.9).sin();
    let w2 = (fx * 0.18 - fy * 0.34 + t * 0.6).sin();
    let h = w1 + w2;
    let idx = if h > 1.6 { 0 } else if h > 0.8 { 1 } else if h > 0.2 { 2 }
              else if h > -0.3 { 3 } else if h > -0.9 { 4 } else if h > -1.5 { 5 } else { 6 };
    let glyph = match idx { 0 | 1 | 2 => '~', 3 => '-', 4 => '_', 5 => '.', _ => ',' };
    let (r, g, b) = ramp[idx];
    let mut style = Style::default().fg(Color::Rgb(r, g, b));
    if h > 0.8 {
        style = style.add_modifier(Modifier::BOLD);
    }
    (glyph, style)
}

fn sewer_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    const R: [(u8, u8, u8); 7] = [
        (140, 165, 110), (110, 135, 85), (90, 115, 70),
        (70, 95, 55), (55, 75, 45), (40, 55, 35), (28, 40, 25),
    ];
    tinted_water_glyph(x, y, tick, &R)
}

fn swamp_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    const R: [(u8, u8, u8); 7] = [
        (85, 95, 55), (70, 80, 45), (55, 65, 40),
        (45, 55, 35), (35, 45, 25), (25, 35, 20), (18, 28, 15),
    ];
    tinted_water_glyph(x, y, tick, &R)
}

fn wreckage_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    const R: [(u8, u8, u8); 7] = [
        (95, 145, 145), (75, 120, 130), (55, 95, 115),
        (40, 80, 100), (30, 65, 85), (22, 50, 70), (15, 38, 55),
    ];
    tinted_water_glyph(x, y, tick, &R)
}

fn cathedral_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    const R: [(u8, u8, u8); 7] = [
        (130, 110, 165), (105, 90, 140), (85, 75, 120),
        (70, 60, 105), (55, 50, 90), (40, 38, 70), (28, 28, 55),
    ];
    tinted_water_glyph(x, y, tick, &R)
}

fn tomb_pool_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    const R: [(u8, u8, u8); 7] = [
        (210, 195, 130), (180, 160, 100), (150, 130, 75),
        (125, 105, 60), (100, 85, 50), (80, 65, 40), (60, 50, 30),
    ];
    tinted_water_glyph(x, y, tick, &R)
}

/// True when the cell is inside the spawn-village perimeter walls. The
/// village hides everything outside (and vice versa) so the player can't
/// see through stone.
pub fn village_inside(x: i32, y: i32) -> bool {
    x > WALL_L_OUT && x < WALL_R_OUT && y > WALL_TOP_EDGE && y < WALL_BOT_CAP
}

/// Spatial visibility mask: returns true if a cell at (wx, wy) should be
/// rendered from the player's vantage at (px, py). Only the village uses
/// this currently — the player can't see through perimeter walls in
/// either direction. The strip south of the village (y > WALL_BOT_EDGE,
/// the dock + ocean) is always visible from inside (south gate).
pub fn cell_visible_from(px: i32, py: i32, wx: i32, wy: i32) -> bool {
    let p_in = village_inside(px, py);
    let c_in = village_inside(wx, wy);
    if p_in == c_in {
        return true;
    }
    // Perimeter walls + their corner caps are always visible from either
    // side — they're the surface you're meant to look at, not an
    // obstruction.
    if village_perimeter(wx, wy).is_some() {
        return true;
    }
    // Player inside, cell outside: only the south strip (dock + ocean)
    // can be seen through the dock gap. Everything else blacks out.
    if p_in && wy > WALL_BOT_EDGE {
        return true;
    }
    // Player outside, cell inside: never visible.
    false
}

/// Generic buried-wall check: a wall cell with no walkable 4-neighbor is
/// "inside the wall" and renders pitch black. Applied to any Tile::Wall
/// in dims that use rectangular wall masses.
pub fn wall_buried(world: &World, x: i32, y: i32) -> bool {
    if !matches!(world.get(x, y), Tile::Wall) {
        return false;
    }
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let n = world.get(x + dx, y + dy);
        if n.walkable() || matches!(n, Tile::Water | Tile::MineralWater | Tile::DeepWater | Tile::Lava) {
            return false;
        }
    }
    true
}

