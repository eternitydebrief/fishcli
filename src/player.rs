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
    /// Required to mine ore veins. Bought from the Miner NPC.
    pub has_pickaxe: bool,
    /// Currently-equipped tackle (hat / vest / line / lure).
    pub tackle: crate::tackle::EquippedTackle,
    /// Bait stock + active bait.
    pub bait: crate::bait::BaitStock,
    /// Currently-equipped forged gear (feet/neck/ring/cape/pickaxe) and
    /// the registry of every piece ever crafted.
    pub gear: crate::gear::EquippedGear,
    /// Ingot stockpile: ore name -> count. Smelting pushes here, forging
    /// consumes from here. Raw ore items stay on `items` (Mineral category)
    /// until smelted.
    pub ingots: std::collections::BTreeMap<String, u32>,
    /// Hull tier of the boat. 0 = no boat. 1 = basic skiff (shore + shallow
    /// ocean), 6 = Fog Sea capable. Gates how deep the player can push the
    /// boat away from shore. Set to 1 by the Shipwright on initial build.
    pub hull_tier: u32,
    /// Crew hunger gauge (0..=100). +1 per catch made while `on_boat`. Use
    /// `:feed <n>` to sacrifice fish from the basket (-3 hunger per fish).
    /// At 100 the crew refuses to row — fishing is blocked until you feed
    /// them. Idle on land = no drain.
    pub crew_hunger: u32,
    /// Engine biofuel (0..=200). Drains 1 per step taken while `on_boat`.
    /// Hitting 0 dumps you back at the home pier and disembarks the boat.
    /// Refill with `:burn <n>` — each fish yields `5 * difficulty` units.
    pub biofuel: u32,
    /// Logs in the player's stack from `:chop`-ping trees. Currency for
    /// shipwright hull upgrades.
    pub wood: u32,
}

impl Player {
    /// Effective pickaxe tier the player can swing. 0 = none. The starter
    /// pickaxe (granted by the Miner NPC and tracked via `has_pickaxe`)
    /// counts as tier 1. Forged pickaxes equipped in the gear slot upgrade
    /// the tier beyond that.
    pub fn pickaxe_tier(&self) -> u32 {
        let base = if self.has_pickaxe { 1 } else { 0 };
        base.max(self.gear.combined_perks().pickaxe_tier)
    }
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
            has_pickaxe: false,
            tackle: crate::tackle::EquippedTackle::default(),
            bait: crate::bait::BaitStock::default(),
            gear: crate::gear::EquippedGear::default(),
            ingots: std::collections::BTreeMap::new(),
            hull_tier: 0,
            crew_hunger: 0,
            biofuel: 0,
            wood: 0,
        }
    }
}

/// Hull tier limits. `ocean_depth_max(t)` = the deepest `ocean_depth_at`
/// value the boat is allowed to push into at hull tier `t`. Beyond the
/// max for tier 6, the player crosses into the Fog Sea.
pub fn ocean_depth_max(hull_tier: u32) -> u32 {
    match hull_tier {
        0 => 0,
        1 => 4,
        2 => 8,
        3 => 14,
        4 => 22,
        5 => 32,
        _ => u32::MAX, // tier 6+: no limit; Fog Sea reachable
    }
}

/// (valu_cost, wood_cost) to upgrade from tier `from` to tier `from + 1`.
pub fn hull_upgrade_cost(from: u32) -> Option<(u64, u32)> {
    Some(match from {
        0 => (1_000, 10),     // Build the first hull (also a shipwright gate)
        1 => (5_000, 25),
        2 => (25_000, 60),
        3 => (100_000, 150),
        4 => (500_000, 350),
        5 => (2_000_000, 900),
        _ => return None,
    })
}

/// Display label per hull tier.
pub fn hull_label(tier: u32) -> &'static str {
    match tier {
        0 => "no hull",
        1 => "Skiff",
        2 => "Coastal Cutter",
        3 => "Bluewater Trawler",
        4 => "Deep Hauler",
        5 => "Abyssal Frigate",
        _ => "Fog Walker",
    }
}
