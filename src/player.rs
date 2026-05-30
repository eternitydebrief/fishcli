use crate::fish::FishDef;
use crate::item::Item;
use crate::rod::OwnedRods;

pub struct Player {
    pub x: i32,
    pub y: i32,
    #[allow(dead_code)]
    pub name: String,
    pub valu: u64,
    pub inventory: Vec<&'static FishDef>,
    pub items: Vec<Item>,
    pub facing: (i32, i32),
    pub rods: OwnedRods,
}

impl Player {
    pub fn spawn() -> Self {
        Self {
            x: 0,
            y: 2,
            name: String::new(),
            valu: 0,
            inventory: Vec::new(),
            items: Vec::new(),
            facing: (0, 1),
            rods: OwnedRods { max_owned: 1, equipped: 1 },
        }
    }
}
