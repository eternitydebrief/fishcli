pub struct Player {
    pub x: usize,
    pub y: usize,
}

impl Player {
    pub fn spawn() -> Self {
        Self { x: 30, y: 9 }
    }
}
