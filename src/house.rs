//! Procedural one-room house interiors. Each house's furniture layout is
//! deterministic per seed (derived from the door's world coords).
//!
//! Layout grid is fixed-size (WIDTH x HEIGHT). The bottom-center cell is
//! the exit; stepping on it returns the player to the overworld.

// Terminal cells are ~2:1 (height:width); a 28x8 grid renders as visually
// 28:16 = 1.75:1 wide, which reads as a proper horizontal room.
pub const WIDTH: i32 = 28;
pub const HEIGHT: i32 = 8;
pub const EXIT_X: i32 = WIDTH / 2;
pub const EXIT_Y: i32 = HEIGHT - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Furn {
    Floor,
    Wall,
    Window,
    Bed,
    Pillow,
    Stove,
    Counter,
    Sink,
    Table,
    Chair,
    Rug,
    Exit,
}

impl Furn {
    pub fn walkable(self) -> bool {
        matches!(self, Furn::Floor | Furn::Rug | Furn::Exit | Furn::Chair)
    }

    pub fn id_str(self) -> &'static str {
        match self {
            Furn::Floor => "Floor",
            Furn::Wall => "Wall",
            Furn::Window => "Window",
            Furn::Bed => "Bed",
            Furn::Pillow => "Pillow",
            Furn::Stove => "Stove",
            Furn::Counter => "Counter",
            Furn::Sink => "Sink",
            Furn::Table => "Table",
            Furn::Chair => "Chair",
            Furn::Rug => "Rug",
            Furn::Exit => "Exit",
        }
    }

    pub fn describe(self) -> &'static str {
        crate::inspect_text::get(&format!("furn:{}", self.id_str()))
    }
}

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = seed
        .wrapping_add((x as u32).wrapping_mul(374_761_393))
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// Cell at (x, y) in the interior. Out-of-bounds returns Wall.
pub fn tile_at(x: i32, y: i32, seed: u32) -> Furn {
    if x < 0 || y < 0 || x >= WIDTH || y >= HEIGHT {
        return Furn::Wall;
    }
    // perimeter walls
    if x == 0 || x == WIDTH - 1 || y == 0 || y == HEIGHT - 1 {
        // exit at bottom-center
        if y == HEIGHT - 1 && x == EXIT_X {
            return Furn::Exit;
        }
        // windows on the top wall, spaced
        if y == 0 && (x == 4 || x == WIDTH - 5) {
            return Furn::Window;
        }
        // side windows alternate per house
        if x == 0 && y == 3 && hash(seed as i32, 0, seed) & 1 == 0 {
            return Furn::Window;
        }
        if x == WIDTH - 1 && y == HEIGHT - 4 && hash(seed as i32, 1, seed) & 1 == 1 {
            return Furn::Window;
        }
        return Furn::Wall;
    }

    // Bed in the top-left (3-wide, 2-tall, with pillow at the head).
    if (1..=3).contains(&x) && (1..=2).contains(&y) {
        if y == 1 && x == 1 {
            return Furn::Pillow;
        }
        return Furn::Bed;
    }

    // Kitchen along the top-right: stove, counter run, sink.
    if y == 1 && (WIDTH - 6..=WIDTH - 2).contains(&x) {
        return match x - (WIDTH - 6) {
            0 => Furn::Stove,
            1 | 2 => Furn::Counter,
            3 => Furn::Sink,
            _ => Furn::Counter,
        };
    }

    // Table and chairs around the room's centre. Seed-driven nudge for variety.
    let nudge = (hash(0, 0, seed) % 3) as i32 - 1;
    let tx = WIDTH / 2 + nudge;
    let ty = HEIGHT / 2;
    if (x, y) == (tx, ty) {
        return Furn::Table;
    }
    if (x, y) == (tx - 1, ty) || (x, y) == (tx + 1, ty) {
        return Furn::Chair;
    }

    // A rug stamped underneath the table area.
    if (tx - 1..=tx + 1).contains(&x) && (ty - 1..=ty + 1).contains(&y) {
        return Furn::Rug;
    }

    Furn::Floor
}
