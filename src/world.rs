use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

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
    rock: f32,
    pebble: f32,
    flower: f32,
    cactus: f32,
    puddle_bonus: f32,
}

fn biome_params(b: Biome) -> BiomeParams {
    match b {
        Biome::Meadow => BiomeParams {
            tree: 0.025, big_rock: 0.002, rock: 0.010, pebble: 0.040, flower: 0.012,
            cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Forest => BiomeParams {
            tree: 0.090, big_rock: 0.002, rock: 0.008, pebble: 0.020, flower: 0.003,
            cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Rocky => BiomeParams {
            tree: 0.008, big_rock: 0.012, rock: 0.045, pebble: 0.120, flower: 0.001,
            cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Scrub => BiomeParams {
            tree: 0.005, big_rock: 0.001, rock: 0.006, pebble: 0.020, flower: 0.002,
            cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Desert => BiomeParams {
            tree: 0.0, big_rock: 0.004, rock: 0.020, pebble: 0.110, flower: 0.0,
            cactus: 0.012, puddle_bonus: 0.0,
        },
        Biome::Tundra => BiomeParams {
            tree: 0.012, big_rock: 0.005, rock: 0.025, pebble: 0.080, flower: 0.001,
            cactus: 0.0, puddle_bonus: 0.0,
        },
        Biome::Swamp => BiomeParams {
            tree: 0.050, big_rock: 0.001, rock: 0.004, pebble: 0.015, flower: 0.006,
            cactus: 0.0, puddle_bonus: 0.18,
        },
    }
}

pub fn biome_at(x: i32, y: i32, seed: u32) -> Biome {
    let fx = x as f32 * 0.045;
    let fy = y as f32 * 0.055;
    let s = (seed as f32) * 0.00007;

    // domain warp so band boundaries become curvy blobs rather than straight lines
    let warp_x =
        (fx * 0.42 + fy * 0.31 + s).sin() * 3.5 + (fy * 0.27 - s * 0.7).sin() * 1.8;
    let warp_y =
        (fx * 0.33 - fy * 0.47 + s * 1.3).sin() * 3.5 + (fx * 0.19 + s * 0.5).sin() * 1.8;
    let wx = fx + warp_x;
    let wy = fy + warp_y;

    // temperature: hot positive, cold negative
    let temp = (wx * 0.18 + wy * 0.07 + s).sin() + (wx * 0.09 - wy * 0.13).sin() * 0.5;
    // moisture: wet positive, dry negative
    let moist =
        (wx * 0.13 - wy * 0.21 + s * 1.7).sin() + (wx * 0.07 + wy * 0.11).sin() * 0.5;
    // vegetation: high = forest-y
    let veg =
        (wx * 0.08 + wy * 0.06 - s * 0.9).sin() + (wx * 0.21 + wy * 0.09).sin() * 0.5;

    if temp > 0.7 && moist < -0.2 {
        Biome::Desert
    } else if temp < -0.7 {
        Biome::Tundra
    } else if moist > 0.8 {
        Biome::Swamp
    } else if veg > 0.6 {
        Biome::Forest
    } else if moist < -0.4 && veg < 0.0 {
        Biome::Scrub
    } else if veg < -0.5 {
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
    BigRock,
    Pebble,
    Flower,
    Cactus,
    Well,
}

impl Tile {
    pub fn walkable(self) -> bool {
        matches!(
            self,
            Tile::Grass | Tile::Sand | Tile::Pebble | Tile::Flower
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
            Tile::Rock => "A boulder, half-buried. Lichen patches one side.",
            Tile::BigRock => "A massive outcrop of weather-worn stone.",
            Tile::Pebble => "Small stones. They click underfoot.",
            Tile::Flower => "A wildflower, swaying. You feel a little better just looking.",
            Tile::Cactus => "A wary cactus, spines dry and bristling.",
            Tile::Well => "An old stone well. The bottom is darker than dark. You hear faint splashing.",
        }
    }
}

pub struct World {
    pub seed: u32,
}

pub struct WorldView<'a> {
    pub world: &'a World,
    pub player: (i32, i32),
    pub tick: u64,
}

impl<'a> Widget for WorldView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let half_w = (area.width as i32) / 2;
        let half_h = (area.height as i32) / 2;
        let player_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        for sy in 0..area.height {
            for sx in 0..area.width {
                let cx = area.x + sx;
                let cy = area.y + sy;
                let cell = &mut buf[(cx, cy)];
                if sx as i32 == half_w && sy as i32 == half_h {
                    cell.set_char('@').set_style(player_style);
                } else {
                    let wx = self.player.0 - half_w + sx as i32;
                    let wy = self.player.1 - half_h + sy as i32;
                    if let Some(npc) = crate::npc::npc_at(wx, wy) {
                        cell.set_char(npc.render_char()).set_style(
                            Style::default()
                                .fg(npc.render_color())
                                .add_modifier(Modifier::BOLD),
                        );
                    } else {
                        let (g, s) = self.world.render_tile(wx, wy, self.tick);
                        cell.set_char(g).set_style(s);
                    }
                }
            }
        }
    }
}

impl World {
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    pub fn get(&self, x: i32, y: i32) -> Tile {
        if let Some(t) = village_tile(x, y) {
            return t;
        }
        if y >= 6 {
            return Tile::Water;
        }
        if y == 5 {
            return Tile::Sand;
        }
        if water_body_at(x, y, self.seed) {
            return Tile::Water;
        }
        if well_at(x, y, self.seed) {
            return Tile::Well;
        }
        if !in_village_zone(x, y) {
            let biome = biome_at(x, y, self.seed);
            let p = biome_params(biome);
            if p.cactus > 0.0 {
                let rc = hash2(x, y, self.seed.wrapping_add(0xCAC7_CAC7)) as f32 / u32::MAX as f32;
                if rc < p.cactus {
                    return Tile::Cactus;
                }
            }
            if let Some(part) = tree_at(x, y, self.seed, p.tree) {
                return part;
            }
            if big_rock_at(x, y, self.seed, p.big_rock) {
                return Tile::BigRock;
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
        Tile::Grass
    }

    #[allow(dead_code)]
    pub fn biome(&self, x: i32, y: i32) -> Biome {
        biome_at(x, y, self.seed)
    }

    pub fn render_tile(&self, x: i32, y: i32, tick: u64) -> (char, Style) {
        match self.get(x, y) {
            Tile::Wall => wall_glyph(x, y),
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
            Tile::Grass => grass_anim(x, y, tick, biome_at(x, y, self.seed)),
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
                    (g, Style::default().fg(shade((180, 165, 120), x, y, 0x5A1D_5A1D, 14)))
                }
            }
            Tile::TreeTrunk | Tile::TreeCanopy => tree_render(x, y, self.seed),
            Tile::Rock => rock_glyph(x, y),
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
        }
    }
}

fn is_big_rock_anchor(x: i32, y: i32, seed: u32, density: f32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if water_body_at(x, y, seed) {
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

fn is_tree_anchor(x: i32, y: i32, seed: u32, density: f32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 || y <= -1000 {
        return false;
    }
    if water_body_at(x, y, seed) {
        return false;
    }
    let r = hash2(x, y, seed.wrapping_add(0xC0DE_C0DE)) as f32 / u32::MAX as f32;
    r < density
}

const WATER_CELL: i32 = 26;

fn water_body_at(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 5 {
        return false; // ocean strip handled elsewhere
    }
    let cx = x.div_euclid(WATER_CELL);
    let cy = y.div_euclid(WATER_CELL);
    for dcy in -1..=1 {
        for dcx in -1..=1 {
            let ccx = cx + dcx;
            let ccy = cy + dcy;
            let h = hash2(ccx, ccy, seed.wrapping_add(0xF00D_BEEF));
            // ~14% of coarse cells host a water body
            if h % 7 != 0 {
                continue;
            }
            // anchor offset inside the coarse cell
            let ox = ((h >> 4) as i32).rem_euclid(WATER_CELL);
            let oy = ((h >> 12) as i32).rem_euclid(WATER_CELL);
            let ax = ccx * WATER_CELL + ox;
            let ay = ccy * WATER_CELL + oy;
            // size class
            let radius: i32 = match (h >> 20) % 10 {
                0..=5 => 1, // puddle / tiny
                6..=8 => 3, // pond
                _ => 6,     // small lake
            };
            // never place water body too close to the ocean shore (avoid touching y >= 5)
            if ay + radius >= 5 {
                continue;
            }
            let dx = (x - ax).abs();
            let dy = (y - ay).abs();
            if dx + dy <= radius {
                return true;
            }
        }
    }
    false
}

fn tree_at(x: i32, y: i32, seed: u32, density: f32) -> Option<Tile> {
    for dy in 0..=2i32 {
        for dx in -1..=1i32 {
            let ax = x + dx;
            let ay = y + dy;
            let local_density = biome_params(biome_at(ax, ay, seed)).tree;
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
            let density = biome_params(biome_at(ax, ay, seed)).tree;
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
    x.abs() <= 30 && (-6..=4).contains(&y)
}

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = seed.wrapping_add((x as u32).wrapping_mul(374_761_393));
    h = h.wrapping_add((y as u32).wrapping_mul(668_265_263));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
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
            leaf_style(g, anchor_hash, (55, 85, 50), x, y)
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
            leaf_style(g, anchor_hash, (40, 70, 45), x, y)
        }
        (TreeSpecies::Bush, _) => {
            let g = match anchor_hash % 3 {
                0 => 'o',
                1 => '*',
                _ => 'q',
            };
            leaf_style(g, anchor_hash, (70, 90, 55), x, y)
        }
    }
}

fn trunk_style(anchor_hash: u32, g: char) -> (char, Style) {
    let r = 80 + (anchor_hash % 20) as u8;
    let gc = 55 + (anchor_hash % 15) as u8;
    let b = 35 + (anchor_hash % 12) as u8;
    (g, Style::default().fg(Color::Rgb(r, gc, b)))
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
    let v = hash2(x, y, 0xF00D_F00D) % 3;
    let (g, base) = match v {
        0 => ('o', (100, 100, 100)),
        1 => ('O', (120, 120, 120)),
        _ => ('@', (90, 90, 90)),
    };
    (g, Style::default().fg(shade(base, x, y, 0xF00D_F00D, 12)))
}

fn pebble_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xABCD_1234) % 3;
    let g = match v {
        0 => '.',
        1 => ',',
        _ => '`',
    };
    (g, Style::default().fg(shade((115, 105, 90), x, y, 0xABCD_1234, 15)))
}

fn flower_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xFFEE_DD11) % 3;
    let color = match h {
        0 => Color::Rgb(180, 175, 150),
        1 => Color::Rgb(170, 150, 130),
        _ => Color::Rgb(160, 140, 150),
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
            let density = biome_params(biome_at(ax, ay, seed)).big_rock;
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
    let dx = x - anchor.0;
    let dy = y - anchor.1;
    let g = match (dx, dy) {
        (0, 0) => '/',
        (1, 0) => '\\',
        (0, 1) => '\\',
        (1, 1) => '/',
        _ => '#',
    };
    let shade = hash2(anchor.0, anchor.1, 0xCAFE_BABE) % 40;
    let base = 110 + shade as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(base, base, base))
            .add_modifier(Modifier::BOLD),
    )
}

