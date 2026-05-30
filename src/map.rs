use ratatui::{
    style::{Color, Style},
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
    pub fn glyph(self) -> char {
        match self {
            Tile::Grass => '.',
            Tile::Wall => '#',
            Tile::DoorRod => 'R',
            Tile::DoorSchool => 'S',
            Tile::Water => '~',
            Tile::Dock => '=',
            Tile::Sand => ',',
        }
    }

    pub fn color(self) -> Color {
        match self {
            Tile::Grass => Color::Green,
            Tile::Wall => Color::Gray,
            Tile::DoorRod => Color::Yellow,
            Tile::DoorSchool => Color::Magenta,
            Tile::Water => Color::Blue,
            Tile::Dock => Color::LightYellow,
            Tile::Sand => Color::Yellow,
        }
    }

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

    pub fn render_lines(&self, player: Option<(usize, usize)>) -> Vec<Line<'static>> {
        (0..self.height)
            .map(|y| {
                let spans: Vec<Span<'static>> = (0..self.width)
                    .map(|x| {
                        if player == Some((x, y)) {
                            Span::styled("@".to_string(), Style::default().fg(Color::White))
                        } else {
                            let t = self.get(x, y);
                            Span::styled(
                                t.glyph().to_string(),
                                Style::default().fg(t.color()),
                            )
                        }
                    })
                    .collect();
                Line::from(spans)
            })
            .collect()
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
