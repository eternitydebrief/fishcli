use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Grass,
    Wall,
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
        }
    }
}

pub struct World {
    pub seed: u32,
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
        if !in_village_zone(x, y) {
            if let Some(part) = tree_at(x, y, self.seed) {
                return part;
            }
            if big_rock_at(x, y, self.seed) {
                return Tile::BigRock;
            }
            let r = hash2(x, y, self.seed.wrapping_add(0x1234_5678)) as f32 / u32::MAX as f32;
            if r < 0.015 {
                return Tile::Rock;
            }
            if r < 0.055 {
                return Tile::Pebble;
            }
            if r < 0.085 {
                return Tile::Flower;
            }
        }
        Tile::Grass
    }

    pub fn render_viewport(
        &self,
        player: (i32, i32),
        viewport_w: usize,
        viewport_h: usize,
        tick: u64,
    ) -> Vec<Line<'static>> {
        if viewport_w == 0 || viewport_h == 0 {
            return Vec::new();
        }
        let half_w = (viewport_w as i32) / 2;
        let half_h = (viewport_h as i32) / 2;
        (0..viewport_h as i32)
            .map(|sy| {
                let spans: Vec<Span<'static>> = (0..viewport_w as i32)
                    .map(|sx| {
                        if sx == half_w && sy == half_h {
                            return Span::styled(
                                "@".to_string(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            );
                        }
                        let wx = player.0 - half_w + sx;
                        let wy = player.1 - half_h + sy;
                        let (g, s) = self.render_tile(wx, wy, tick);
                        Span::styled(g.to_string(), s)
                    })
                    .collect();
                Line::from(spans)
            })
            .collect()
    }

    fn render_tile(&self, x: i32, y: i32, tick: u64) -> (char, Style) {
        match self.get(x, y) {
            Tile::Wall => ('#', Style::default().fg(Color::Gray)),
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
            Tile::Grass => grass_anim(x, y, tick),
            Tile::Water => water_anim(x, y, tick),
            Tile::Sand => {
                let shore = matches!(self.get(x, y + 1), Tile::Water);
                if shore {
                    foam_anim(x, tick)
                } else {
                    let g = match hash2(x, y, 0x5A1D_5A1D) % 3 {
                        0 => ',',
                        1 => '.',
                        _ => '`',
                    };
                    (g, Style::default().fg(shade((220, 200, 130), x, y, 0x5A1D_5A1D, 20)))
                }
            }
            Tile::TreeTrunk | Tile::TreeCanopy => tree_render(x, y, self.seed),
            Tile::Rock => rock_glyph(x, y),
            Tile::BigRock => big_rock_glyph(x, y, self.seed),
            Tile::Pebble => pebble_glyph(x, y),
            Tile::Flower => flower_glyph(x, y),
        }
    }
}

fn is_big_rock_anchor(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 {
        return false;
    }
    let r = hash2(x, y, seed.wrapping_add(0xBEEF_FACE)) as f32 / u32::MAX as f32;
    r < 0.004
}

