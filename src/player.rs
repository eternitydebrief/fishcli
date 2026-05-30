pub struct Player {
    pub x: i32,
    pub y: i32,
}

impl Player {
    pub fn spawn() -> Self {
        Self { x: 0, y: 2 }
    }
}
