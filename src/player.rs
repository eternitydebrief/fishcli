use crate::map::Map;

pub struct Player {
    pub x: usize,
    pub y: usize,
}

impl Player {
    pub fn spawn() -> Self {
        Self { x: 30, y: 9 }
    }

    pub fn try_move(&mut self, map: &Map, dx: i32, dy: i32) {
        let nx = self.x as i32 + dx;
        let ny = self.y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= map.width as i32 || ny >= map.height as i32 {
            return;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        if map.get(nx, ny).walkable() {
            self.x = nx;
            self.y = ny;
        }
    }
}
