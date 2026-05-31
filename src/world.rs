use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use std::cell::RefCell;
use std::collections::HashMap;

// per-thread memoization for the two most-called lookups. capped so memory
// stays bounded across long explorations.
const CACHE_CAP: usize = 16384;

thread_local! {
    static BIOME_CACHE: RefCell<HashMap<(i32, i32), Biome>> = RefCell::new(HashMap::with_capacity(CACHE_CAP));
    static WATER_CACHE: RefCell<HashMap<(i32, i32), CellWaterInfo>> = RefCell::new(HashMap::with_capacity(CACHE_CAP));
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
    BIOME_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(&b) = c.get(&(x, y)) {
            return b;
        }
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        let b = biome_at(x, y, seed);
        c.insert((x, y), b);
        b
    })
}

fn cached_water_info(x: i32, y: i32, seed: u32) -> CellWaterInfo {
    WATER_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(&b) = c.get(&(x, y)) {
            return b;
        }
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        let b = compute_water_info(x, y, seed);
        c.insert((x, y), b);
        b
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
            tree: 0.025, big_rock: 0.0008, medium_rock: 0.0020, rock: 0.0015,
            pebble: 0.040, flower: 0.012, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Forest => BiomeParams {
            tree: 0.090, big_rock: 0.0008, medium_rock: 0.0015, rock: 0.0010,
            pebble: 0.020, flower: 0.003, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Rocky => BiomeParams {
            tree: 0.008, big_rock: 0.0060, medium_rock: 0.0140, rock: 0.0080,
            pebble: 0.120, flower: 0.001, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Scrub => BiomeParams {
            tree: 0.005, big_rock: 0.0006, medium_rock: 0.0015, rock: 0.0010,
            pebble: 0.020, flower: 0.002, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Desert => BiomeParams {
            tree: 0.0, big_rock: 0.0020, medium_rock: 0.0050, rock: 0.0035,
            pebble: 0.110, flower: 0.0, cactus: 0.012, puddle_bonus: 0.0,
        },
        Biome::Tundra => BiomeParams {
            tree: 0.012, big_rock: 0.0025, medium_rock: 0.0060, rock: 0.0040,
            pebble: 0.080, flower: 0.001, cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Swamp => BiomeParams {
            tree: 0.050, big_rock: 0.0006, medium_rock: 0.0010, rock: 0.0005,
            pebble: 0.015, flower: 0.006, cactus: 0.0, puddle_bonus: 0.18,
        },
    }
}

pub fn biome_at(x: i32, y: i32, seed: u32) -> Biome {
    let fx = x as f32 * 0.045;
    let fy = y as f32 * 0.055;
    let s = (seed as f32) * 0.00007;

    // single domain-warp pair (2 sins) gives curvy boundaries
    let warp_x = (fx * 0.42 + fy * 0.31 + s).sin() * 3.5;
    let warp_y = (fx * 0.33 - fy * 0.47 + s * 1.3).sin() * 3.5;
    let wx = fx + warp_x;
    let wy = fy + warp_y;

    // 3 noise channels (one sin each) drive biome selection
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
                | Tile::MineEntrance
                | Tile::CaveFloor
                | Tile::MineExit
                | Tile::Seabed
                | Tile::DeepWater
                | Tile::Kelp
                | Tile::InfernoFloor
                | Tile::LandmarkDoor
        )
    }

    pub fn describe(self) -> &'static str {
        match self {
            Tile::Grass => "Grass. Soft and quiet underfoot.",
            Tile::Wall => "A timber wall, weathered by salt air.",
            Tile::Roof => "A brick roof - russet tiles overlapping like fish scales.",
            Tile::DoorRod => "A creaky door. The Rod Shop sign hangs above it.",
            Tile::DoorSchool => "A formal door. The Fishing School's crest is painted on the lintel.",
            Tile::Water => "Dark water. Something moves below.",
            Tile::Dock => "Worn planks of the village dock.",
            Tile::Sand => "Damp sand. Bits of shell, gull tracks.",
            Tile::TreeTrunk => "A sturdy trunk. Bark rough under your fingers.",
            Tile::TreeCanopy => "Dense foliage. Birds rustle inside.",
            Tile::Rock => "A stone, knee-high. Easy to step around.",
            Tile::MediumRock => "A pair of split boulders pressed shoulder to shoulder.",
            Tile::BigRock => "A massive outcrop of weather-worn stone.",
            Tile::Pebble => "Small stones. They click underfoot.",
            Tile::Flower => "A wildflower, swaying. You feel a little better just looking.",
            Tile::Cactus => "A wary cactus, spines dry and bristling.",
            Tile::Well => "An old stone well. The bottom is darker than dark. You hear faint splashing.",
            Tile::Path => "A trodden path of packed earth and gravel.",
            Tile::Lamppost => "An iron lamppost. A small flame warms the glass at dusk.",
            Tile::Bench => "A worn wooden bench. Carved initials beneath the seat.",
            Tile::MineEntrance => "A mineshaft entrance. Wooden A-frame. Press f to descend.",
            Tile::MineFrame => "Heavy timbers brace the shaft mouth.",
            Tile::CaveFloor => "Packed earth, smelling of iron and old water.",
            Tile::CaveWall => "Cold rock wall.",
            Tile::Stalactite => "A stalactite. The cave's slow tooth, hanging.",
            Tile::Stalagmite => "A stalagmite. Patient drips, stood up.",
            Tile::OreRock => "An ore-bearing rock. A pickaxe would unlock it.",
            Tile::MineralWater => "A pool of utterly still water. Something glints below.",
            Tile::MineExit => "Wooden rungs up. Press f to climb back to the surface.",
            Tile::Seabed => "Soft sand of the seabed. You breathe — somehow.",
            Tile::CoralTrunk => "A coral pillar, layered with centuries.",
            Tile::CoralCanopy => "A coral crown, full of small bright life.",
            Tile::Kelp => "Tall kelp, swaying with the current.",
            Tile::DeepWater => "The open deep. You can fish here, anywhere.",
            Tile::Anemone => "An anemone, blooming and waiting.",
            Tile::InfernoWall => "Basalt, hot to the touch. The deeper rock remembers the fire.",
            Tile::InfernoFloor => "Cracked ground, glowing dimly between the seams.",
            Tile::Lava => "Lava. Something black drifts under the surface. You can fish it.",
            Tile::LandmarkWall => "A wall of the landmark before you.",
            Tile::LandmarkDoor => "An open archway. Step through.",
            Tile::Tombstone => "A weathered tombstone. Names worn off. You feel watched.",
        }
    }
}