fn big_rock_at(x: i32, y: i32, seed: u32) -> bool {
    for dx in 0..2i32 {
        for dy in 0..2i32 {
            let ax = x - dx;
            let ay = y - dy;
            if is_big_rock_anchor(ax, ay, seed) {
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

fn is_tree_anchor(x: i32, y: i32, seed: u32) -> bool {
    if in_village_zone(x, y) {
        return false;
    }
    if y >= 4 || y <= -40 {
        return false;
    }
    let r = hash2(x, y, seed.wrapping_add(0xC0DE_C0DE)) as f32 / u32::MAX as f32;
    r < 0.03
}

fn tree_at(x: i32, y: i32, seed: u32) -> Option<Tile> {
    for dy in 0..=2i32 {
        for dx in -1..=1i32 {
            let ax = x + dx;
            let ay = y + dy;
            if !is_tree_anchor(ax, ay, seed) {
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
            if !is_tree_anchor(ax, ay, seed) {
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
            leaf_style(g, anchor_hash, (60, 95, 55), x, y)
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
            // pine is darker / cooler
            leaf_style(g, anchor_hash, (40, 80, 50), x, y)
        }
        (TreeSpecies::Bush, _) => {
            let g = match anchor_hash % 3 {
                0 => 'o',
                1 => '*',
                _ => 'q',
            };
            leaf_style(g, anchor_hash, (75, 100, 55), x, y)
        }
    }
}

fn trunk_style(anchor_hash: u32, g: char) -> (char, Style) {
    let r = 90 + (anchor_hash % 25) as u8;
    let gc = 60 + (anchor_hash % 20) as u8;
    let b = 35 + (anchor_hash % 15) as u8;
    (
        g,
        Style::default()
            .fg(Color::Rgb(r, gc, b))
            .add_modifier(Modifier::BOLD),
    )
}

fn leaf_style(g: char, anchor_hash: u32, base: (u8, u8, u8), x: i32, y: i32) -> (char, Style) {
    let tint_r = (anchor_hash % 25) as i32 - 12;
    let tint_g = ((anchor_hash >> 8) % 25) as i32 - 12;
    let tint_b = ((anchor_hash >> 16) % 25) as i32 - 12;
    let base = (
        (base.0 as i32 + tint_r).clamp(0, 255) as u8,
        (base.1 as i32 + tint_g).clamp(0, 255) as u8,
        (base.2 as i32 + tint_b).clamp(0, 255) as u8,
    );
    (
        g,
        Style::default()
            .fg(shade(base, x, y, 0xAA55_AA56, 14))
            .add_modifier(Modifier::BOLD),
    )
}

fn rock_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xF00D_F00D) % 3;
    let (g, base) = match v {
        0 => ('o', (110, 110, 110)),
        1 => ('O', (140, 140, 140)),
        _ => ('@', (100, 100, 100)),
    };
    (
        g,
        Style::default()
            .fg(shade(base, x, y, 0xF00D_F00D, 18))
            .add_modifier(Modifier::BOLD),
    )
}

fn pebble_glyph(x: i32, y: i32) -> (char, Style) {
    let v = hash2(x, y, 0xABCD_1234) % 3;
    let g = match v {
        0 => '.',
        1 => ',',
        _ => '`',
    };
    (g, Style::default().fg(shade((130, 120, 100), x, y, 0xABCD_1234, 20)))
}

fn flower_glyph(x: i32, y: i32) -> (char, Style) {
    let h = hash2(x, y, 0xFFEE_DD11) % 7;
    let color = match h {
        0 => Color::Rgb(230, 80, 80),
        1 => Color::Rgb(240, 180, 60),
        2 => Color::Rgb(250, 240, 80),
        3 => Color::Rgb(100, 220, 100),
        4 => Color::Rgb(80, 160, 240),
        5 => Color::Rgb(170, 100, 240),
        _ => Color::Rgb(250, 130, 220),
    };
    ('*', Style::default().fg(color).add_modifier(Modifier::BOLD))
}

fn big_rock_glyph(x: i32, y: i32, seed: u32) -> (char, Style) {
    let mut anchor = (0, 0);
    let mut found = false;
    'find: for dy in 0..2i32 {
        for dx in 0..2i32 {
            let ax = x - dx;
            let ay = y - dy;
            if is_big_rock_anchor(ax, ay, seed) {
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
        return Some(Tile::Wall);
    }
    let in_right_house = (18..=22).contains(&x) && (-3..=1).contains(&y);
    if in_right_house {
        if x == 20 && y == 1 {
            return Some(Tile::DoorSchool);
        }
        return Some(Tile::Wall);
    }
    if (-6..=5).contains(&x) && (5..=8).contains(&y) {
        return Some(Tile::Dock);
    }
    None
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
    let phase =
        (x.unsigned_abs() as u64 + (y.unsigned_abs() as u64) * 3 + tick / 4) % 12;
    let glyph = match phase {
        0 | 1 => '~',
        2 | 3 => '-',
        4 => '.',
        5..=8 => '~',
        9 => '-',
        _ => '~',
    };
    let base = match phase {
        0..=2 => (20, 60, 160),
        3..=5 => (40, 110, 200),
        6..=8 => (60, 180, 220),
        _ => (30, 80, 180),
    };
    (glyph, Style::default().fg(shade(base, x, y, 0xA11_BABE, 18)))
}

fn grass_anim(x: i32, y: i32, tick: u64) -> (char, Style) {
    let seed = (x.unsigned_abs() as u64)
        .wrapping_mul(7)
        .wrapping_add((y.unsigned_abs() as u64).wrapping_mul(13));
    let phase = (seed + tick / 25) % 41;
    let g = match phase {
        0 => ',',
        1 => '\'',
        2 => '`',
        _ => '.',
    };
    (g, Style::default().fg(shade((50, 130, 50), x, y, 0x6C00_6C00, 25)))
}

fn foam_anim(x: i32, tick: u64) -> (char, Style) {
    let phase = (x.unsigned_abs() as u64 * 3 + tick / 6) % 17;
    let (g, base) = match phase {
        0 => ('*', (240, 240, 240)),
        1 => ('o', (200, 200, 200)),
        2 => ('.', (230, 230, 230)),
        _ => (',', (210, 190, 130)),
    };
    (
        g,
        Style::default()
            .fg(shade(base, x, 0, 0xF0AA_F0AA, 12))
            .add_modifier(Modifier::BOLD),
    )
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
