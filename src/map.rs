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

pub struct Map {
    pub width: usize,
    pub height: usize,
    cells: Vec<Tile>,
}

impl Map {
    pub fn starter() -> Self {
        const RAW: &[&str] = &[
            "............................................................",
            "..#####.............................................#####...",
            "..#...#.............................................#...#...",
            "..#...#.............................................#...#...",
            "..#.R.#.............................................#.S.#...",
            "..#####.............................................#####...",
            "............................................................",
            "............................................................",
            "............................................................",
            "............................................................",
            "............................................................",
            "............................................................",
            ",,,,,,,,,,,,,,,,,,,,,,,,============,,,,,,,,,,,,,,,,,,,,,,,,",
            "~~~~~~~~~~~~~~~~~~~~~~~~============~~~~~~~~~~~~~~~~~~~~~~~~",
            "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ];

        let height = RAW.len();
        let width = RAW[0].len();
        let mut cells = Vec::with_capacity(width * height);
        for (y, row) in RAW.iter().enumerate() {
            assert_eq!(row.len(), width, "map row {y} length mismatch");
            for ch in row.chars() {
                let tile = match ch {
                    '.' => Tile::Grass,
                    '#' => Tile::Wall,
                    'R' => Tile::DoorRod,
                    'S' => Tile::DoorSchool,
                    '~' => Tile::Water,
                    '=' => Tile::Dock,
                    ',' => Tile::Sand,
                    other => panic!("unknown tile char {other:?}"),
                };
                cells.push(tile);
            }
        }

        Self { width, height, cells }
    }

    pub fn get(&self, x: usize, y: usize) -> Tile {
        self.cells[y * self.width + x]
    }

    pub fn render_lines(&self, player: Option<(usize, usize)>, tick: u64) -> Vec<Line<'static>> {
        (0..self.height)
            .map(|y| {
                let spans: Vec<Span<'static>> = (0..self.width)
                    .map(|x| {
                        if player == Some((x, y)) {
                            Span::styled(
                                "@".to_string(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            let (g, style) = self.render_tile(x, y, tick);
                            Span::styled(g.to_string(), style)
                        }
                    })
                    .collect();
                Line::from(spans)
            })
            .collect()
    }

    fn render_tile(&self, x: usize, y: usize, tick: u64) -> (char, Style) {
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
                let shore = y + 1 < self.height && matches!(self.get(x, y + 1), Tile::Water);
                if shore {
                    foam_anim(x, tick)
                } else {
                    (',', Style::default().fg(Color::Yellow))
                }
            }
        }
    }
}

fn water_anim(x: usize, y: usize, tick: u64) -> (char, Style) {
    let phase = (x as u64 + (y as u64) * 3 + tick / 4) % 12;
    let glyph = match phase {
        0 | 1 => '~',
        2 | 3 => '=',
        4 => '-',
        5 | 6 => '~',
        7 | 8 => '~',
        9 => '-',
        10 | 11 => '~',
        _ => '~',
    };
    let base = match phase {
        0..=2 => Color::Blue,
        3..=5 => Color::LightBlue,
        6..=8 => Color::Cyan,
        _ => Color::Blue,
    };
    (glyph, Style::default().fg(base))
}

fn grass_anim(x: usize, y: usize, tick: u64) -> (char, Style) {
    let seed = x.wrapping_mul(7).wrapping_add(y.wrapping_mul(13)) as u64;
    let phase = (seed + tick / 25) % 41;
    match phase {
        0 => (',', Style::default().fg(Color::LightGreen)),
        1 => ('\'', Style::default().fg(Color::LightGreen)),
        2 => ('`', Style::default().fg(Color::Green)),
        _ => ('.', Style::default().fg(Color::Green)),
    }
}

fn foam_anim(x: usize, tick: u64) -> (char, Style) {
    let phase = (x as u64 * 3 + tick / 6) % 17;
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
    fn starter_parses() {
        let m = Map::starter();
        assert!(m.width > 0 && m.height > 0);
    }
}
