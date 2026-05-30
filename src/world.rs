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
}

impl Tile {
    pub fn walkable(self) -> bool {
        matches!(self, Tile::Grass | Tile::Sand)
    }
}

pub struct World;

impl World {
    pub fn new(_seed: u32) -> Self {
        Self
    }

    pub fn get(&self, x: i32, y: i32) -> Tile {
        if let Some(t) = village_tile(x, y) {
            return t;
        }
        if y >= 6 {
            Tile::Water
        } else if y == 5 {
            Tile::Sand
        } else {
            Tile::Grass
        }
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
                'R',
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Tile::DoorSchool => (
                'S',
                Style::default()
                    .fg(Color::Magenta)
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
                    (',', Style::default().fg(Color::Yellow))
                }
            }
        }
    }
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

fn water_anim(x: i32, y: i32, tick: u64) -> (char, Style) {
    let phase =
        (x.unsigned_abs() as u64 + (y.unsigned_abs() as u64) * 3 + tick / 4) % 12;
    let glyph = match phase {
        0 | 1 => '~',
        2 | 3 => '=',
        4 => '-',
        5..=8 => '~',
        9 => '-',
        _ => '~',
    };
    let color = match phase {
        0..=2 => Color::Blue,
        3..=5 => Color::LightBlue,
        6..=8 => Color::Cyan,
        _ => Color::Blue,
    };
    (glyph, Style::default().fg(color))
}

fn grass_anim(x: i32, y: i32, tick: u64) -> (char, Style) {
    let seed = (x.unsigned_abs() as u64)
        .wrapping_mul(7)
        .wrapping_add((y.unsigned_abs() as u64).wrapping_mul(13));
    let phase = (seed + tick / 25) % 41;
    match phase {
        0 => (',', Style::default().fg(Color::LightGreen)),
        1 => ('\'', Style::default().fg(Color::LightGreen)),
        2 => ('`', Style::default().fg(Color::Green)),
        _ => ('.', Style::default().fg(Color::Green)),
    }
}

fn foam_anim(x: i32, tick: u64) -> (char, Style) {
    let phase = (x.unsigned_abs() as u64 * 3 + tick / 6) % 17;
    match phase {
        0 => (
            '*',
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        1 => ('o', Style::default().fg(Color::Gray)),
        2 => ('.', Style::default().fg(Color::White)),
        _ => (',', Style::default().fg(Color::Yellow)),
    }
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