fn village_tile(x: i32, y: i32) -> Option<Tile> {
    let in_left_house = (-22..=-18).contains(&x) && (-3..=1).contains(&y);
    if in_left_house {
        if x == -20 && y == 1 {
            return Some(Tile::DoorRod);
        }
        if y == -3 {
            return Some(Tile::Roof);
        }
        return Some(Tile::Wall);
    }
    let in_right_house = (18..=22).contains(&x) && (-3..=1).contains(&y);
    if in_right_house {
        if x == 20 && y == 1 {
            return Some(Tile::DoorSchool);
        }
        if y == -3 {
            return Some(Tile::Roof);
        }
        return Some(Tile::Wall);
    }
    if (-6..=5).contains(&x) && (5..=8).contains(&y) {
        return Some(Tile::Dock);
    }
    // village well in the central square
    if (x, y) == (0, -1) {
        return Some(Tile::Well);
    }
    None
}

const WELL_CELL: i32 = 60;

fn well_at(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    if water_body_at(x, y, seed) {
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
    (glyph, Style::default().fg(shade(base, x, y, 0xA11_BABE, 6)))
}

fn grass_anim(x: i32, y: i32, _tick: u64, biome: Biome) -> (char, Style) {
    let base = match biome {
        Biome::Meadow => (65, 105, 65),
        Biome::Forest => (45, 80, 50),
        Biome::Rocky => (95, 100, 70),
        Biome::Scrub => (110, 105, 75),
        Biome::Desert => (170, 145, 95),
        Biome::Tundra => (170, 175, 175),
        Biome::Swamp => (60, 75, 50),
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
            .fg(shade((85, 120, 65), x, y, 0xCAC7_F00D, 10))
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
    const SPAWN_EVERY: u64 = 3;
    const LIFETIME: u64 = 22;
    const REACH: i32 = 4;

    let mut active: Option<(u64, i32, u32)> = None;
    let earliest = tick.saturating_sub(LIFETIME);
    let mut t = (earliest / SPAWN_EVERY) * SPAWN_EVERY;
    while t <= tick {
        for dx in -REACH..=REACH {
            let ax = x - dx;
            let h = hash2(ax, t as i32, 0xFA0A_FA0A);
            if h % 90 != 0 {
                continue;
            }
            let reach = ((h >> 4) as i32 % 3 + 2).abs();
            if dx.abs() > reach {
                continue;
            }
            // skip splashes that haven't started yet at this tick (paranoia)
            if t > tick {
                continue;
            }
            // sand row gets splash chance only if water row is reaching too
            if row == 0 {
                let extend = ((h >> 8) % 2) == 0;
                if !extend {
                    continue;
                }
            }
            // prefer the freshest splash so multiple don't fight
            if let Some((cur_t, _, _)) = active {
                if t < cur_t {
                    continue;
                }
            }
            active = Some((t, ax, h));
        }
        t = t.saturating_add(SPAWN_EVERY);
    }

    let (spawn_t, anchor_x, anchor_hash) = active?;
    let age = tick - spawn_t;
    let intensity = 1.0 - (age as f32 / LIFETIME as f32);

    let local_dx = x - anchor_x;
    let ch_hash = hash2(
        x,
        (spawn_t as i32).wrapping_add(local_dx * 7),
        0xCAFE_F00D,
    );
    // chaotic asymmetric glyph
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

    // fade with age, biased toward white/cream with occasional pale blue
    let _ = anchor_hash;
    let lum = (130.0 + intensity * 120.0).clamp(0.0, 255.0) as u8;
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
