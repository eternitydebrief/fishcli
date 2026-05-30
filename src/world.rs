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
}

impl Tile {
    pub fn walkable(self) -> bool {
        matches!(
            self,
            Tile::Grass | Tile::Sand | Tile::Pebble | Tile::Flower | Tile::Path
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
        }
    }
}

pub struct World {
    pub seed: u32,
}

pub struct WorldView<'a> {
    pub world: &'a World,
    pub player: (i32, i32),
    pub player_facing: (i32, i32),
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
                    let g = match self.player_facing {
                        (0, -1) => '^',
                        (0, 1) => 'v',
                        (-1, 0) => '<',
                        (1, 0) => '>',
                        _ => '@',
                    };
                    cell.set_char(g).set_style(player_style);
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
        Tile::Grass
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
                    (g, Style::default().fg(shade((198, 182, 132), x, y, 0x5A1D_5A1D, 14)))
                }
            }
            Tile::TreeTrunk | Tile::TreeCanopy => tree_render(x, y, self.seed),
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

const WATER_CELL_W: i32 = 36;
const WATER_CELL_H: i32 = 22;

fn water_body_at(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 5 {
        return false; // ocean strip handled elsewhere
    }
    let cx = x.div_euclid(WATER_CELL_W);
    let cy = y.div_euclid(WATER_CELL_H);
    for dcy in -1..=1 {
        for dcx in -1..=1 {
            let ccx = cx + dcx;
            let ccy = cy + dcy;
            let h = hash2(ccx, ccy, seed.wrapping_add(0xF00D_BEEF));
            // ~12% of coarse cells host a water body
            if h % 8 != 0 {
                continue;
            }
            let ox = ((h >> 4) as i32).rem_euclid(WATER_CELL_W);
            let oy = ((h >> 12) as i32).rem_euclid(WATER_CELL_H);
            let ax = ccx * WATER_CELL_W + ox;
            let ay = ccy * WATER_CELL_H + oy;
            // horizontal-elongated size classes: (rx, ry) where rx > ry
            let (rx, ry): (i32, i32) = match (h >> 20) % 10 {
                0..=4 => (2, 1),  // puddle (wider than tall)
                5..=7 => (5, 2),  // pond
                8 => (8, 3),      // lake
                _ => (12, 4),     // long lake
            };
            if ay + ry >= 5 {
                continue;
            }
            // ellipse-ish test: stretched horizontally
            let dx = (x - ax) as f32 / rx.max(1) as f32;
            let dy = (y - ay) as f32 / ry.max(1) as f32;
            if dx * dx + dy * dy <= 1.0 {
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
    x.abs() <= 50 && (-18..=5).contains(&y)
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
    if water_body_at(x, y, seed) {
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
    let p = biome_params(biome_at(x, y, seed));
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

    // dock and well
    if (-6..=5).contains(&x) && (5..=8).contains(&y) {
        return Some(Tile::Dock);
    }
    if (x, y) == (0, -1) {
        return Some(Tile::Well);
    }
    // pathways inside the walls
    if village_path(x, y) {
        return Some(Tile::Path);
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
    (-3..=3).contains(&x)
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
