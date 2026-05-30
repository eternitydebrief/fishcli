use crate::fish::FishDef;

pub struct Player {
    pub x: i32,
    pub y: i32,
    #[allow(dead_code)]
    pub name: String,
    pub valu: u64,
    pub inventory: Vec<&'static FishDef>,
}

impl Player {
    pub fn spawn() -> Self {
        Self {
            x: 0,
            y: 2,
            name: String::new(),
            valu: 0,
            inventory: Vec::new(),
        }
    }
}
