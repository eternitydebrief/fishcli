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
    /// True once the Shipwright has built the player a boat. Required to
    /// board via `:inspect` on a water tile.
    pub has_boat: bool,
    /// Currently on the boat. While true the player glyph is '8' and water
    /// tiles act like solid ground (faster than swimming). Set true by
    /// inspecting water, set false by stepping onto land.
    pub on_boat: bool,
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
            has_boat: false,
            on_boat: false,
        }
    }
}