pub struct World {
    pub seed: u32,
    pub dim: Dimension,
}

pub struct WorldView<'a> {
    pub world: &'a World,
    pub player: (i32, i32),
    pub player_facing: (i32, i32),
    pub tick: u64,
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
                    let g = match self.player_facing {
                        (0, -1) => '^',
                        (0, 1) => 'v',
                        (-1, 0) => '<',
                        (1, 0) => '>',
                        _ => '@',
                    };
                    return (g, player_style);
                }
                let wx = self.player.0 - half_w + sx;
                let wy = self.player.1 - half_h + sy;
                // NPCs are per-dim now (atlantean citizens, crypt ghouls,
                // infernal imps each live only in their own dim).
                if let Some(npc) = crate::npc::npc_at_dim(wx, wy, self.world.dim) {
                    return (
                        npc.render_char(),
                        Style::default()
                            .fg(npc.render_color())
                            .add_modifier(Modifier::BOLD),
                    );
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
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Tile {
        match self.dim {
            Dimension::Surface => self.surface_get(x, y),
            Dimension::Mines => self.mines_get(x, y),
            Dimension::Atlantis => self.atlantis_get(x, y),
            Dimension::Inferno => self.inferno_get(x, y),
        }
    }

    fn surface_get(&self, x: i32, y: i32) -> Tile {
        if let Some(t) = village_tile(x, y) {
            return t;
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
                return part;
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
        if is_mine_entrance_anchor(x, y, self.seed) {
            return Tile::MineExit;
        }
        let open = cave_open_at(x, y, self.seed);
        let r = hash2(x, y, self.seed.wrapping_add(0xCAFE_C0DE)) % 1000;
        if !open {
            // solid walls with the occasional ore vein and stalactite
            if r < 30 {
                return Tile::OreRock;
            }
            if r < 55 {
                return Tile::Stalactite;
            }
            return Tile::CaveWall;
        }
        // open floor: scattered decoration
        if r < 25 {
            return Tile::Stalagmite;
        }
        if r < 40 {
            return Tile::Rock;
        }
        if r < 50 {
            return Tile::Pebble;
        }
        // small underground pools — the mineral-fish pockets
        if mineral_pool_at(x, y, self.seed) {
            return Tile::MineralWater;
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
        let open = cave_open_at(x, y, self.seed.wrapping_add(0x1AFE_5A00));
        let r = hash2(x, y, self.seed.wrapping_add(0xF1AE_F1AE)) % 1000;
        if !open {
            if r < 20 {
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
        match self.get(x, y) {
            Tile::Wall => perimeter_glyph(x, y).unwrap_or_else(|| wall_glyph(x, y)),
            Tile::Roof => roof_glyph(x, y),
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
            Tile::Dock => ('=', Style::default().fg(Color::LightYellow)),
            Tile::Grass => grass_anim(x, y, tick, cached_biome_at(x, y, self.seed)),
            Tile::Water => {
                if matches!(self.get(x, y - 1), Tile::Sand) {
                    shore_anim(x, 1, tick)
                } else {
                    water_anim(x, y, tick)
                }
            }
            Tile::Sand => {
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
                    .bg(Color::Rgb(20, 20, 30))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::Path => {
                let g = match hash2(x, y, 0xDADA_BABE) % 3 {
                    0 => '.',
                    1 => ',',
                    _ => '.',
                };
                (g, Style::default().fg(Color::Rgb(150, 135, 105)))
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
            Tile::MineEntrance => (
                '#',
                Style::default()
                    .fg(Color::Rgb(60, 40, 25))
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::MineFrame => mine_frame_glyph(x, y, self.seed),
            Tile::CaveFloor => cave_floor_glyph(x, y),
            Tile::CaveWall => cave_wall_glyph(x, y),
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
            Tile::OreRock => ore_rock_glyph(x, y),
            Tile::MineralWater => mineral_water_glyph(x, y, tick),
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
            Tile::DeepWater => deep_water_glyph(x, y, tick),
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
            Tile::InfernoWall => inferno_wall_glyph(x, y),
            Tile::InfernoFloor => inferno_floor_glyph(x, y),
            Tile::Lava => lava_glyph(x, y, tick),
            Tile::LandmarkWall => landmark_wall_glyph(x, y, self.dim),
            Tile::LandmarkDoor => landmark_door_glyph(self.dim),
            Tile::Tombstone => {
                let g = match hash2(x, y, 0x7070_85_70) % 3 {
                    0 => 'T',
                    1 => '|',
                    _ => '+',
                };
                (g, Style::default().fg(Color::Rgb(170, 170, 180)).add_modifier(Modifier::BOLD))
            }
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
    let shade = 70 + (h % 35) as u8;
    (
        g,
        Style::default().fg(Color::Rgb(shade + 30, shade, shade.saturating_sub(15))),
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
    let shade = 60 + (h % 45) as u8;
    let r = shade + 20;
    let gc = shade.saturating_sub(5);
    let b = shade.saturating_sub(20);
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
}

fn ore_rock_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0x09E5_EED1);
    let (fg, ch) = match h % 7 {
        0 => (Color::Rgb(230, 200, 90), '*'),  // gold
        1 => (Color::Rgb(200, 220, 240), '+'), // silver
        2 => (Color::Rgb(220, 130, 90), 'o'),  // copper
        3 => (Color::Rgb(180, 160, 220), '%'), // amethyst
        4 => (Color::Rgb(100, 220, 180), '#'), // turquoise
        5 => (Color::Rgb(240, 100, 100), '&'), // ruby
        _ => (Color::Rgb(80, 200, 240), 'X'),  // sapphire
    };
    (
        ch,
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    )
}

fn mineral_water_glyph(x: i32, y: i32, tick: u64) -> (char, Style) {
    // shimmering pool with mineral glints; cycles through a few tints
    let phase = ((tick / 8) as i32 + x + y).rem_euclid(4);
    let g = match (hash2(x, y, 0x9A7E_5A1E) + tick as u32 / 12) % 5 {
        0 => '~',
        1 => '*',
        2 => '.',
        3 => ',',
        _ => '~',
    };
    let (r, gc, b) = match phase {
        0 => (90, 200, 240),
        1 => (140, 220, 200),
        2 => (200, 200, 240),
        _ => (160, 200, 220),
    };
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
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
    None
}

/// The Crypt in the Mines. 11x7 rectangle around (0, 0), interior dotted
/// with tombstones. Door at south.
fn mines_crypt_at(x: i32, y: i32) -> Option<Tile> {
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
    };
    (g, Style::default().fg(fg).add_modifier(Modifier::BOLD))
}

fn landmark_door_glyph(dim: Dimension) -> (char, Style) {
    let fg = match dim {
        Dimension::Atlantis => Color::Rgb(255, 230, 130),
        Dimension::Mines => Color::Rgb(110, 100, 90),
        Dimension::Inferno => Color::Rgb(255, 130, 50),
        Dimension::Surface => Color::Rgb(210, 175, 110),
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
    // figure out which neighbor cell is the anchor
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
                        .fg(Color::Rgb(120, 80, 45))
                        .bg(Color::Rgb(40, 28, 18))
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

fn water_body_at(x: i32, y: i32, seed: u32) -> bool {
    compute_water_info(x, y, seed).in_water
}

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
    // 3 small huts around a central well
    let huts = &[
        ((-10, -7), (-6, -5), (-8, -5), Tile::DoorRod),
        ((6, -7), (10, -5), (8, -5), Tile::DoorRod),
        ((-2, 5), (2, 7), (0, 7), Tile::DoorSchool),
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
    // 5 houses inside
    let huts = &[
        ((-15, -7), (-11, -5), (-13, -5), Tile::DoorRod),
        ((-5, -7), (-1, -5), (-3, -5), Tile::DoorRod),
        ((5, -7), (9, -5), (7, -5), Tile::DoorSchool),
        ((-9, 5), (-5, 7), (-7, 7), Tile::DoorRod),
        ((5, 5), (9, 7), (7, 7), Tile::DoorSchool),
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
        ((-16, -1), (-12, 1), (-12, 1), Tile::DoorRod),  // west
        ((12, -1), (16, 1), (12, 1), Tile::DoorRod),     // east
        ((-1, -16), (1, -12), (0, -12), Tile::DoorSchool), // north
        ((-1, 12), (1, 16), (0, 12), Tile::DoorSchool),  // south
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
    // keep entrances on dry, dry-ish land — never under the ocean or in the village
    if y >= 4 {
        return false;
    }
    if in_village_zone(x, y) {
        return false;
    }
    if cached_water_body_at(x, y, seed) {
        return false;
    }
    // about 1 per 900 cells: visible-ish landmarks. Tune as needed.
    let h = hash2(x, y, seed.wrapping_add(0xE17E_ED01));
    h % 900 == 7
}

fn mine_entrance_tile_at(x: i32, y: i32, seed: u32) -> Option<Tile> {
    if is_mine_entrance_anchor(x, y, seed) {
        return Some(Tile::MineEntrance);
    }
    // frame cells: anchor is at (ax, ay) with frame at the 5 cells of the
    // 3-wide, 2-tall box (excluding the anchor itself which is the opening).
    for dx in -1..=1i32 {
        for dy in -1..=0i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if is_mine_entrance_anchor(x - dx, y - dy, seed) {
                return Some(Tile::MineFrame);
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
        // trunk - paired [ ] stacked two rows tall
        if (dy == 0 || dy == -1) && (dx == 0 || dx == 1) {
            let g = if dx == 0 { '[' } else { ']' };
            let r = 145 + (anchor_hash % 25) as u8;
            let gc = 100 + (anchor_hash % 22) as u8;
            let b = 60 + (anchor_hash % 18) as u8;
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
        ((-37, -33), (-1, 1), (-35, 1), Tile::DoorRod),
        ((-20, -16), (-1, 1), (-18, 1), Tile::DoorRod),
        ((-2, 2), (-5, -3), (0, -3), Tile::DoorRod),
        ((-25, -21), (-7, -5), (-23, -5), Tile::DoorRod),
        ((21, 25), (-7, -5), (23, -5), Tile::DoorSchool),
        ((16, 20), (-1, 1), (18, 1), Tile::DoorSchool),
        ((33, 37), (-1, 1), (35, 1), Tile::DoorSchool),
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
    (-14, 3), (14, 3),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_returns_water_at_depth() {
        let w = World::new(0);
        assert_eq!(w.get(0, 10), Tile::Water);
    }

    #[test]
    fn village_has_doors() {
        let w = World::new(0);
        assert_eq!(w.get(-20, 1), Tile::DoorRod);
        assert_eq!(w.get(20, 1), Tile::DoorSchool);
    }
}
