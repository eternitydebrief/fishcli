use crate::fish::FishDef;
use crate::fishdex::Fishdex;
use crate::fishing::{Fishing, FishingResult};
use crate::fishlist;
use crate::item::{Category, Item};
use crate::narrator::Narrator;
use crate::notes;
use crate::npc::{self, Npc};
use crate::player::Player;
use crate::quest;
use crate::save::{self, SaveData};
use crate::stats::{fish_catch_xp, Skills, Stats};
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use crate::world::{biome_at, Biome, Tile, World, WorldView};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// A headline event surfaced at the top of the viewport. Each kind picks
/// its own border colour and prefix; the body is a plain string the
/// caller controls. Many banners can coexist — they stack vertically in
/// the order they were pushed.
#[derive(Clone)]
pub enum BannerKind {
    Discovery,
    Recipe,
    Achievement,
    Xp {
        level: u32,
        total_xp: u64,
    },
}

#[derive(Clone)]
pub struct Banner {
    pub kind: BannerKind,
    pub line: String,
    pub ttl_ticks: u32,
}

/// Reward awarded once when an encyclopedia's running counter crosses
/// `threshold`. Stacks with achievement chain rewards.
struct EncyclopediaMilestone {
    threshold: u32,
    label: &'static str,
    valu: u64,
    skill_points: u32,
    /// Persistent buff string (see `buffs::apply_effect`). None = no buff.
    permanent_effect: Option<&'static str>,
}

const FISHDEX_MILES: &[EncyclopediaMilestone] = &[
    EncyclopediaMilestone { threshold:  25, label: "Apprentice Naturalist", valu:    500, skill_points: 1, permanent_effect: None },
    EncyclopediaMilestone { threshold:  50, label: "Field Cataloguer",      valu:   2_000, skill_points: 1, permanent_effect: Some("luck:0.01") },
    EncyclopediaMilestone { threshold: 100, label: "Hundred Hands",         valu:   5_000, skill_points: 2, permanent_effect: Some("price_mult:0.02") },
    EncyclopediaMilestone { threshold: 200, label: "Bestiary Keeper",       valu:  15_000, skill_points: 3, permanent_effect: Some("luck:0.03") },
    EncyclopediaMilestone { threshold: 350, label: "Living Library",        valu:  50_000, skill_points: 5, permanent_effect: Some("price_mult:0.05") },
    EncyclopediaMilestone { threshold: 500, label: "World Compendium",      valu: 200_000, skill_points: 8, permanent_effect: Some("price_mult:0.08") },
    EncyclopediaMilestone { threshold: 700, label: "All Things That Swim",  valu: 1_000_000, skill_points: 15, permanent_effect: Some("price_mult:0.15") },
];

const COOKBOOK_MILES: &[EncyclopediaMilestone] = &[
    EncyclopediaMilestone { threshold:  1, label: "First Mastery",     valu:    500, skill_points: 1, permanent_effect: None },
    EncyclopediaMilestone { threshold:  3, label: "Apprentice Cook",   valu:  2_000, skill_points: 1, permanent_effect: Some("price_mult:0.01") },
    EncyclopediaMilestone { threshold:  6, label: "Journeyman Cook",   valu:  8_000, skill_points: 2, permanent_effect: Some("wait_mult:-0.01") },
    EncyclopediaMilestone { threshold: 10, label: "Master Cook",       valu: 30_000, skill_points: 3, permanent_effect: Some("price_mult:0.03") },
    EncyclopediaMilestone { threshold: 15, label: "Grand Chef",        valu: 100_000, skill_points: 5, permanent_effect: Some("walk_speed:0.02") },
    EncyclopediaMilestone { threshold: 20, label: "Pantheon Palate",   valu: 500_000, skill_points: 10, permanent_effect: Some("price_mult:0.10") },
];

pub enum Scene {
    Overworld,
    RodShop { cursor: u32 },
    FishingSchool { cursor: usize, tab: usize },
    Fishing(Fishing),
    Fishdex(Fishdex),
    NamePrompt(String),
    Dialogue {
        npc: &'static Npc,
        line: usize,
    },
    Notes(NotesBuf),
    Inventory {
        tab: usize,
    },
    Help(HelpTopic),
    Stats,
    Settings,
    Quests { cursor: usize },
    Map { offset: (i32, i32) },
    /// Hidden developer console. Only reachable via a SHA-512 gated command.
    /// Cursor selects an editable field or action; h/l adjust by step,
    /// H/L by big step, enter triggers actions.
    Debug { cursor: usize },
    /// The Rod's loot-pool selector. Pressing `k` while in overworld with
    /// tier 202 equipped opens this. Picking a pool sets
    /// `current_pool_override` and lets the player fish anywhere.
    LootPool { cursor: usize },
    /// Fishmonger sell flow: pick which fish (j/k navigate, enter),
    /// then how many (All / One / Custom X), then sell.
    Fishmonger {
        cursor: usize,
        step: FishmongerStep,
    },
    /// Type-the-ore minigame. Each keypress advances if it matches the
    /// next expected character of the ore's name; wrong keys are ignored.
    /// Completing the name grants the ore and consumes a vein charge.
    Mining(crate::mining::Mining),
    /// Tackle shop: 4 slot tabs (hat/vest/line/lure). h/l switch tab,
    /// j/k navigate tiers, enter buys the next tier of the active slot.
    TackleShop { slot_idx: usize, cursor: usize },
    /// Bait shop: list of baits with current stock; enter buys 1, B equips
    /// the active bait. q/esc leaves.
    BaitShop { cursor: usize },
    /// Shipwright hull-upgrade menu: cursor over the next available tier.
    /// Enter pays valu + wood, bumps `hull_tier` (and grants `has_boat` on
    /// the first build). q/esc leaves.
    Shipwright { cursor: usize },
    /// Lumberjacking minigame: type the displayed F/G/H/J sequence to
    /// chop the tree. Wrong key locks input for 3s and halves the yield.
    Chopping(crate::chop::Chopping),
    /// Cooking menu: scrollable recipe list. Rows are dimmed when the
    /// player is missing ingredients; enter cooks the highlighted dish
    /// (consumes ingredients, applies stamina + effect). `/` opens the
    /// filter editor; typed text matches recipe name + ingredient names.
    Cooking {
        cursor: usize,
        filter: String,
        editing_filter: bool,
    },
    /// Achievements menu: shows active (next-unmet) tier per chain plus
    /// completed-tier history at the bottom. q/esc leaves.
    Achievements { cursor: usize },
    /// Boss fishing fight: two fish and two bars, F/V drive the left bar
    /// and J/N drive the right.
    Boss(crate::boss::Boss),
    /// Bug-net micro-game. The player faced a bug tile and pressed `f`.
    /// Space attempts the catch when the cursor is inside the target zone.
    BugCatch(crate::bug_catch::BugCatch),
    /// Scales spend menu. j/k pick an axis, enter spends 1 scale.
    Scales { cursor: usize },
    /// Lure-bench crafting menu. j/k pick a recipe, enter crafts (consumes
    /// bait inputs + valu, writes to BaitStock).
    LureBench { cursor: usize },
    /// Per-frame perf instrumentation viewer. esc/q leaves.
    Perf,
    /// Blacksmith menu. Reached by pressing `f` on a Blacksmith NPC.
    /// Branches to Smelt / Forge / sell-ore / sell-gear / leave.
    Blacksmith {
        cursor: u8,
    },
    /// Forged-gear sell picker. Lists every piece in `owned`; Enter sells
    /// the cursor row at (sum of ingot value in its recipe) * 1.10.
    SellGear {
        cursor: usize,
    },
    /// Gear-slot manager. `slot_idx` tabs the slot (Feet / Neck / Ring /
    /// Pickaxe — Cape is auto-managed and not editable). `item_idx` picks
    /// from the owned items eligible for that slot. Enter equips, `u`
    /// unequips.
    Gear {
        slot_idx: usize,
        item_idx: usize,
    },
    /// Blacksmith smelt UI. Lists every ore the player has at least
    /// `ore_per_ingot` of; cursor picks which row. Typing "smelt" once
    /// consumes that ore stack and produces one ingot of the same type.
    Smelt {
        cursor: usize,
        typed: String,
    },
    /// Blacksmith forge UI. Lists every gear def the player meets the
    /// blacksmithing-level + ingot + valu requirements for. Cursor picks
    /// a row; typing the gear's name forges it (consumes ingots + valu).
    Forge {
        cursor: usize,
        typed: String,
    },
    /// Inside someone's house. Procedural one-room interior keyed by the
    /// world-coords of the door used to enter. Player moves with hjkl in
    /// a small grid; stepping back onto the interior door tile exits.
    HouseInterior {
        /// Player position within the interior grid.
        px: i32,
        py: i32,
        /// Overworld coords to restore the player to on exit.
        return_xy: (i32, i32),
        /// Seed for procedural furniture layout (derived from door coords).
        seed: u32,
    },
}

#[derive(Clone, Debug)]
pub enum FishmongerStep {
    /// Browsing the basket — pick which fish type to sell.
    PickFish,
    /// A fish is chosen (`picked_name` + max available). Pick a quantity option.
    PickQuantity { picked: String, max: u64 },
    /// Custom quantity input — typed digits into `buf`.
    EnterQuantity { picked: String, max: u64, buf: String },
    /// Final confirmation before money changes hands. Shows the total
    /// payout for transparency; y/Enter confirms, n/Esc cancels.
    Confirm { picked: String, qty: u64, total: u64 },
}

#[derive(Clone, Copy)]
pub enum HelpTopic {
    Controls,
    Commands,
}

pub struct NotesBuf {
    /// each line as its own String
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

impl NotesBuf {
    pub fn from_text(t: &str) -> Self {
        let lines: Vec<String> = if t.is_empty() {
            vec![String::new()]
        } else {
            t.split('\n').map(String::from).collect()
        };
        let last_row = lines.len().saturating_sub(1);
        let last_col = lines[last_row].chars().count();
        Self {
            lines,
            cursor_row: last_row,
            cursor_col: last_col,
        }
    }

    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }
}

pub enum Mode {
    Insert,
    Normal,
    Command(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CastPhase {
    Casting,
    Waiting,
    Biting,
}

pub struct CastState {
    pub phase: CastPhase,
    pub fish: &'static crate::fish::FishDef,
    pub bobber: (i32, i32),
    pub cast_pos: f32,
    pub cast_vel: f32,
    pub cast_strength: f32,
    pub wait_ticks_left: u32,
    pub bite_ticks_left: u32,
    /// True when the bobber landed inside an active hotspot patch.
    /// Boosts catch speed and zeroes out the trash gate.
    pub hotspot: bool,
}

/// Horizontal movement interval (ticks/step). Smaller because terminal cells
/// are roughly 2:1 - a vertical step covers ~2x the visual distance of a horizontal one.
const MOVE_INTERVAL_H: u64 = 2;
// Vertical was 4 (twice horizontal, matching 2:1 cell aspect). Slightly
// snappier at 3 — still visually slower than horizontal but doesn't
// feel sluggish.
const MOVE_INTERVAL_V: u64 = 3;

/// Forced game viewport. Terminals smaller than (MIN_W, MIN_H) get an
/// apologetic "make the window bigger" message. Terminals at or beyond
/// (MAX_W, MAX_H) get blank letterbox padding around a viewport capped
/// at MAX_*. Anything in between renders at the real terminal size.
pub const MIN_W: u16 = 140;
pub const MIN_H: u16 = 30;
pub const MAX_W: u16 = 150;
pub const MAX_H: u16 = 50;

/// Computes the centered viewport rect inside the terminal area. Width
/// stretches between MIN_W and MAX_W; height between MIN_H and MAX_H.
pub fn viewport(frame: &ratatui::Frame) -> Rect {
    let full = frame.area();
    let w = full.width.min(MAX_W);
    let h = full.height.min(MAX_H);
    let x = full.x + full.width.saturating_sub(w) / 2;
    let y = full.y + full.height.saturating_sub(h) / 2;
    Rect { x, y, width: w, height: h }
}

fn render_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let msg = format!(
        "Please lower the font size or stretch your terminal. (need {}x{}, have {}x{})",
        MIN_W, MIN_H, area.width, area.height,
    );
    let para = ratatui::widgets::Paragraph::new(msg)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    // center vertically too: use a 1-row band in the middle.
    let mid_y = area.y + area.height / 2;
    let band = Rect {
        x: area.x,
        y: mid_y,
        width: area.width,
        height: 3.min(area.height),
    };
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(para, band);
}

pub struct App {
    pub world: World,
    pub player: Player,
    pub scene: Scene,
    pub mode: Mode,
    pub running: bool,
    pub anim_tick: u64,
    pub rng_state: u32,
    pub caught: Vec<bool>,
    /// First-catch location per fish index (biome label, water type).
    /// `None` if never caught yet (or caught before tracking existed).
    pub caught_at: Vec<Option<(String, String)>>,
    /// First-catch context (time-of-day label, weather label, season label).
    pub caught_context: Vec<Option<(String, String, String)>>,
    /// Set when a cast becomes a hooked fight; consumed on catch to record
    /// the first-catch location for that species.
    pub pending_catch_loc: Option<(String, String)>,
    pub narrator: Narrator,
    /// quest id -> progress count
    pub quest_progress: HashMap<String, u32>,
    /// quest ids that have been completed and rewarded
    pub quest_done: Vec<String>,
    /// most recent biome the player stepped into
    pub current_biome: Option<Biome>,
    /// label currently shown in the location popup (village name or biome)
    pub current_location: Option<String>,
    /// shown when location changes, ticks down to 0
    pub biome_popup_ticks: u32,
    /// Active banner stack — discoveries, achievements, xp gains and any
    /// other "headline" event share this queue and render top-down so a
    /// burst of events doesn't have one obscure another.
    pub banners: Vec<Banner>,
    /// total valu earned lifetime (sum of quest rewards + sales)
    pub lifetime_valu: u64,
    /// time when this session started (for play-time stat)
    pub session_start: std::time::Instant,
    /// instant of the last accepted step in Overworld/HouseInterior. Used
    /// to cap movement rate so mashing arrows + WASD (or holding them under
    /// keyboard autorepeat) doesn't let the player zip across the map
    /// faster than intended.
    pub last_step_at: std::time::Instant,
    /// play time loaded from save (excluding this session)
    pub saved_play_secs: u64,
    /// id of the quest currently pinned to the top-left tracker
    pub pinned_quest: Option<String>,
    /// coarse map cells (each 4w x 2h world cells) the player has explored
    /// Per-dimension fog of war. Each dim keeps its own discovered tiles so
    /// surface exploration doesn't reveal the mines, and vice versa.
    pub seen_cells: std::collections::HashMap<
        crate::world::Dimension,
        std::collections::HashSet<(i32, i32)>,
    >,
    pub stats: Stats,
    pub skills: Skills,
    pub buffs: crate::buffs::Buffs,
    pub skill_tree: crate::skill_tree::SkillTree,
    /// Per-species catch count (parallel to `caught`). Mastery milestones
    /// at 1/5/10/25/50/100 each grant a skill point and a small permanent
    /// sale-value bonus for that species.
    pub mastery: Vec<u32>,
    /// Per-recipe cook count, parallel to `recipes::recipes()`. Mastery
    /// milestones at 5/25/100 boost the dish's effect magnitude (additive
    /// to the buff's `magnitude` param). Length is auto-extended on load.
    pub cooking_mastery: Vec<u32>,
    /// Per-bug-species catch count, parallel to `bugs::defs()`. Append-only;
    /// length is zero-extended on load to match the current bug count.
    pub bugs_caught: Vec<u32>,
    /// Cells where a bug was picked today, keyed by (dim, x, y). Suppresses
    /// the bug glyph for the rest of the day so the same bug can't be
    /// caught twice. Cleared when `bugs_picked_day_id` falls behind the
    /// current game day.
    pub bugs_picked_today: std::collections::HashSet<(crate::world::Dimension, i32, i32)>,
    /// In-game day-id the picked set was last populated for. On day rollover
    /// the set is dropped and rebuilt.
    pub bugs_picked_day_id: u64,
    /// Cells where a soil patch was dug today. Shares the day-rollover with
    /// `bugs_picked_today`.
    pub soil_dug_today: std::collections::HashSet<(crate::world::Dimension, i32, i32)>,
    /// Cells whose forageable object was searched today. Shares day rollover.
    pub foraged_today: std::collections::HashSet<(crate::world::Dimension, i32, i32)>,
    /// Scales: persistent token currency. Drops at ~5% per fish catch.
    pub scales: u64,
    /// Per-axis token spend. Read via `scales_bonus(axis)` and applied
    /// additively in the appropriate stat path.
    pub scales_spent: std::collections::BTreeMap<String, u32>,
    /// Times the player has prestiged. +5% global xp_mult per stack.
    pub prestige_count: u32,
    /// Landmark capes the player has already unlocked. Read on tick to
    /// decide which `landmarks::landmarks()` entries still need firing.
    pub landmarks_unlocked: Vec<String>,
    /// Per-species shiny catch count, parallel to `caught`. Length is
    /// auto-extended on load.
    pub shiny_per_species: Vec<u32>,
    /// Tree anchor coords for the in-progress chopping minigame. Consumed
    /// (and cleared) on chop completion to mark exactly one tree as cut.
    pub pending_chop_anchor: Option<(i32, i32)>,
    /// Per-recipe "discovered" flag (parallel to `recipes::recipes()`).
    /// Discovery happens automatically the moment the player catches any
    /// fish whose name appears in the recipe's ingredient list. Discovered
    /// recipes show up in the cookbook; undiscovered ones stay hidden.
    pub recipe_discovered: Vec<bool>,
    /// Highest fishdex milestone index already paid out (so the same
    /// reward isn't re-granted across saves). Indexes into FISHDEX_MILES.
    pub fishdex_milestones_granted: u32,
    /// Highest cookbook milestone index already paid out — indexes into
    /// COOKBOOK_MILES.
    pub cookbook_milestones_granted: u32,
    /// Total mastery milestones earned across all species (sum, not per-fish).
    /// Used to track skill-point granting so we never double-count.
    pub mastery_milestones: u32,
    /// Lifetime achievement progress (unlocked ids + total points granted).
    pub achievements: crate::achievements::AchievementProgress,
    /// Per-dimension first-visit flags. Persisted in save.
    pub visited_mines: bool,
    pub visited_atlantis: bool,
    pub visited_inferno: bool,
    /// When set, all future casts ignore biome/water and pull from this
    /// named loot pool ("cosmic", "infernal", "angelic", "mineral", ...).
    /// Only settable by The Rod's k-menu and cleared by selecting Default.
    pub current_pool_override: Option<String>,
    /// background autosave channel - thread coalesces and writes.
    autosave_tx: mpsc::Sender<SaveData>,
    last_autosave_at: Instant,
    last_autosave_hash: u64,
    pub cast: Option<CastState>,
    held_dir: Option<(i32, i32)>,
    held_until_tick: u64,
    last_step_tick: u64,
    /// Vein cooldown / charge tracking. Keyed by (dim, x, y).
    pub veins: crate::mining::VeinMap,
    /// Bait consumed at cast time; applied (and cleared) on the next catch.
    /// Tuple is (effect_id, magnitude).
    pub bait_pending: Option<(String, f32)>,
    /// Fraction subtracted from this cast's wait time, derived from the
    /// bait's `bite_speed` axis at cast time. Cleared on cast end.
    pub bait_pending_bite_speed: f32,
    /// Pool the active bait pulls toward, with its weight multiplier. Read
    /// during fish selection; cleared on cast end.
    pub bait_pending_pool_pull: Option<(String, f32)>,
    /// Wandering faceless figures in the Mines. Empty when not in Mines or
    /// not yet spawned. Movement ticked in `tick`. NOT persisted.
    pub faceless: Vec<(i32, i32)>,
    /// Unix-secs timestamp at which the Mining XP boost (from a blessing)
    /// expires. 0 = no boost.
    pub mining_boost_until: u64,
    /// When set, the game will save+quit at this anim_tick. Used by the
    /// faceless-curse event to flush a few seconds of cursed log before
    /// kicking the player out.
    pub pending_quit_at: Option<u64>,
    /// Daily quest state. `day_id` is the UTC date when progress started;
    /// if today's date differs, progress resets and `completed` is cleared.
    pub daily_day_id: String,
    pub daily_progress: u32,
    pub daily_completed: bool,
    /// Bonus skill points granted by daily-quest completions across all
    /// days. Feeds skill_tree.available().
    pub daily_bonus_points: u32,
    /// Per-fish mastery challenge progress (challenge id -> current count).
    pub challenge_progress: std::collections::BTreeMap<String, u32>,
    /// Completed challenge ids — guarantees one-time reward.
    pub challenge_done: Vec<String>,
    /// Total skill points awarded by completed challenges (sum). Feeds
    /// `skill_tree.available`.
    pub challenge_bonus_points: u32,
    /// Last species caught (for streak detection). None at fresh start
    /// or after a non-matching catch.
    pub streak_species: Option<String>,
    pub streak_count: u32,
    /// Inverted stamina: walking/mining/interacting drain it, fishing
    /// restores it. Floor 0 (player must fish to act again).
    pub stamina: f32,
    /// Persisted user prefs (autosave interval, log lines, contrast).
    pub settings: Settings,
    /// Cursor for the settings menu.
    pub settings_cursor: usize,
    /// Currently active rolled bounty (None = no quest accepted). Player
    /// can hold at most one bounty at a time; complete or abandon to roll
    /// a fresh one.
    pub bounty: Option<crate::procedural_quests::ProceduralQuest>,
    /// Tutorial progress index. Each step fires a one-time hint then
    /// advances. Stops contributing chatter past TUTORIAL_STEPS.
    pub tutorial_step: u32,
    /// In-game month-id at which the cape last paid out. Updated by
    /// `tick_cape_payout` once per month rollover. Persisted.
    pub last_cape_payout_month: u64,
    /// Daily merchant counters. Reset when `last_market_day` changes.
    pub fish_sold_today: u32,
    pub ore_sold_today: u32,
    pub last_market_day: u64,
    /// Random countdown of steps before the next stamina drain event.
    /// Rerolled to 5..=20 every time it fires. NOT persisted (ephemeral
    /// per session is fine — at worst you get a slightly easy first walk).
    steps_until_drain: u32,
}

pub const TUTORIAL_STEPS: u32 = 10;

/// Baseline maximum stamina before Iron Lungs ranks.
pub const STAMINA_BASE_MAX: f32 = 100.0;

/// FNV-1a hash of the player's name. Used to seed the world so each name
/// produces a unique-but-deterministic map. Two players who pick the
/// same name will see the same world — that's the design intent (the
/// name *is* the seed; pick a different name for a different world).
pub fn seed_from_name(name: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h == 0 { 1 } else { h }
}

/// User-tweakable preferences. Persisted in the save.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub autosave_interval_secs: u32,
    pub log_lines: u16,
    pub high_contrast: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { autosave_interval_secs: 5, log_lines: 10, high_contrast: false }
    }
}

impl App {
    pub fn new() -> Self {
        let mut app = Self::fresh();
        if let Some(data) = save::load_from_disk() {
            app.apply_save(&data);
            // If a dim's generator changed since the save was written
            // (e.g. labyrinth refactor moved walls around), the player's
            // saved coords might now be a wall. Sweep them onto the
            // nearest walkable cell before handing control back.
            app.snap_player_to_walkable();
            let who = if app.player.name.is_empty() {
                "angler".to_string()
            } else {
                app.player.name.clone()
            };
            app.narrator.say(format!("Welcome back, {who}."));
        } else {
            app.scene = Scene::NamePrompt(String::new());
            app.narrator.say("No save found - pick a name to begin.");
            app.narrator
                .say("Esc -> Normal mode. : for commands (:w :wq :q :q! :s :m :e :h).");
        }
        app
    }

    fn fresh() -> Self {
        let mut narrator = Narrator::new();
        narrator.say("You arrive at the village.");
        narrator.say("Yellow D west = rod shop. Pink D east = fishing school. Dock is south.");
        narrator.say("hjkl/wasd/arrows: move    f: interact    g: pick up    x: inspect    e: fishdex    esc: normal");
        Self {
            world: World::new(0xDEAD_BEEF),
            player: Player::spawn(),
            scene: Scene::Overworld,
            mode: Mode::Insert,
            running: true,
            anim_tick: 0,
            rng_state: 0xC0FF_EE42,
            caught: vec![false; fishlist::fish().len()],
            caught_at: vec![None; fishlist::fish().len()],
            caught_context: vec![None; fishlist::fish().len()],
            mastery: vec![0; fishlist::fish().len()],
            cooking_mastery: vec![0; crate::recipes::recipes().len()],
            bugs_caught: vec![0; crate::bugs::defs().len()],
            bugs_picked_today: std::collections::HashSet::new(),
            bugs_picked_day_id: 0,
            soil_dug_today: std::collections::HashSet::new(),
            foraged_today: std::collections::HashSet::new(),
            scales: 0,
            scales_spent: std::collections::BTreeMap::new(),
            prestige_count: 0,
            landmarks_unlocked: Vec::new(),
            shiny_per_species: vec![0; fishlist::fish().len()],
            pending_chop_anchor: None,
            recipe_discovered: vec![false; crate::recipes::recipes().len()],
            banners: Vec::new(),
            fishdex_milestones_granted: 0,
            cookbook_milestones_granted: 0,
            mastery_milestones: 0,
            achievements: crate::achievements::AchievementProgress::default(),
            visited_mines: false,
            visited_atlantis: false,
            visited_inferno: false,
            pending_catch_loc: None,
            narrator,
            quest_progress: HashMap::new(),
            quest_done: Vec::new(),
            current_biome: None,
            current_location: None,
            biome_popup_ticks: 0,
            lifetime_valu: 0,
            session_start: std::time::Instant::now(),
            last_step_at: std::time::Instant::now()
                - std::time::Duration::from_secs(1),
            saved_play_secs: 0,
            pinned_quest: None,
            seen_cells: std::collections::HashMap::new(),
            stats: Stats::default(),
            skills: Skills::default(),
            buffs: crate::buffs::Buffs::default(),
            skill_tree: crate::skill_tree::SkillTree::default(),
            current_pool_override: None,
            autosave_tx: spawn_autosaver(),
            last_autosave_at: Instant::now(),
            last_autosave_hash: 0,
            cast: None,
            held_dir: None,
            held_until_tick: 0,
            last_step_tick: 0,
            veins: crate::mining::VeinMap::new(),
            bait_pending: None,
            bait_pending_bite_speed: 0.0,
            bait_pending_pool_pull: None,
            faceless: Vec::new(),
            mining_boost_until: 0,
            pending_quit_at: None,
            daily_day_id: String::new(),
            daily_progress: 0,
            daily_completed: false,
            daily_bonus_points: 0,
            challenge_progress: std::collections::BTreeMap::new(),
            challenge_done: Vec::new(),
            challenge_bonus_points: 0,
            streak_species: None,
            streak_count: 0,
            stamina: STAMINA_BASE_MAX,
            settings: Settings::default(),
            settings_cursor: 0,
            bounty: None,
            tutorial_step: 0,
            last_cape_payout_month: 0,
            fish_sold_today: 0,
            ore_sold_today: 0,
            last_market_day: 0,
            steps_until_drain: 10,
        }
    }

    pub fn total_play_secs(&self) -> u64 {
        self.saved_play_secs + self.session_start.elapsed().as_secs()
    }

    fn quest_progress(&mut self, kind: &str, target: &str) {
        self.tick_quest_progress(kind, target, false);
        self.tick_daily_progress(kind, target);
    }

    fn quest_progress_silent(&mut self, kind: &str, target: &str) {
        self.tick_quest_progress(kind, target, true);
        self.tick_daily_progress(kind, target);
    }

    fn refresh_daily(&mut self) {
        let today = crate::daily::today_id();
        if today != self.daily_day_id {
            self.daily_day_id = today;
            self.daily_progress = 0;
            self.daily_completed = false;
            if let Some(def) = crate::daily::today_def() {
                self.narrator
                    .say(format!("Today's daily: {}", def.title));
            }
        }
    }

    fn tick_daily_progress(&mut self, kind: &str, target: &str) {
        if self.daily_completed {
            return;
        }
        self.refresh_daily();
        let Some(def) = crate::daily::today_def() else { return };
        if def.kind != kind {
            return;
        }
        // "any" target matches anything of the right kind.
        if def.target != "any" && !def.target.eq_ignore_ascii_case(target) {
            return;
        }
        self.daily_progress = self.daily_progress.saturating_add(1);
        if self.daily_progress >= def.count {
            self.daily_completed = true;
            self.player.valu = self.player.valu.saturating_add(def.reward_valu);
            self.lifetime_valu = self.lifetime_valu.saturating_add(def.reward_valu);
            self.stats.valu_earned = self.stats.valu_earned.saturating_add(def.reward_valu);
            self.daily_bonus_points = self.daily_bonus_points.saturating_add(def.reward_points);
            self.narrator.say(format!(
                "*** Daily complete: {} (+{}$V, +{} skill point{}). ***",
                def.title,
                def.reward_valu,
                def.reward_points,
                if def.reward_points == 1 { "" } else { "s" }
            ));
        }
    }

    fn tick_quest_progress(&mut self, kind: &str, target: &str, silent: bool) {
        for q in quest::quests() {
            if self.quest_done.contains(&q.id) {
                continue;
            }
            if q.objective.kind != kind || q.objective.target != target {
                continue;
            }
            let entry = self.quest_progress.entry(q.id.clone()).or_insert(0);
            *entry += 1;
            let cur = *entry;
            if cur >= q.objective.count {
                self.player.valu = self.player.valu.saturating_add(q.reward.valu);
                self.lifetime_valu = self.lifetime_valu.saturating_add(q.reward.valu);
                self.stats.valu_earned = self.stats.valu_earned.saturating_add(q.reward.valu);
                self.stats.quests_completed += 1;
                self.quest_done.push(q.id.clone());
                self.narrator.say(format!(
                    "Task complete: {} (+{}$V)",
                    q.title, q.reward.valu
                ));
            } else if !silent {
                self.narrator.say(format!(
                    "{}: {}/{}",
                    q.title, cur, q.objective.count
                ));
            }
        }
    }

    fn apply_save(&mut self, data: &SaveData) {
        self.player.x = data.player_x;
        self.player.y = data.player_y;
        self.player.name = data.name.clone();
        self.player.valu = data.valu;
        self.player.inventory = data
            .inventory
            .iter()
            .filter_map(|n| fishlist::fish().iter().find(|f| &f.name == n))
            .collect();
        if data.caught.len() == self.caught.len() {
            self.caught = data.caught.clone();
        } else {
            for (i, &c) in data.caught.iter().enumerate() {
                if let Some(slot) = self.caught.get_mut(i) {
                    *slot = c;
                }
            }
        }
        for (i, loc) in data.caught_at.iter().enumerate() {
            if let Some(slot) = self.caught_at.get_mut(i) {
                if slot.is_none() {
                    *slot = loc.clone();
                }
            }
        }
        for (i, ctx) in data.caught_context.iter().enumerate() {
            if let Some(slot) = self.caught_context.get_mut(i) {
                if slot.is_none() {
                    *slot = ctx.clone();
                }
            }
        }
        self.world = World::new(data.world_seed);
        self.world.dim = data.dim;
        kick_off_pregen(data.world_seed);
        if data.rng_state != 0 {
            self.rng_state = data.rng_state;
        }
        self.quest_progress = data.quest_progress.iter().cloned().collect();
        self.quest_done = data.quest_done.clone();
        self.player.items = data.items.clone();
        self.lifetime_valu = data.lifetime_valu_earned;
        self.saved_play_secs = data.play_time_secs;
        self.pinned_quest = data.pinned_quest.clone();
        // Backcompat: old saves wrote a flat seen_cells (surface only). New
        // saves use seen_by_dim with the dimension stored per-cell.
        self.seen_cells.clear();
        let surface_set = self
            .seen_cells
            .entry(crate::world::Dimension::Surface)
            .or_default();
        for &(x, y) in &data.seen_cells {
            surface_set.insert((x, y));
        }
        for &(dim, x, y) in &data.seen_by_dim {
            self.seen_cells.entry(dim).or_default().insert((x, y));
        }
        self.stats = data.stats.clone();
        self.skills = data.skills.clone();
        self.buffs = data.buffs.clone();
        self.skill_tree = data.skill_tree.clone();
        self.player.has_boat = data.has_boat;
        self.player.has_pickaxe = data.has_pickaxe;
        self.player.has_bug_net = data.has_bug_net;
        // Bugs caught: zero-extend to current bug count so JSON appends
        // don't shift old saves' mastery counts onto the wrong species.
        let nb = crate::bugs::defs().len();
        self.bugs_caught = data.bugs_caught.clone();
        self.bugs_caught.resize(nb, 0);
        // Picked-today set survives a reload within the same in-game day.
        // On day rollover we drop it so today's bugs spawn fresh.
        let today = crate::gametime::game_days(data.play_time_secs);
        if data.bugs_picked_day_id == today {
            self.bugs_picked_today = data.bugs_picked.iter().copied().collect();
            self.bugs_picked_day_id = today;
            self.soil_dug_today = data.soil_dug.iter().copied().collect();
            self.foraged_today = data.foraged.iter().copied().collect();
        } else {
            self.bugs_picked_today.clear();
            self.soil_dug_today.clear();
            self.foraged_today.clear();
            self.bugs_picked_day_id = today;
        }
        self.scales = data.scales;
        self.scales_spent = data.scales_spent.clone();
        self.prestige_count = data.prestige_count;
        self.landmarks_unlocked = data.landmarks_unlocked.clone();
        // Shiny counts: zero-extend to current fish count so JSON appends
        // don't shift old saves' counts onto the wrong species.
        let nf = fishlist::fish().len();
        self.shiny_per_species = data.shiny_per_species.clone();
        self.shiny_per_species.resize(nf, 0);
        self.player.has_shiny_charm = data.has_shiny_charm;
        if !data.mastery.is_empty() {
            let n = self.mastery.len();
            for (i, &v) in data.mastery.iter().enumerate().take(n) {
                self.mastery[i] = v;
            }
        }
        self.mastery_milestones = data.mastery_milestones;
        self.achievements = data.achievements.clone();
        self.visited_mines = data.visited_mines;
        self.visited_atlantis = data.visited_atlantis;
        self.visited_inferno = data.visited_inferno;
        self.player.tackle = data.tackle.clone();
        self.player.bait = data.bait.clone();
        self.player.gear = data.gear.clone();
        self.player.ingots = data.ingots.clone();
        self.last_cape_payout_month = data.last_cape_payout_month;
        self.fish_sold_today = data.fish_sold_today;
        self.ore_sold_today = data.ore_sold_today;
        self.last_market_day = data.last_market_day;
        self.player.hull_tier = data.hull_tier;
        self.player.crew_hunger = data.crew_hunger;
        self.player.biofuel = data.biofuel;
        self.player.wood = data.wood;
        // Cooking mastery array: zero-extend or truncate to match the
        // current recipe count so JSON additions don't break old saves.
        let n = crate::recipes::recipes().len();
        self.cooking_mastery = data.cooking_mastery.clone();
        self.cooking_mastery.resize(n, 0);
        // Recipe discovery is derived from caught[] — every recipe whose
        // ingredient list mentions any fish the player has already caught
        // is auto-discovered on load. Cheap and avoids a new save field.
        self.recipe_discovered = vec![false; n];
        let names: Vec<String> = fishlist::fish()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.caught.get(*i).copied().unwrap_or(false))
            .map(|(_, f)| f.name.to_ascii_lowercase())
            .collect();
        for (ri, r) in crate::recipes::recipes().iter().enumerate() {
            if r
                .ingredients
                .iter()
                .any(|(name, _)| names.iter().any(|n| n == &name.to_ascii_lowercase()))
            {
                self.recipe_discovered[ri] = true;
            }
        }
        self.fishdex_milestones_granted = data.fishdex_milestones_granted;
        self.cookbook_milestones_granted = data.cookbook_milestones_granted;
        self.world.chopped.clear();
        for &(x, y, t) in &data.chopped_trees {
            self.world.chopped.insert((x, y), t);
        }
        self.world.prune_chopped();
        // Legacy save with `has_boat=true` but no hull tier? Treat it as
        // tier 1 with a fresh tank so the player keeps the boat they paid for.
        if self.player.has_boat && self.player.hull_tier == 0 {
            self.player.hull_tier = 1;
            self.player.biofuel = self.player.biofuel.max(50);
        }
        self.daily_day_id = data.daily_day_id.clone();
        self.daily_progress = data.daily_progress;
        self.daily_completed = data.daily_completed;
        self.daily_bonus_points = data.daily_bonus_points;
        self.challenge_progress = data.challenge_progress.clone();
        self.challenge_done = data.challenge_done.clone();
        self.challenge_bonus_points = data.challenge_bonus_points;
        self.streak_species = data.streak_species.clone();
        self.streak_count = data.streak_count;
        self.mining_boost_until = data.mining_boost_until;
        self.stamina = data.stamina.clamp(0.0, self.stamina_max());
        self.settings = data.settings.clone();
        self.bounty = data.bounty.clone();
        self.tutorial_step = data.tutorial_step;
        self.veins = data
            .veins
            .iter()
            .map(|&(dim, x, y, charges, ready)| {
                ((dim, x, y), crate::mining::VeinState {
                    charges_used: charges,
                    ready_at_secs: ready,
                })
            })
            .collect();
        self.player.rods = if data.rods.max_owned == 0 {
            crate::rod::OwnedRods { max_owned: 1, equipped: 1 }
        } else {
            data.rods
        };
    }

    fn current_save(&self) -> SaveData {
        SaveData {
            name: self.player.name.clone(),
            player_x: self.player.x,
            player_y: self.player.y,
            valu: self.player.valu,
            inventory: self
                .player
                .inventory
                .iter()
                .map(|f| f.name.to_string())
                .collect(),
            caught: self.caught.clone(),
            world_seed: self.world.seed,
            rng_state: self.rng_state,
            play_time_secs: self.total_play_secs(),
            lifetime_valu_earned: self.lifetime_valu,
            quest_progress: self
                .quest_progress
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            quest_done: self.quest_done.clone(),
            items: self.player.items.clone(),
            pinned_quest: self.pinned_quest.clone(),
            // legacy field — surface tiles only, written for old loaders
            seen_cells: self
                .seen_cells
                .get(&crate::world::Dimension::Surface)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default(),
            seen_by_dim: self
                .seen_cells
                .iter()
                .flat_map(|(d, set)| set.iter().map(move |(x, y)| (*d, *x, *y)))
                .collect(),
            stats: self.stats.clone(),
            skills: self.skills.clone(),
            rods: self.player.rods,
            caught_at: self.caught_at.clone(),
            caught_context: self.caught_context.clone(),
            buffs: self.buffs.clone(),
            skill_tree: self.skill_tree.clone(),
            has_boat: self.player.has_boat,
            has_pickaxe: self.player.has_pickaxe,
            dim: self.world.dim,
            mastery: self.mastery.clone(),
            mastery_milestones: self.mastery_milestones,
            achievements: self.achievements.clone(),
            visited_mines: self.visited_mines,
            visited_atlantis: self.visited_atlantis,
            visited_inferno: self.visited_inferno,
            tackle: self.player.tackle.clone(),
            bait: self.player.bait.clone(),
            daily_day_id: self.daily_day_id.clone(),
            daily_progress: self.daily_progress,
            daily_completed: self.daily_completed,
            daily_bonus_points: self.daily_bonus_points,
            challenge_progress: self.challenge_progress.clone(),
            challenge_done: self.challenge_done.clone(),
            challenge_bonus_points: self.challenge_bonus_points,
            streak_species: self.streak_species.clone(),
            streak_count: self.streak_count,
            mining_boost_until: self.mining_boost_until,
            veins: self
                .veins
                .iter()
                .map(|(&(dim, x, y), v)| (dim, x, y, v.charges_used, v.ready_at_secs))
                .collect(),
            stamina: self.stamina,
            settings: self.settings.clone(),
            bounty: self.bounty.clone(),
            tutorial_step: self.tutorial_step,
            gear: self.player.gear.clone(),
            ingots: self.player.ingots.clone(),
            last_cape_payout_month: self.last_cape_payout_month,
            fish_sold_today: self.fish_sold_today,
            ore_sold_today: self.ore_sold_today,
            last_market_day: self.last_market_day,
            hull_tier: self.player.hull_tier,
            crew_hunger: self.player.crew_hunger,
            biofuel: self.player.biofuel,
            wood: self.player.wood,
            cooking_mastery: self.cooking_mastery.clone(),
            fishdex_milestones_granted: self.fishdex_milestones_granted,
            cookbook_milestones_granted: self.cookbook_milestones_granted,
            chopped_trees: self
                .world
                .chopped
                .iter()
                .map(|(&(x, y), &t)| (x, y, t))
                .collect(),
            bugs_caught: self.bugs_caught.clone(),
            has_bug_net: self.player.has_bug_net,
            bugs_picked: self.bugs_picked_today.iter().copied().collect(),
            bugs_picked_day_id: self.bugs_picked_day_id,
            soil_dug: self.soil_dug_today.iter().copied().collect(),
            foraged: self.foraged_today.iter().copied().collect(),
            scales: self.scales,
            scales_spent: self.scales_spent.clone(),
            prestige_count: self.prestige_count,
            landmarks_unlocked: self.landmarks_unlocked.clone(),
            shiny_per_species: self.shiny_per_species.clone(),
            has_shiny_charm: self.player.has_shiny_charm,
        }
    }

    /// Consume one fish of the given species from inventory, restore some
    /// stamina (difficulty * 5), and grant a small permanent buff scaled
    /// to the species's difficulty. Unique fish (Fish, Five Elders) can't
    /// be cooked.
    /// Sum the landmark-reward bonus for an axis across every unlocked cape.
    pub fn landmark_bonus(&self, axis: &str) -> f32 {
        let mut acc = 0.0f32;
        for id in &self.landmarks_unlocked {
            if let Some(l) = crate::landmarks::def_by_id(id) {
                acc += match axis {
                    "xp_mult" => l.reward.xp_mult,
                    "valu_mult" => l.reward.valu_mult,
                    "rare_chance" => l.reward.rare_chance,
                    _ => 0.0,
                };
            }
        }
        acc
    }

    /// Per-tick landmark check. Fires any unmet landmark whose criteria now
    /// hold, narrates the unlock, and records the id so it stays unlocked.
    fn check_landmarks(&mut self) {
        let total = fishlist::fish()
            .iter()
            .filter(|f| !f.unique && !f.joke)
            .count()
            .max(1);
        let caught = fishlist::fish()
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.unique && !f.joke)
            .filter(|(i, _)| self.caught.get(*i).copied().unwrap_or(false))
            .count();
        let pct = ((caught as f32) / (total as f32) * 100.0) as u8;
        let mut visited: Vec<&'static str> = Vec::new();
        if self.visited_atlantis { visited.push("Atlantis"); }
        if self.visited_inferno { visited.push("Inferno"); }
        if self.visited_mines { visited.push("Mines"); }
        // Surface dims with sentinel-string match by current/visited dim.
        // For "All Blue" we treat the lifetime visit as "have we ever been".
        // No dedicated flag yet, so derive from current dim:
        if matches!(self.world.dim, crate::world::Dimension::AllBlue) {
            visited.push("All Blue");
        }
        let snap = crate::landmarks::Snapshot {
            catches: self.stats.fish_caught,
            fishdex_pct: pct,
            rod_tier: self.player.rods.max_owned,
            bugs_caught: self.bugs_caught.iter().map(|&c| c as u64).sum(),
            play_hours: self.total_play_secs() / 3600,
            visited_dim_labels: visited,
        };
        for l in crate::landmarks::landmarks() {
            if self.landmarks_unlocked.iter().any(|id| id == &l.id) {
                continue;
            }
            if crate::landmarks::criteria_met(&l.criteria, &snap) {
                self.landmarks_unlocked.push(l.id.clone());
                self.narrator.say(format!(
                    "*** Landmark: {} unlocked. (+{:.0}% xp, +{:.0}% valu, +{:.0}% rare) ***",
                    l.name,
                    l.reward.xp_mult * 100.0,
                    l.reward.valu_mult * 100.0,
                    l.reward.rare_chance * 100.0,
                ));
            }
        }
    }

    /// Prestige: requires ≥95% fishdex completion across non-unique non-joke
    /// species. On commit, resets skill-tree allocations and bumps the
    /// prestige counter (each stack grants +5% global xp_mult, applied in
    /// `prestige_xp_mult`).
    fn do_prestige(&mut self) {
        let total = fishlist::fish()
            .iter()
            .filter(|f| !f.unique && !f.joke)
            .count() as u32;
        let caught: u32 = fishlist::fish()
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.unique && !f.joke)
            .filter(|(i, _)| self.caught.get(*i).copied().unwrap_or(false))
            .count() as u32;
        if total == 0 {
            self.narrator.say("No fish defined.".to_string());
            return;
        }
        let pct = (caught as f32) / (total as f32);
        if pct < 0.95 {
            self.narrator.say(format!(
                "Prestige requires 95% fishdex: {caught}/{total} ({:.0}%).",
                pct * 100.0
            ));
            return;
        }
        self.skill_tree = crate::skill_tree::SkillTree::default();
        self.prestige_count = self.prestige_count.saturating_add(1);
        self.narrator.say(format!(
            "*** PRESTIGE {}. Skill tree reset; +5% global xp permanent. ***",
            self.prestige_count
        ));
    }

    /// Permanent global xp mult from prestige stacks. +5% per stack.
    pub fn prestige_xp_mult(&self) -> f32 {
        1.0 + (self.prestige_count as f32) * 0.05
    }

    /// Craft a lure at the bait bench. Consumes inputs + valu, writes the
    /// output bait id to the player's stock. Refuses gracefully if any
    /// input is short or the player can't afford the valu cost.
    fn craft_lure(&mut self, idx: usize) {
        let recipes = crate::lure_recipes::recipes();
        let Some(r) = recipes.get(idx) else { return };
        if self.player.valu < r.valu_cost {
            self.narrator.say(format!(
                "Need {}$V for {} — have {}$V.",
                r.valu_cost, r.name, self.player.valu
            ));
            return;
        }
        for inp in &r.inputs {
            let have = self.player.bait.count(&inp.bait_id);
            if have < inp.count {
                let label = crate::bait::def_by_id(&inp.bait_id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| inp.bait_id.clone());
                self.narrator.say(format!(
                    "Need {} {} (have {}).",
                    inp.count, label, have
                ));
                return;
            }
        }
        // Commit.
        for inp in &r.inputs {
            let entry = self.player.bait.stock.entry(inp.bait_id.clone()).or_insert(0);
            *entry = entry.saturating_sub(inp.count);
        }
        self.player.valu = self.player.valu.saturating_sub(r.valu_cost);
        self.player.bait.add(&r.output_bait_id, r.output_count);
        let out_label = crate::bait::def_by_id(&r.output_bait_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| r.output_bait_id.clone());
        self.narrator
            .say(format!("Crafted {}x {}.", r.output_count, out_label));
    }

    /// Permanent additive bonus for a scales-spendable axis. 0.0005 per
    /// token spent on the matching axis, capped at 1000 tokens per axis
    /// (i.e. max +50% per axis).
    pub fn scales_bonus(&self, axis: &str) -> f32 {
        let spent = self.scales_spent.get(axis).copied().unwrap_or(0).min(1000);
        (spent as f32) * 0.0005
    }

    /// Axes the player can spend scales on.
    pub const SCALES_AXES: &'static [&'static str] = &[
        "rare_chance",
        "catch_speed",
        "valu_mult",
        "xp_mult",
        "bite_speed",
    ];

    /// Total mastery (catches) across all non-unique, non-joke fish of the
    /// given difficulty band. Used by the rod-mastery gate.
    fn mastery_count_at_difficulty(&self, diff: u8) -> u32 {
        fishlist::fish()
            .iter()
            .enumerate()
            .filter(|(_, f)| f.difficulty == diff && !f.unique && !f.joke)
            .map(|(i, _)| self.mastery.get(i).copied().unwrap_or(0))
            .sum()
    }

    /// True when the player has cleared the mastery gate for a rod tier.
    fn mastery_gate_met(&self, gate: (u8, u32)) -> bool {
        let (diff, count) = gate;
        self.mastery_count_at_difficulty(diff) >= count
    }

    /// Process up to `count` fish of `name` into bait chunks. Each fish
    /// yields `1 + difficulty/3` chunks, tagged `fish:<slug>` so the bait
    /// shop and consume path see it. Unique / joke fish are refused.
    fn do_process_fish(&mut self, arg: &str) {
        // Parse "<count> <name>" or just "<name>" (default count=1).
        let arg = arg.trim();
        if arg.is_empty() {
            self.narrator
                .say("usage: :process <fish-name> [count].".to_string());
            return;
        }
        let (count, name) = if let Some((head, rest)) = arg.split_once(' ') {
            if let Ok(n) = head.parse::<u32>() {
                (n.max(1), rest.trim())
            } else {
                (1u32, arg)
            }
        } else {
            (1u32, arg)
        };
        // Find the first matching fish by case-insensitive name.
        let mut processed = 0u32;
        let mut yielded = 0u32;
        let mut last_slug = String::new();
        let mut last_diff = 0u8;
        while processed < count {
            let idx = self
                .player
                .inventory
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case(name));
            let Some(i) = idx else { break };
            let f = self.player.inventory[i];
            if f.unique || f.joke {
                self.narrator
                    .say(format!("{} can't be processed.", f.name));
                return;
            }
            let yield_ = 1 + (f.difficulty as u32) / 3;
            let slug = crate::bait::fish_slug(&f.name);
            let bait_id = format!("fish:{slug}");
            self.player.inventory.remove(i);
            self.player.bait.add(&bait_id, yield_);
            processed += 1;
            yielded += yield_;
            last_slug = slug;
            last_diff = f.difficulty;
        }
        if processed == 0 {
            self.narrator
                .say(format!("No {name} in the basket to process."));
            return;
        }
        let _ = last_slug;
        let _ = last_diff;
        self.narrator.say(format!(
            "Processed {processed}x {name} -> +{yielded} bait chunk(s).",
        ));
    }

    /// Feed `n` fish from the basket to the crew. Each fish drops crew
    /// hunger by 3 (saturating at 0). Pulls from the front of the basket
    /// so the player can chain feedings without picking a species. Unique
    /// fish are skipped — sacrificing the deity is a war crime.
    fn do_feed_crew(&mut self, n: u32) {
        if self.player.hull_tier == 0 {
            self.narrator.say("No boat. The Shipwright builds the first hull.");
            return;
        }
        if self.player.crew_hunger == 0 {
            self.narrator.say("The crew isn't hungry.");
            return;
        }
        let mut fed = 0u32;
        while fed < n && self.player.crew_hunger > 0 {
            let idx = self.player.inventory.iter().position(|f| !f.unique);
            let Some(i) = idx else {
                break;
            };
            let f = self.player.inventory.remove(i);
            self.player.crew_hunger = self.player.crew_hunger.saturating_sub(3);
            fed += 1;
            self.narrator
                .say(format!("The crew rips into the {}. (-3 hunger)", f.name));
        }
        if fed == 0 {
            self.narrator
                .say("No non-unique fish in the basket to feed them.");
        } else {
            self.narrator.say(format!(
                "Fed {fed} fish to the crew. Hunger: {}/100.",
                self.player.crew_hunger
            ));
        }
    }

    /// Burn `n` fish to fill the biofuel tank. Each fish contributes
    /// `5 * difficulty` units (bigger catch = more oil). Cap at 200.
    fn do_burn_biofuel(&mut self, n: u32) {
        if self.player.hull_tier == 0 {
            self.narrator.say("No boat. The Shipwright builds the first hull.");
            return;
        }
        let mut burned = 0u32;
        let mut gained = 0u32;
        while burned < n {
            let idx = self.player.inventory.iter().position(|f| !f.unique);
            let Some(i) = idx else { break };
            let f = self.player.inventory.remove(i);
            let units = (5u32).saturating_mul(f.difficulty as u32).max(5);
            self.player.biofuel = (self.player.biofuel.saturating_add(units)).min(200);
            self.narrator.say(format!(
                "Rendered the {} into oil. +{units} biofuel.",
                f.name
            ));
            burned += 1;
            gained += units;
        }
        if burned == 0 {
            self.narrator
                .say("No non-unique fish in the basket to burn.");
        } else {
            self.narrator.say(format!(
                "Burned {burned} fish for {gained} biofuel. Tank: {}/200.",
                self.player.biofuel
            ));
        }
    }

    /// Begin the chopping minigame on the tree the player is facing.
    /// The actual yield + xp grant happens inside the Chopping scene's
    /// completion path; the world's tree anchor is recorded here so the
    /// completion path can mark it chopped (with a respawn timer).
    fn do_chop(&mut self) {
        let (dx, dy) = self.player.facing;
        let tx = self.player.x + dx;
        let ty = self.player.y + dy;
        let t = self.world.get(tx, ty);
        if !matches!(
            t,
            crate::world::Tile::TreeTrunk | crate::world::Tile::TreeCanopy
        ) {
            self.narrator
                .say("Nothing to chop. Face a tree and `:chop` again.");
            return;
        }
        if self.stamina <= 0.0 && !self.skill_tree.stamina_second_wind() {
            self.narrator.say("Too tired to swing. Fish first.");
            return;
        }
        // Resolve which anchor this trunk/canopy belongs to so the chop
        // can mark exactly one tree as cut down. Village oaks are static
        // (no anchor in the proc system), so they fall back to (tx, ty)
        // as a pseudo-anchor — chopping a village oak still removes its
        // canopy via the chopped-map lookup on that cell.
        let anchor =
            crate::world::find_tree_anchor_pub(tx, ty, self.world.seed)
                .unwrap_or((tx, ty));
        self.pending_chop_anchor = Some(anchor);
        let lvl = self.skills.woodcutting_level();
        let c = crate::chop::Chopping::new(lvl, &mut self.rng_state);
        self.scene = Scene::Chopping(c);
        self.mode = Mode::Insert;
        self.narrator.say(
            "Chopping. Type the F/G/H/J sequence. Wrong key → 3s lockout.".to_string(),
        );
        self.tutorial_advance(5);
    }

    fn handle_chop_key(&mut self, code: KeyCode) {
        let tick = self.anim_tick;
        let completed = match (&mut self.scene, code) {
            (_, KeyCode::Esc) => {
                self.scene = Scene::Overworld;
                return;
            }
            (Scene::Chopping(c), KeyCode::Char(ch)) => c.type_char(ch, tick),
            _ => false,
        };
        if completed {
            // Pull yield first; the borrow ends before we mutate xp/stamina.
            let base_yield = match &self.scene {
                Scene::Chopping(c) => c.wood_yield,
                _ => 0,
            };
            // Species multiplier: pine > round > bush; village oaks = 2x.
            let species_mult = self
                .pending_chop_anchor
                .map(|(ax, ay)| {
                    crate::world::tree_yield_mult_at(ax, ay, self.world.seed)
                })
                .unwrap_or(1.0);
            let yield_ =
                ((base_yield as f32 * species_mult).round() as u32).max(1);
            self.spend_stamina(2.0);
            self.player.wood = self.player.wood.saturating_add(yield_);
            let lvl = self.skills.woodcutting_level();
            let xp = 5 + (lvl as u64) / 3;
            self.skills.woodcutting_xp += xp;
            let after = self.skills.woodcutting_level();
            self.show_xp_gain("Woodcutting", xp, self.skills.woodcutting_xp, after);
            self.narrator.say(format!(
                "*thunk* +{yield_} wood. (Stash: {})",
                self.player.wood
            ));
            if after > lvl {
                self.narrator
                    .say(format!("Woodcutting level up! Now level {after}."));
            }
            // Mark the felled tree gone. Respawn in 10 real-time minutes
            // mirrors the vein-cooldown cadence; keeps clearings sparse
            // without making the world feel deforested forever.
            if let Some(anchor) = self.pending_chop_anchor.take() {
                self.world.chop_tree(anchor.0, anchor.1, 10 * 60);
            }
            // Counters for achievement chains (forester / lumberjack-tier).
            self.stats.wood_chopped =
                self.stats.wood_chopped.saturating_add(yield_ as u64);
            self.stats.trees_felled = self.stats.trees_felled.saturating_add(1);
            self.quest_progress_silent("chop", "any");
            self.scene = Scene::Overworld;
        }
    }

    /// Open the shipwright upgrade menu — switches to Scene::Shipwright
    /// with the cursor on the cheapest available hull upgrade.
    fn do_open_shipwright(&mut self) {
        self.scene = Scene::Shipwright { cursor: 0 };
        self.mode = Mode::Insert;
        self.tutorial_advance(7);
    }

    fn handle_shipwright_key(&mut self, code: KeyCode) {
        let Scene::Shipwright { cursor } = &mut self.scene else { return };
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
            KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1).min(5),
            KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let target_from = *cursor as u32;
                // Only allow buying the *next* tier — older tiers are
                // already owned, deeper tiers are out of reach.
                if target_from != self.player.hull_tier {
                    self.narrator.say(format!(
                        "That's not the next tier. You're at hull {}.",
                        self.player.hull_tier
                    ));
                    return;
                }
                let Some((valu, wood)) =
                    crate::player::hull_upgrade_cost(self.player.hull_tier)
                else {
                    self.narrator.say("Hull is already at max tier.");
                    return;
                };
                if self.player.valu < valu {
                    self.narrator.say(format!(
                        "Need {valu} valu. You have {}.",
                        self.player.valu
                    ));
                    return;
                }
                if self.player.wood < wood {
                    self.narrator.say(format!(
                        "Need {wood} wood. You have {}.",
                        self.player.wood
                    ));
                    return;
                }
                self.player.valu -= valu;
                self.player.wood -= wood;
                self.player.hull_tier += 1;
                // First build also grants the legacy `has_boat` flag and
                // tops off the biofuel tank so the player can sail away.
                if !self.player.has_boat {
                    self.player.has_boat = true;
                    self.player.biofuel = self.player.biofuel.max(50);
                }
                let new_tier = self.player.hull_tier;
                self.narrator.say(format!(
                    "*** Shipwright completes the {}. Hull tier {}. ***",
                    crate::player::hull_label(new_tier),
                    new_tier
                ));
            }
            _ => {}
        }
    }

    /// Push a banner onto the active stack. Caller picks the kind, body
    /// line and lifetime. Banners render top-down in insertion order.
    pub fn push_banner(&mut self, kind: BannerKind, line: String, ttl_ticks: u32) {
        self.banners.push(Banner { kind, line, ttl_ticks });
    }

    /// Enqueue a discovery banner: a fish or recipe is being seen for the
    /// first time. Grants Encyclopedia xp scaled by `xp` and stacks the
    /// banner under any earlier popups still on screen.
    fn register_discovery(&mut self, label: String, xp: u64, is_recipe: bool) {
        let before = self.skills.encyclopedia_level();
        self.skills.encyclopedia_xp = self.skills.encyclopedia_xp.saturating_add(xp);
        let after = self.skills.encyclopedia_level();
        let kind = if is_recipe {
            BannerKind::Recipe
        } else {
            BannerKind::Discovery
        };
        self.push_banner(
            kind,
            format!("{label}   +{xp} encyclopedia xp"),
            80,
        );
        if after > before {
            self.narrator
                .say(format!("Encyclopedia level up! Now level {after}."));
        }
    }

    /// Fish-discovery side effects: enqueue the fish banner and
    /// auto-unlock every recipe whose ingredient list mentions this fish.
    /// Each newly-discovered recipe gets its own banner + xp drop.
    fn on_fish_first_discovered(&mut self, fish_idx: usize) {
        let fish = match crate::fishlist::fish().get(fish_idx) {
            Some(f) => f,
            None => return,
        };
        let diff = fish.difficulty.max(1) as u64;
        let xp = 10 + diff * 5;
        self.register_discovery(format!("Fish: {}", fish.name), xp, false);
        let recs = crate::recipes::recipes();
        for (ri, r) in recs.iter().enumerate() {
            let already = self
                .recipe_discovered
                .get(ri)
                .copied()
                .unwrap_or(false);
            if already {
                continue;
            }
            let referenced = r
                .ingredients
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(&fish.name));
            if !referenced {
                continue;
            }
            if let Some(slot) = self.recipe_discovered.get_mut(ri) {
                *slot = true;
            }
            let rxp = 15 + (r.min_cooking_level as u64);
            self.register_discovery(format!("Recipe: {}", r.name), rxp, true);
        }
    }

    /// Decrement each banner's TTL and drop expired ones. Banners with
    /// independent timers means a long burst clears in roughly the time
    /// the longest-lived banner was scheduled for, not serial playback.
    fn tick_banners(&mut self) {
        for b in self.banners.iter_mut() {
            b.ttl_ticks = b.ttl_ticks.saturating_sub(1);
        }
        self.banners.retain(|b| b.ttl_ticks > 0);
    }

    /// Run after every catch + after every cook: pays out fishdex/cookbook
    /// milestone rewards as the running totals cross each threshold.
    fn check_encyclopedia_milestones(&mut self) {
        let unique_caught = self.caught.iter().filter(|c| **c).count() as u32;
        while (self.fishdex_milestones_granted as usize) < FISHDEX_MILES.len() {
            let next = &FISHDEX_MILES[self.fishdex_milestones_granted as usize];
            if unique_caught < next.threshold {
                break;
            }
            self.grant_milestone_reward("Fishdex", next);
            self.fishdex_milestones_granted += 1;
        }
        let mastered_recipes = self
            .cooking_mastery
            .iter()
            .filter(|m| **m >= 5)
            .count() as u32;
        while (self.cookbook_milestones_granted as usize) < COOKBOOK_MILES.len() {
            let next = &COOKBOOK_MILES[self.cookbook_milestones_granted as usize];
            if mastered_recipes < next.threshold {
                break;
            }
            self.grant_milestone_reward("Cookbook", next);
            self.cookbook_milestones_granted += 1;
        }
    }

    fn grant_milestone_reward(&mut self, label: &str, m: &EncyclopediaMilestone) {
        self.player.valu = self.player.valu.saturating_add(m.valu);
        self.lifetime_valu = self.lifetime_valu.saturating_add(m.valu);
        if m.skill_points > 0 {
            self.mastery_milestones = self
                .mastery_milestones
                .saturating_add(m.skill_points);
        }
        if let Some(eff) = m.permanent_effect {
            if let Some((msg, _)) = crate::buffs::apply_effect(&mut self.buffs, eff) {
                self.narrator.say(format!("*** {msg} ***"));
            }
        }
        self.narrator.say(format!(
            "*** {label} milestone: {} ({} unique). +{}$V{}{} ***",
            m.label,
            m.threshold,
            m.valu,
            if m.skill_points > 0 {
                format!(", +{} skill point(s)", m.skill_points)
            } else {
                String::new()
            },
            if m.permanent_effect.is_some() {
                ", permanent buff"
            } else {
                ""
            },
        ));
    }

    fn handle_cooking_key(&mut self, code: KeyCode) {
        let Scene::Cooking { cursor, filter, editing_filter } = &mut self.scene else {
            return;
        };
        if *editing_filter {
            match code {
                KeyCode::Enter => {
                    *editing_filter = false;
                    *cursor = 0;
                }
                KeyCode::Esc => {
                    *editing_filter = false;
                    filter.clear();
                    *cursor = 0;
                }
                KeyCode::Backspace => {
                    filter.pop();
                }
                KeyCode::Char(c) if !c.is_control() => {
                    if filter.chars().count() < 40 {
                        filter.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        let visible = cookbook_visible_indices(filter);
        let n = visible.len();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if filter.is_empty() {
                    self.scene = Scene::Overworld;
                } else {
                    filter.clear();
                    *cursor = 0;
                }
            }
            KeyCode::Char('/') => {
                *editing_filter = true;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    *cursor = (*cursor + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *cursor = cursor.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(&idx) = visible.get(*cursor) {
                    self.cook_recipe_at(idx);
                }
            }
            _ => {}
        }
    }

    /// Cook the recipe at index `idx` in `recipes::recipes()`. Validates
    /// level + ingredients, consumes ingredients, applies stamina + the
    /// buff effect, bumps mastery, grants cooking xp. Mastery multiplies
    /// stamina + effect: +5% per 5 cooks, capped at +50%.
    fn cook_recipe_at(&mut self, idx: usize) {
        let recs = crate::recipes::recipes();
        let Some(r) = recs.get(idx) else { return };
        if !self.recipe_discovered.get(idx).copied().unwrap_or(false) {
            self.narrator
                .say("You haven't discovered that recipe yet. Catch a fish that uses it.");
            return;
        }
        let lvl = self.skills.cooking_level();
        if lvl < r.min_cooking_level {
            self.narrator.say(format!(
                "{} requires cooking level {}. You're at {}.",
                r.name, r.min_cooking_level, lvl
            ));
            return;
        }
        if !crate::recipes::can_cook(r, &self.player.inventory) {
            self.narrator.say(format!(
                "Missing ingredients for {}. Check the basket (:i).",
                r.name
            ));
            return;
        }
        // Consume ingredients — first match wins per (name, qty) entry.
        for (name, qty) in &r.ingredients {
            let mut left = *qty;
            while left > 0 {
                if let Some(pos) = self
                    .player
                    .inventory
                    .iter()
                    .position(|f| f.name.eq_ignore_ascii_case(name))
                {
                    self.player.inventory.remove(pos);
                    left -= 1;
                } else {
                    break;
                }
            }
        }
        // Apply scaling. Two stacking sources, both additive:
        //   * recipe mastery: +5% per 5 cooks of this dish, cap +50%.
        //   * cooking level:  +0.5% per level, cap +30%.
        // Combined ceiling sits at +80% on stamina + effect magnitude so
        // a fully-mastered chef gets a real edge without trivialising it.
        let m = self.cooking_mastery.get(idx).copied().unwrap_or(0) as f32;
        let mastery_bonus = ((m / 5.0).floor() * 0.05).min(0.50);
        let cooking_bonus = (lvl as f32 * 0.005).min(0.30);
        let scale = 1.0 + mastery_bonus + cooking_bonus;
        let stamina_grant = (r.stamina as f32 * scale).round();
        if stamina_grant > 0.0 {
            self.grant_stamina(stamina_grant);
        }
        if let Some(eff) = &r.effect {
            // Parse and scale the magnitude before applying. Reuses the
            // buff syntax from fish effects so a scaled "price_mult:0.005"
            // becomes "price_mult:0.0075" at +50% mastery, etc.
            if let Some((k, v)) = eff.split_once(':') {
                if let Ok(mag) = v.trim().parse::<f32>() {
                    let scaled = format!("{}:{}", k.trim(), mag * scale);
                    if let Some((msg, _)) =
                        crate::buffs::apply_effect(&mut self.buffs, &scaled)
                    {
                        self.narrator.say(format!("*** {msg} ***"));
                    }
                }
            } else if let Some((msg, _)) =
                crate::buffs::apply_effect(&mut self.buffs, eff)
            {
                self.narrator.say(format!("*** {msg} ***"));
            }
        }
        // Mastery + cooking xp. Cooking xp scales with the recipe's
        // min_cooking_level so high-tier dishes give more.
        let first_cook = self.cooking_mastery.get(idx).copied().unwrap_or(0) == 0;
        if let Some(slot) = self.cooking_mastery.get_mut(idx) {
            *slot = slot.saturating_add(1);
            let after_m = *slot;
            const NARRATE_AT: &[u32] = &[1, 5, 10, 25, 50, 100, 250];
            if NARRATE_AT.contains(&after_m) {
                self.narrator.say(format!(
                    "*** Recipe mastery {after_m} on {}! ***",
                    r.name
                ));
            }
        }
        if first_cook {
            // First time the player actually plates this dish — separate
            // event from "discovered" (which fires on the ingredient
            // fish-catch). Encyclopedia xp + the recipe banner so the
            // player gets a clear "you cooked something new" moment.
            let dish_xp = 30 + r.min_cooking_level as u64 * 2;
            self.register_discovery(
                format!("First cook: {}", r.name),
                dish_xp,
                true,
            );
        }
        let rname = r.name.clone();
        self.quest_progress_silent("cook", "any");
        self.quest_progress_silent("cook", &rname);
        let xp = (20 + r.min_cooking_level as u64 * 4)
            .saturating_mul((scale * 100.0) as u64)
            / 100;
        let before = self.skills.cooking_level();
        self.skills.cooking_xp += xp.max(10);
        let after = self.skills.cooking_level();
        self.show_xp_gain("Cooking", xp.max(10), self.skills.cooking_xp, after);
        self.narrator.say(format!(
            "Cooked {}. +{} stamina.{}",
            r.name,
            stamina_grant as i32,
            if scale > 1.0 {
                format!(" (mastery x{:.2})", scale)
            } else {
                String::new()
            }
        ));
        if after > before {
            self.narrator
                .say(format!("Cooking level up! Now level {after}."));
        }
        self.check_encyclopedia_milestones();
    }

    fn do_cook(&mut self, name: &str) {
        if !self.is_near_cooking_pot() {
            self.narrator.say(
                "You need to be at a cooking pot. Find the Chef's pot in the village.",
            );
            return;
        }
        let key = name.to_ascii_lowercase();
        let idx = self
            .player
            .inventory
            .iter()
            .position(|f| f.name.to_ascii_lowercase() == key && !f.unique);
        let Some(idx) = idx else {
            self.narrator.say(format!("No {name} in your basket."));
            return;
        };
        let f = self.player.inventory.remove(idx);
        let diff = f.difficulty as f32;
        self.grant_stamina(diff * 5.0);
        // Tiny permanent buff: 0.5% per difficulty point in sell price.
        let bonus = 0.005 * diff;
        self.buffs.price_mult_bonus += bonus;
        self.narrator.say(format!(
            "You cook the {}. +{:.0} stamina, +{:.1}% lifetime sell price.",
            f.name,
            diff * 5.0,
            bonus * 100.0,
        ));
    }

    /// Advance the tutorial to `target_step` if not already past it, and
    /// emit the matching one-time hint line.
    fn tutorial_advance(&mut self, target_step: u32) {
        if self.tutorial_step > target_step || target_step >= TUTORIAL_STEPS {
            return;
        }
        self.tutorial_step = target_step + 1;
        let hint = match target_step {
            0 => "Tutorial: move with hjkl, wasd, or arrows. Try walking south to the pier.",
            1 => "Tutorial: face water and press f to cast. Yank when the line tugs.",
            2 => "Tutorial: keep the fish inside the rectangle. j/k, w/s, or arrows move you.",
            3 => "Tutorial: press i to view your basket. Find a fishmonger to sell.",
            4 => "Tutorial: try :e for the fishdex, :s for stats, :m for the map.",
            5 => "Tutorial: face a tree and type :chop. The Lumberjack (west of the well) explains.",
            6 => "Tutorial: try :cook or :cookbook. Recipes unlock as you catch their ingredients.",
            7 => "Tutorial: when you have a boat, talk to the Shipwright (south pier) to upgrade hulls.",
            8 => "Tutorial: aboard the boat, :feed and :burn convert fish to crew/fuel.",
            9 => "Tutorial complete. Press :help for the full command list.",
            _ => "",
        };
        if !hint.is_empty() {
            self.narrator.say(hint.to_string());
        }
    }

    fn tick_bounty(&mut self, fish_name: &str) {
        let done = if let Some(b) = self.bounty.as_mut() {
            if b.fish_name == fish_name {
                b.progress = b.progress.saturating_add(1);
                b.progress >= b.count
            } else {
                false
            }
        } else {
            false
        };
        if done {
            if let Some(b) = self.bounty.take() {
                self.player.valu = self.player.valu.saturating_add(b.reward_valu);
                self.lifetime_valu = self.lifetime_valu.saturating_add(b.reward_valu);
                self.stats.valu_earned = self.stats.valu_earned.saturating_add(b.reward_valu);
                self.challenge_bonus_points = self
                    .challenge_bonus_points
                    .saturating_add(b.reward_points);
                self.narrator.say(format!(
                    "*** Bounty complete: {}! +{}$V, +{} skill point(s). ***",
                    b.title(),
                    b.reward_valu,
                    b.reward_points
                ));
            }
        }
    }

    fn adjust_setting(&mut self, delta: i32) {
        match self.settings_cursor {
            0 => {
                let v = self.settings.autosave_interval_secs as i32 + delta;
                self.settings.autosave_interval_secs = v.clamp(1, 60) as u32;
            }
            1 => {
                let v = self.settings.log_lines as i32 + delta;
                self.settings.log_lines = v.clamp(5, 15) as u16;
            }
            2 => {
                self.settings.high_contrast = !self.settings.high_contrast;
            }
            _ => {}
        }
    }

    /// If the player's current world cell isn't walkable (e.g. they
    /// portalled into a labyrinth dim where their saved coords now
    /// happen to be wall), sweep outward in expanding rings until a
    /// walkable cell is found and snap them there. Gives up after
    /// radius 80 — past that the dim is broken anyway.
    fn snap_player_to_walkable(&mut self) {
        let (px, py) = (self.player.x, self.player.y);
        if self.world.get(px, py).walkable() {
            return;
        }
        for r in 1..80i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    // only the outermost ring at this radius
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let nx = px + dx;
                    let ny = py + dy;
                    if self.world.get(nx, ny).walkable() {
                        self.player.x = nx;
                        self.player.y = ny;
                        return;
                    }
                }
            }
        }
    }

    pub fn stamina_max(&self) -> f32 {
        STAMINA_BASE_MAX + self.skill_tree.stamina_max_bonus()
    }

    /// Spend `cost` stamina for a non-fishing action. With Second Wind unlocked
    /// and stamina under 10% of max, costs are waived. At 0 stamina the
    /// caller should refuse the action.
    fn spend_stamina(&mut self, cost: f32) {
        if cost <= 0.0 {
            return;
        }
        let max = self.stamina_max();
        if self.skill_tree.stamina_second_wind() && self.stamina < max * 0.10 {
            return;
        }
        // Boots can shrink the per-action drain (diamond boots = ~30% less).
        let cost = cost * self.player.gear.combined_perks().stamina_loss_mult;
        self.stamina = (self.stamina - cost).max(0.0);
        if self.stamina <= 0.1 && self.anim_tick % 40 == 0 {
            self.narrator.say("You are exhausted. Sit down and fish.");
        }
    }

    fn grant_stamina(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        let max = self.stamina_max();
        self.stamina = (self.stamina + amount).min(max);
    }

    /// Stamina drained per random walk event (not per step). Reduced by
    /// the Light Step skill.
    fn walk_event_drain(&self) -> f32 {
        let red = self.skill_tree.stamina_walk_reduce();
        (1.0 * (1.0 - red)).max(0.0)
    }

    fn reroll_steps_until_drain(&mut self) {
        let r = crate::fish::next_rand_f32(&mut self.rng_state);
        // Uniform 15..=40 inclusive.
        self.steps_until_drain = 15 + (r * 26.0) as u32;
    }

    pub fn do_save(&mut self) -> bool {
        let data = self.current_save();
        self.last_autosave_hash = save_hash(&data);
        self.last_autosave_at = Instant::now();
        match save::save_to_disk(&data) {
            Ok(()) => {
                self.narrator.say("Saved.");
                true
            }
            Err(e) => {
                self.narrator.say(format!("Save failed: {e}"));
                false
            }
        }
    }

    fn show_xp_gain(&mut self, skill: &'static str, gained: u64, total_xp: u64, level: u32) {
        self.narrator.say(format!("+{gained} {skill} xp"));
        // 5 seconds at 20fps -> 100 ticks
        self.push_banner(
            BannerKind::Xp { level, total_xp },
            format!("+{gained} {skill} xp"),
            100,
        );
    }

    fn tick_cast(&mut self) {
        let Some(c) = self.cast.as_mut() else { return; };
        match c.phase {
            CastPhase::Casting => {
                c.cast_pos += c.cast_vel;
                if c.cast_pos > 1.0 {
                    c.cast_pos = 1.0;
                    c.cast_vel = -c.cast_vel.abs();
                }
                if c.cast_pos < 0.0 {
                    c.cast_pos = 0.0;
                    c.cast_vel = c.cast_vel.abs();
                }
            }
            CastPhase::Waiting => {
                if c.wait_ticks_left > 0 {
                    c.wait_ticks_left -= 1;
                } else {
                    c.phase = CastPhase::Biting;
                    c.bite_ticks_left = 40;
                }
            }
            CastPhase::Biting => {
                if c.bite_ticks_left > 0 {
                    c.bite_ticks_left -= 1;
                } else {
                    let name = c.fish.name.clone();
                    self.cast = None;
                    self.narrator.say(format!("The {name} slipped off the hook."));
                    self.stats.fish_escaped += 1;
                    if self.stats.catch_streak > 0 {
                        self.narrator.say(format!(
                            "Streak broken at {}.",
                            self.stats.catch_streak
                        ));
                        self.stats.catch_streak = 0;
                    }
                }
            }
        }
    }

    fn cast_action(&mut self) {
        // Pre-compute &self-needing bits before taking a mutable borrow on
        // self.cast. Without this the re-roll path can't reach the weather
        // / play-secs helpers without a borrow conflict.
        let scales_bite_speed = self.scales_bonus("bite_speed");
        let pre_weather = self.current_weather();
        let pre_total_secs = self.total_play_secs();
        let Some(c) = self.cast.as_mut() else { return; };
        match c.phase {
            CastPhase::Casting => {
                c.cast_strength = c.cast_pos.clamp(0.0, 1.0);
                // bobber distance: 1..=3 cells based on strength, plus permanent
                // buff bonus from rare fish like the Long Caster.
                let max_d = (1 + (c.cast_strength * 2.0).round() as i32
                    + self.buffs.bobber_range_bonus)
                    .max(1);
                let (fx, fy) = self.player.facing;
                let mut bd = 1;
                for d in 1..=max_d {
                    let bx = self.player.x + fx * d;
                    let by = self.player.y + fy * d;
                    if matches!(
                        self.world.get(bx, by),
                        Tile::Water | Tile::Dock | Tile::Well
                    ) {
                        bd = d;
                    } else {
                        break;
                    }
                }
                c.bobber = (self.player.x + fx * bd, self.player.y + fy * bd);
                c.hotspot = crate::world::is_hotspot(c.bobber.0, c.bobber.1, self.world.seed);
                // On a hotspot strike, re-roll the previously-picked fish
                // (which may already be junk) under a no-trash gate so the
                // promised "no garbage" guarantee actually holds.
                if c.hotspot && c.fish.joke {
                    let (bx, by) = c.bobber;
                    let biome_label =
                        biome_at(bx, by, self.world.seed).label().to_string();
                    let water = water_kind_at(&self.world, bx, by).to_string();
                    let depth = if water == "ocean" {
                        ocean_depth_at(&self.world, bx, by)
                    } else {
                        0
                    };
                    let rare_window = crate::gametime::time_of_day(pre_total_secs)
                        .is_rare_window()
                        || crate::weather::weather_modifiers(pre_weather).rare_pct
                            > 0.0;
                    let bait_pool_pull = self
                        .bait_pending_pool_pull
                        .as_ref()
                        .map(|(t, m)| (t.as_str(), *m));
                    c.fish = crate::fish::pick_fish_full(
                        &mut self.rng_state,
                        fishlist::fish(),
                        &biome_label,
                        &water,
                        None,
                        rare_window,
                        Some(pre_weather.value()),
                        self.stats.fish_caught,
                        self.skills.fishing_level(),
                        self.player.rods.max_owned,
                        depth,
                        bait_pool_pull,
                        true,
                    );
                }
                // geometric wait length
                let r = crate::fish::next_rand_f32(&mut self.rng_state);
                let k = (1.0f32 - r * 0.9999).ln() / 0.75f32.ln();
                let secs = (k.ceil() as u32).clamp(1, 30) as f32;
                let total_bite_speed = (self.bait_pending_bite_speed
                    + self.player.tackle.sum_effect("bite_speed")
                    + scales_bite_speed)
                    .clamp(0.0, 0.7);
                let bite_mult = (1.0 - total_bite_speed).max(0.3);
                let scaled = secs * (1.0 - c.cast_strength * 0.5)
                    * self.buffs.wait_mult()
                    * bite_mult;
                c.wait_ticks_left = (scaled * 20.0).max(20.0) as u32;
                c.phase = CastPhase::Waiting;
                self.narrator
                    .say(format!("Cast lands {} tiles out. Waiting...", bd));
                if c.hotspot {
                    self.narrator.say("Struck a hotspot!".to_string());
                }
            }
            CastPhase::Biting => {
                let fish = c.fish;
                let (bx, by) = c.bobber;
                let cast_strength = c.cast_strength;
                let hotspot = c.hotspot;
                let biome = biome_at(bx, by, self.world.seed).label().to_string();
                let water = water_kind_at(&self.world, bx, by).to_string();
                self.pending_catch_loc = Some((biome, water));
                self.cast = None;
                self.narrator
                    .say("Something hits the line!".to_string());
                let w_mods = crate::weather::weather_modifiers(self.current_weather());
                let dim_bonus_pct = if matches!(
                    self.world.dim,
                    crate::world::Dimension::AllBlue
                ) { 0.10 } else { 0.0 };
                let bait_catch_speed = match &self.bait_pending {
                    Some((e, m)) if e == "catch_speed" => *m,
                    _ => 0.0,
                };
                let combo_chain_bonus = if self.buffs.combo_chain_left > 0 {
                    self.buffs.combo_chain_mult
                } else {
                    0.0
                };
                let hotspot_bonus = if hotspot { 0.25 } else { 0.0 };
                let extra_speed = w_mods.catch_speed_pct
                    + dim_bonus_pct
                    + bait_catch_speed
                    + combo_chain_bonus
                    + hotspot_bonus;
                let bait_label = self
                    .player
                    .bait
                    .active
                    .as_ref()
                    .and_then(|id| crate::bait::def_by_id(id))
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "none".to_string());
                let tackle_label = format!(
                    "H{} V{} L{} R{}",
                    self.player.tackle.hat,
                    self.player.tackle.vest,
                    self.player.tackle.line,
                    self.player.tackle.lure,
                );
                let mut f = Fishing::new_with_skills(
                    fish,
                    self.rng_state,
                    self.skills.fishing_level(),
                    self.player.rods.equipped,
                    cast_strength,
                    &self.skill_tree,
                    extra_speed,
                );
                f.gear_bait_label = bait_label;
                f.gear_tackle_label = tackle_label;
                self.scene = Scene::Fishing(f);
            }
            CastPhase::Waiting => {
                // No '!' on the bobber yet — pressing space here aborts the
                // cast instead of doing nothing. Saves the player a separate
                // Esc keystroke.
                self.cast = None;
                self.narrator
                    .say("Reeled in early. Whatever was nibbling, you'll never know.");
            }
        }
    }

    /// Per-tick stamina regen. Only the Meditative skill ticks here —
    /// fishing's restorative payoff is the per-catch lump grant, not a
    /// continuous drip.
    fn tick_stamina(&mut self) {
        if self.stamina >= self.stamina_max() {
            return;
        }
        let r = self.skill_tree.stamina_idle_regen();
        if r > 0.0 {
            self.grant_stamina(r);
        }
    }

    fn cancel_cast(&mut self) {
        if self.cast.is_some() {
            self.cast = None;
            self.narrator.say("Reeled in the empty line.");
        }
    }

    fn maybe_autosave(&mut self) {
        // Send a snapshot to the background thread every
        // `settings.autosave_interval_secs` seconds, but only if the save
        // actually changed since the last write.
        let secs = self.settings.autosave_interval_secs.max(1) as u64;
        if self.last_autosave_at.elapsed() < Duration::from_secs(secs) {
            return;
        }
        let snapshot = self.current_save();
        let h = save_hash(&snapshot);
        if h == self.last_autosave_hash {
            self.last_autosave_at = Instant::now();
            return;
        }
        if self.autosave_tx.send(snapshot).is_ok() {
            self.last_autosave_hash = h;
        }
        self.last_autosave_at = Instant::now();
    }

    pub fn tick(&mut self) {
        let _tick_scope = crate::perf::Scope::new("tick.total");
        self.anim_tick = self.anim_tick.wrapping_add(1);
        if self.biome_popup_ticks > 0 {
            self.biome_popup_ticks -= 1;
        }
        self.tick_cast();
        self.tick_stamina();
        self.tick_banners();
        self.maybe_autosave();
        // Pantheon meta-progression checks: cheap, idempotent. Only fires when
        // a threshold is crossed and that god isn't already granted.
        if self.anim_tick % 20 == 0 {
            self.check_pantheon_unlocks();
            self.check_achievements();
            self.refresh_daily();
            self.tick_faceless();
            self.sync_cape();
            self.tick_cape_payout();
            self.tick_market_day_rollover();
            self.tick_bugs_day_rollover();
            self.check_landmarks();
        }
        if let Some(t) = self.pending_quit_at {
            if self.anim_tick >= t {
                self.do_save();
                self.running = false;
            }
        }

        let movement_allowed = matches!(self.mode, Mode::Insert)
            && matches!(
                self.scene,
                Scene::Overworld | Scene::HouseInterior { .. } | Scene::Perf
            )
            && self.cast.is_none();
        if movement_allowed {
            if let Some(dir) = self.held_dir {
                if self.anim_tick > self.held_until_tick {
                    self.held_dir = None;
                } else {
                    let base = if dir.1 == 0 {
                        MOVE_INTERVAL_H
                    } else {
                        MOVE_INTERVAL_V
                    };
                    // Exhausted: each step interval is ~1.43x longer (30%
                    // slower). Doesn't block movement — just drags it.
                    let stam_mult = if self.stamina <= 0.0 { 1.0 / 0.7 } else { 1.0 };
                    let gear_mult = self.player.gear.combined_perks().move_speed_mult.max(0.2);
                    let interval = ((base as f32) * self.buffs.walk_mult() * stam_mult * gear_mult)
                        .round()
                        .max(1.0) as u64;
                    if self.anim_tick.saturating_sub(self.last_step_tick) >= interval {
                        self.step_dispatch(dir.0, dir.1);
                        self.last_step_tick = self.anim_tick;
                    }
                }
            }
        } else {
            self.held_dir = None;
        }

        if let Scene::Fishing(g) = &mut self.scene {
            g.tick();
        }
        if let Scene::BugCatch(b) = &mut self.scene {
            b.tick(self.anim_tick);
            if b.result.is_some() {
                self.resolve_bug_catch();
            }
        }
        if let Scene::Boss(b) = &mut self.scene {
            b.tick();
            if let Some(result) = &b.finished {
                let (msg, fa_name, fb_name) = (
                    match result {
                        crate::boss::BossResult::Won => "*** Boss defeated! ***",
                        crate::boss::BossResult::Lost => "The bosses get away. (Try again with :boss.)",
                    }
                    .to_string(),
                    b.fish_a.name.clone(),
                    b.fish_b.name.clone(),
                );
                let won = matches!(result, crate::boss::BossResult::Won);
                self.scene = Scene::Overworld;
                self.narrator.say(msg);
                if won {
                    // Quick reward: +20% Fishing XP boost + valu equal to sum of
                    // sell prices.
                    let reward = (fishlist::fish()
                        .iter()
                        .find(|f| f.name == fa_name)
                        .map(|f| f.sell_price())
                        .unwrap_or(0)
                        + fishlist::fish()
                            .iter()
                            .find(|f| f.name == fb_name)
                            .map(|f| f.sell_price())
                            .unwrap_or(0)) as u64
                        * 3;
                    self.player.valu = self.player.valu.saturating_add(reward);
                    self.lifetime_valu = self.lifetime_valu.saturating_add(reward);
                    self.stats.valu_earned = self.stats.valu_earned.saturating_add(reward);
                    self.narrator
                        .say(format!("+{reward}$V boss bounty."));
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if matches!(self.scene, Scene::NamePrompt(_)) {
            if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
                return;
            }
            self.handle_name_prompt(key.code);
            return;
        }

        if matches!(self.mode, Mode::Insert) {
            if let Scene::Boss(b) = &mut self.scene {
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    match key.code {
                        KeyCode::Char('f') | KeyCode::Char('F') => { b.input_left_up(); return; }
                        KeyCode::Char('v') | KeyCode::Char('V') => { b.input_left_down(); return; }
                        KeyCode::Char('j') | KeyCode::Char('J') => { b.input_right_up(); return; }
                        KeyCode::Char('n') | KeyCode::Char('N') => { b.input_right_down(); return; }
                        _ => {}
                    }
                }
            }
            if let Scene::Fishing(g) = &mut self.scene {
                match key.code {
                    KeyCode::Char('k') | KeyCode::Char('w') | KeyCode::Up => {
                        g.input_up(key.kind);
                        return;
                    }
                    KeyCode::Char('j') | KeyCode::Char('s') | KeyCode::Down => {
                        g.input_down(key.kind);
                        return;
                    }
                    KeyCode::Char('r') => {
                        // "yank up" — a stronger explicit upward pull.
                        g.input_yank_up(key.kind);
                        return;
                    }
                    KeyCode::Char('t') | KeyCode::Enter => {
                        // "yank down" — strength scales with Rod of
                        // Legends "Heavy Yank" skill (weak → equal to yank-up)
                        let strength = self.skill_tree.yank_down_strength();
                        g.input_yank_down(key.kind, strength);
                        return;
                    }
                    KeyCode::Char('b') if matches!(key.kind, KeyEventKind::Press) => {
                        // Rod of Legends T2: active rectangle boost
                        g.input_legends_boost();
                        return;
                    }
                    KeyCode::Char('n') if matches!(key.kind, KeyEventKind::Press) => {
                        // Tamer T2: active fish slow
                        g.input_tamer_slow();
                        return;
                    }
                    _ => {}
                }
            }
            if matches!(self.scene, Scene::Overworld | Scene::HouseInterior { .. }) {
                if let Some(dir) = direction_for(key.code) {
                    self.handle_movement(dir, key.kind);
                    return;
                }
            }
        }

        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return;
        }

        match &mut self.mode {
            Mode::Insert => self.insert_key(key),
            Mode::Normal => self.normal_key(key.code),
            Mode::Command(_) => self.command_key(key.code),
        }
    }

    fn handle_name_prompt(&mut self, code: KeyCode) {
        let Scene::NamePrompt(buf) = &mut self.scene else {
            return;
        };
        match code {
            KeyCode::Enter => {
                let trimmed = buf.trim().to_string();
                let name = if trimmed.is_empty() {
                    "angler".to_string()
                } else {
                    trimmed
                };
                self.player.name = name.clone();
                // Derive the world seed from the player's name so it's
                // deterministic per save (two saves under the same name
                // generate the same map — intended; the user wants this).
                let seed = seed_from_name(&name);
                self.world = World::new(seed);
                self.rng_state = seed ^ 0xC0FF_EE42;
                self.narrator.say(format!("Welcome, {name}."));
                self.narrator
                    .say("Try :w to save your progress whenever.");
                self.scene = Scene::Overworld;
                kick_off_pregen(seed);
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) if !c.is_control() && buf.chars().count() < 24 => {
                buf.push(c);
            }
            _ => {}
        }
    }

    fn handle_movement(&mut self, dir: (i32, i32), kind: KeyEventKind) {
        match kind {
            KeyEventKind::Press => {
                // step immediately on a fresh press or direction change;
                // os-repeat presses just extend the hold without re-stepping
                let fresh = self.held_dir != Some(dir);
                if fresh {
                    self.step_dispatch(dir.0, dir.1);
                    self.last_step_tick = self.anim_tick;
                }
                self.held_dir = Some(dir);
                self.held_until_tick = self.anim_tick + 10;
            }
            KeyEventKind::Repeat => {
                self.held_dir = Some(dir);
                self.held_until_tick = self.anim_tick + 10;
            }
            KeyEventKind::Release => {
                if self.held_dir == Some(dir) {
                    self.held_dir = None;
                }
            }
        }
    }

    /// Single entry point for "the player wants to step one cell in (dx, dy)".
    /// Routes to the overworld step or to the house-interior step depending
    /// on the current scene. Throttled: keyboard autorepeat + mashing two
    /// axes simultaneously (e.g. Down + Right + S + D) would otherwise let
    /// the player blur across the map at 30+ cells/sec. The cooldown is
    /// axis-aware to match the 2:1 cell aspect (vertical cells are twice
    /// as tall as horizontal cells are wide, so a vertical step covers ~2x
    /// the visual distance and needs ~2x the cooldown to feel even) and is
    /// scaled by boot perks via move_speed_mult.
    fn step_dispatch(&mut self, dx: i32, dy: i32) {
        // Lines and feet stay where you cast them: no walking while a cast
        // is live (waiting for bite or hooked).
        if self.cast.is_some() {
            return;
        }
        const BASE_COOLDOWN_H_MS: f32 = 69.0;
        const BASE_COOLDOWN_V_MS: f32 = 126.0;
        let mult = self.player.gear.combined_perks().move_speed_mult.max(0.2);
        let base = if dy != 0 { BASE_COOLDOWN_V_MS } else { BASE_COOLDOWN_H_MS };
        let cd = std::time::Duration::from_millis((base * mult) as u64);
        if self.last_step_at.elapsed() < cd {
            return;
        }
        match &self.scene {
            Scene::Overworld => self.step(dx, dy),
            Scene::HouseInterior { .. } => self.step_house(dx, dy),
            _ => return,
        }
        self.last_step_at = std::time::Instant::now();
    }

    fn step_house(&mut self, dx: i32, dy: i32) {
        let Scene::HouseInterior { px, py, seed, .. } = &mut self.scene else { return };
        self.player.facing = (dx, dy);
        let nx = *px + dx;
        let ny = *py + dy;
        let f = crate::house::tile_at(nx, ny, *seed);
        if !f.walkable() {
            return;
        }
        // Don't auto-exit on stepping onto the door tile — the player must
        // press f while facing it. Otherwise it's too easy to slide out by
        // accident.
        *px = nx;
        *py = ny;
    }

    fn insert_key(&mut self, key: KeyEvent) {
        let code = key.code;
        if code == KeyCode::Esc {
            match self.scene {
                Scene::Overworld => {
                    self.mode = Mode::Normal;
                    self.narrator.say("-- NORMAL -- (i to play, : for command)");
                }
                _ => self.exit_subscene(),
            }
            return;
        }
        // Shortcut from any insert-mode scene that doesn't consume ':' to
        // command mode. Suppressed in any scene where ':' is either part of
        // text input (Notes, NamePrompt, Fishdex filter, Mining ore name) or
        // an active minigame the player must not lose ticks during
        // (Fishing, Boss, Dialogue).
        let fishdex_filtering = matches!(
            &self.scene, Scene::Fishdex(d) if d.editing_filter
        );
        let cookbook_filtering = matches!(
            &self.scene, Scene::Cooking { editing_filter, .. } if *editing_filter
        );
        let cmd_shortcut_blocked = matches!(
            self.scene,
            Scene::Notes(_) | Scene::NamePrompt(_) | Scene::Dialogue { .. }
            | Scene::Mining(_) | Scene::Chopping(_) | Scene::Fishing(_) | Scene::Boss(_)
            | Scene::BugCatch(_)
        ) || fishdex_filtering || cookbook_filtering;
        if code == KeyCode::Char(':') && !cmd_shortcut_blocked {
            self.mode = Mode::Command(String::new());
            return;
        }
        match &mut self.scene {
            Scene::Overworld => self.handle_overworld(code),
            Scene::Fishdex(d) => {
                if d.editing_filter {
                    match code {
                        KeyCode::Enter => d.apply_filter(&self.caught),
                        KeyCode::Esc => d.clear_filter(),
                        KeyCode::Backspace => d.pop_filter(),
                        KeyCode::Char(c) if !c.is_control() => d.push_filter(c),
                        _ => {}
                    }
                } else {
                    match code {
                        KeyCode::Char('j') | KeyCode::Down => d.cursor_down(&self.caught),
                        KeyCode::Char('k') | KeyCode::Up => d.cursor_up(&self.caught),
                        KeyCode::Char('/') => d.start_filter(),
                        KeyCode::Char('c') => {
                            // Jump from the selected fish into the
                            // cookbook with the filter set to its name —
                            // but only if the player is standing at a
                            // cooking pot.
                            let sel = d.state.selected().unwrap_or(0);
                            let known = self.caught.get(sel).copied().unwrap_or(false);
                            if !known {
                                // skip silently — fish not caught yet
                            } else if !self.is_near_cooking_pot() {
                                self.narrator.say(
                                    "You need to be at a cooking pot. Find the Chef's pot in the village.",
                                );
                            } else {
                                let name = fishlist::fish()[sel].name.clone();
                                self.scene = Scene::Cooking {
                                    cursor: 0,
                                    filter: name,
                                    editing_filter: false,
                                };
                            }
                        }
                        KeyCode::Char('q') | KeyCode::Char('e') => self.exit_subscene(),
                        _ => {}
                    }
                }
            }
            Scene::RodShop { cursor } => {
                let owned = self.player.rods.max_owned;
                let next = owned + 1;
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => self.exit_subscene(),
                    KeyCode::Char('j') | KeyCode::Down => {
                        let last = (crate::rod::rods().len() as u32).saturating_sub(1);
                        *cursor = (*cursor + 1).min(last);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        // buy the next rod if possible
                        if let Some(rod) = crate::rod::get(next) {
                            // tier 201 = The Fishing Rod: requires the Fish + 1M valu.
                            // Fish is NOT consumed; you keep it forever.
                            if next == 201 {
                                let has_fish = self.has_unique("Fish");
                                const CRAFT_COST: u64 = 1_000_000;
                                if !has_fish {
                                    self.narrator
                                        .say("The Fishing Rod requires THE FISH. You haven't caught it.");
                                } else if self.player.valu < CRAFT_COST {
                                    self.narrator.say(format!(
                                        "Crafting The Fishing Rod costs {CRAFT_COST}$V."
                                    ));
                                } else {
                                    self.player.valu -= CRAFT_COST;
                                    self.player.rods.max_owned = next;
                                    self.player.rods.equipped = next;
                                    self.narrator.say(format!(
                                        "*** CRAFTED {} for {CRAFT_COST}$V. The Fish stays with you. ***",
                                        rod.name
                                    ));
                                }
                            } else if next == 202 {
                                // The Rod: requires all 4 gods + The Fishing Rod owned.
                                // No valu cost; this is the apex of the pantheon.
                                let missing: Vec<&str> = ["Ish", "Fsh", "Fih", "Fis"]
                                    .into_iter()
                                    .filter(|n| !self.has_unique(n))
                                    .collect();
                                if owned < 201 {
                                    self.narrator
                                        .say("The Rod requires The Fishing Rod first.");
                                } else if !missing.is_empty() {
                                    self.narrator.say(format!(
                                        "The Rod requires the Pantheon. Missing: {}.",
                                        missing.join(", ")
                                    ));
                                } else {
                                    self.player.rods.max_owned = next;
                                    self.player.rods.equipped = next;
                                    self.narrator.say(
                                        "*** YOU ASSEMBLE THE PANTHEON. THE ROD IS YOURS. ***",
                                    );
                                    self.narrator.say(
                                        "Fish permits this. Ish rages, Fsh wonders, Fih laughs, Fis knows.",
                                    );
                                }
                            } else if self.buffs.free_rods > 0 {
                                self.buffs.free_rods -= 1;
                                self.player.rods.max_owned = next;
                                self.player.rods.equipped = next;
                                self.narrator.say(format!(
                                    "Free rod redeemed! Got #{next} - {} (no cost).",
                                    rod.name
                                ));
                            } else if let Some((req_diff, req_count)) =
                                crate::rod::mastery_gate(next).filter(|_| {
                                    !self.mastery_gate_met(crate::rod::mastery_gate(next).unwrap())
                                })
                            {
                                let have: u32 = self.mastery_count_at_difficulty(req_diff);
                                self.narrator.say(format!(
                                    "Mastery gate: need {req_count} catches of any difficulty-{req_diff} fish. ({have} so far.)"
                                ));
                            } else if self.player.valu >= rod.price() {
                                self.player.valu -= rod.price();
                                self.player.rods.max_owned = next;
                                self.player.rods.equipped = next;
                                self.narrator.say(format!(
                                    "Bought rod #{next} - {} for {}$V",
                                    rod.name,
                                    rod.price()
                                ));
                            } else {
                                self.narrator.say(format!(
                                    "Need {}$V to buy {}.",
                                    rod.price(),
                                    rod.name
                                ));
                            }
                        }
                    }
                    KeyCode::Char('e') => {
                        // equip the selected if owned
                        let tier = *cursor + 1;
                        if tier <= owned {
                            self.player.rods.equipped = tier;
                            self.narrator.say(format!("Equipped tier {tier}."));
                        }
                    }
                    _ => {}
                }
            }
            Scene::FishingSchool { cursor, tab } => {
                let branches = crate::skill_tree::TreeBranch::ALL;
                let branch = branches[(*tab).min(branches.len() - 1)];
                let nodes = crate::skill_tree::nodes_in_tree(branch);
                let n = nodes.len();
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => self.exit_subscene(),
                    KeyCode::Char('j') | KeyCode::Down => {
                        *cursor = (*cursor + 1).min(n.saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        *tab = if *tab == 0 { branches.len() - 1 } else { *tab - 1 };
                        *cursor = 0;
                    }
                    KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
                        *tab = (*tab + 1) % branches.len();
                        *cursor = 0;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if n == 0 { return; }
                        let node = nodes[(*cursor).min(n - 1)];
                        let lvl = self.skills.fishing_level();
                        let available = self.skill_tree.available(
                            lvl,
                            self.achievements.points_granted
                                + self.daily_bonus_points
                                + self.challenge_bonus_points,
                            self.mastery_milestones,
                            self.skills.encyclopedia_level(),
                        );
                        if available == 0 {
                            self.narrator.say("No skill points to spend.");
                        } else if self.skill_tree.rank(&node.id) >= node.max_rank {
                            self.narrator.say("Already at max rank.");
                        } else if !self.skill_tree.is_unlocked(node) {
                            self.narrator.say("Locked: invest 1 point in a parent node first.");
                        } else {
                            crate::skill_tree::invest(&mut self.skill_tree, node);
                            self.narrator.say(format!(
                                "Invested 1 point in {} (now {}/{}).",
                                node.label,
                                self.skill_tree.rank(&node.id),
                                node.max_rank
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Scene::Boss(_) => {
                // F/V/J/N already handled before the mode dispatch; only esc/q
                // here lets the player back out (counts as forfeit).
                if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.narrator.say("You forfeit the fight.");
                    self.scene = Scene::Overworld;
                }
            }
            Scene::Fishing(_) => {
                if matches!(code, KeyCode::Char('q')) {
                    self.exit_subscene();
                }
            }
            Scene::Help(_) | Scene::Stats => {
                if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.scene = Scene::Overworld;
                }
            }
            Scene::Settings => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.settings_cursor = (self.settings_cursor + 1).min(2);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.settings_cursor = self.settings_cursor.saturating_sub(1);
                }
                KeyCode::Char('h') | KeyCode::Left => self.adjust_setting(-1),
                KeyCode::Char('l') | KeyCode::Right => self.adjust_setting(1),
                _ => {}
            },
            Scene::Map { offset } => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                KeyCode::Left | KeyCode::Char('h') => offset.0 -= 1,
                KeyCode::Right | KeyCode::Char('l') => offset.0 += 1,
                KeyCode::Up | KeyCode::Char('k') => offset.1 -= 1,
                KeyCode::Down | KeyCode::Char('j') => offset.1 += 1,
                _ => {}
            },
            Scene::Quests { cursor } => {
                let active = active_quest_ids(&self.quest_done);
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                    KeyCode::Char('j') | KeyCode::Down => {
                        if !active.is_empty() {
                            *cursor = (*cursor + 1).min(active.len() - 1);
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Char('p') => {
                        if let Some(id) = active.get(*cursor) {
                            if self.pinned_quest.as_deref() == Some(id.as_str()) {
                                self.pinned_quest = None;
                                self.narrator.say("Unpinned quest.");
                            } else {
                                self.pinned_quest = Some(id.clone());
                                self.narrator
                                    .say(format!("Pinned: {}", quest_title(id).unwrap_or("?")));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Scene::Fishmonger { .. } => {
                // Take ownership of the scene so we can mutate step
                // without holding a borrow on self.scene while we
                // also mutate self.player/etc.
                let prev = std::mem::replace(&mut self.scene, Scene::Overworld);
                if let Scene::Fishmonger { cursor, step } = prev {
                    let (next_cursor, next_step) =
                        self.handle_fishmonger(code, key.kind, cursor, step);
                    if let Some(step) = next_step {
                        self.scene = Scene::Fishmonger {
                            cursor: next_cursor,
                            step,
                        };
                    }
                    // else: returned to Overworld already
                }
                return;
            }
            Scene::LootPool { cursor } => {
                let n = LOOT_POOLS.len();
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                    KeyCode::Char('j') | KeyCode::Down => {
                        *cursor = (*cursor + 1).min(n.saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let pool = LOOT_POOLS[*cursor];
                        if pool.0.is_empty() {
                            self.current_pool_override = None;
                            self.narrator.say("Pool cleared - normal fishing.");
                        } else {
                            self.current_pool_override = Some(pool.0.to_string());
                            self.narrator.say(format!(
                                "Pool locked to {}. The Rod lets you fish anywhere.",
                                pool.1
                            ));
                        }
                        self.scene = Scene::Overworld;
                    }
                    _ => {}
                }
            }
            Scene::Debug { cursor } => {
                let entries = debug_entries_count();
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.scene = Scene::Overworld;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        *cursor = (*cursor + 1).min(entries.saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        let c = *cursor;
                        self.debug_adjust(c, -1);
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        let c = *cursor;
                        self.debug_adjust(c, 1);
                    }
                    KeyCode::Char('H') => {
                        let c = *cursor;
                        self.debug_adjust(c, -100);
                    }
                    KeyCode::Char('L') => {
                        let c = *cursor;
                        self.debug_adjust(c, 100);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let c = *cursor;
                        self.debug_action(c);
                    }
                    _ => {}
                }
            }
            Scene::Inventory { tab } => match code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => {
                    self.scene = Scene::Overworld;
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
                    *tab = (*tab + 1) % Category::all().len();
                }
                KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
                    *tab = (*tab + Category::all().len() - 1) % Category::all().len();
                }
                KeyCode::Char('c') => {
                    // Quick-cook shortcut: only works while standing next
                    // to a cooking pot (mirror of the `:cook` rule).
                    if !self.is_near_cooking_pot() {
                        self.narrator.say(
                            "You need to be at a cooking pot. Find the Chef's pot in the village.",
                        );
                    } else {
                        self.scene = Scene::Cooking {
                            cursor: 0,
                            filter: String::new(),
                            editing_filter: false,
                        };
                    }
                }
                _ => {}
            },
            Scene::Notes(buf) => {
                match code {
                    KeyCode::Esc => {
                        // save and leave
                        let txt = buf.to_text();
                        match notes::save(&txt) {
                            Ok(()) => self.narrator.say("Notebook saved."),
                            Err(e) => self.narrator.say(format!("Note save failed: {e}")),
                        }
                        self.scene = Scene::Overworld;
                    }
                    KeyCode::Enter => {
                        let row = buf.cursor_row;
                        let split_at = {
                            let line = &buf.lines[row];
                            line.char_indices()
                                .nth(buf.cursor_col)
                                .map(|(i, _)| i)
                                .unwrap_or(line.len())
                        };
                        let rest = buf.lines[row].split_off(split_at);
                        buf.lines.insert(row + 1, rest);
                        buf.cursor_row += 1;
                        buf.cursor_col = 0;
                    }
                    KeyCode::Backspace => {
                        if buf.cursor_col > 0 {
                            let byte = buf.lines[buf.cursor_row]
                                .char_indices()
                                .nth(buf.cursor_col - 1)
                                .map(|(i, _)| i);
                            if let Some(b) = byte {
                                let end = buf.lines[buf.cursor_row]
                                    .char_indices()
                                    .nth(buf.cursor_col)
                                    .map(|(i, _)| i)
                                    .unwrap_or(buf.lines[buf.cursor_row].len());
                                buf.lines[buf.cursor_row].replace_range(b..end, "");
                                buf.cursor_col -= 1;
                            }
                        } else if buf.cursor_row > 0 {
                            let cur = buf.lines.remove(buf.cursor_row);
                            buf.cursor_row -= 1;
                            buf.cursor_col = buf.lines[buf.cursor_row].chars().count();
                            buf.lines[buf.cursor_row].push_str(&cur);
                        }
                    }
                    KeyCode::Left => {
                        if buf.cursor_col > 0 {
                            buf.cursor_col -= 1;
                        } else if buf.cursor_row > 0 {
                            buf.cursor_row -= 1;
                            buf.cursor_col = buf.lines[buf.cursor_row].chars().count();
                        }
                    }
                    KeyCode::Right => {
                        let len = buf.lines[buf.cursor_row].chars().count();
                        if buf.cursor_col < len {
                            buf.cursor_col += 1;
                        } else if buf.cursor_row + 1 < buf.lines.len() {
                            buf.cursor_row += 1;
                            buf.cursor_col = 0;
                        }
                    }
                    KeyCode::Up => {
                        if buf.cursor_row > 0 {
                            buf.cursor_row -= 1;
                            buf.cursor_col = buf
                                .cursor_col
                                .min(buf.lines[buf.cursor_row].chars().count());
                        }
                    }
                    KeyCode::Down => {
                        if buf.cursor_row + 1 < buf.lines.len() {
                            buf.cursor_row += 1;
                            buf.cursor_col = buf
                                .cursor_col
                                .min(buf.lines[buf.cursor_row].chars().count());
                        }
                    }
                    KeyCode::Char(c) => {
                        let byte = buf.lines[buf.cursor_row]
                            .char_indices()
                            .nth(buf.cursor_col)
                            .map(|(i, _)| i)
                            .unwrap_or(buf.lines[buf.cursor_row].len());
                        buf.lines[buf.cursor_row].insert(byte, c);
                        buf.cursor_col += 1;
                    }
                    _ => {}
                }
            }
            Scene::Dialogue { npc, line } => {
                let total = npc.dialogue.len();
                match code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if *line + 1 >= total {
                            self.scene = Scene::Overworld;
                        } else {
                            *line += 1;
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.scene = Scene::Overworld;
                    }
                    _ => {}
                }
            }
            Scene::NamePrompt(_) => {}
            Scene::Mining(_) => self.handle_mining_key(code),
            Scene::Blacksmith { .. } => self.handle_blacksmith_key(code),
            Scene::SellGear { .. } => self.handle_sell_gear_key(code),
            Scene::Gear { .. } => self.handle_gear_key(code),
            Scene::Smelt { .. } => self.handle_smelt_key(code),
            Scene::Forge { .. } => self.handle_forge_key(code),
            Scene::HouseInterior { .. } => self.handle_house_key(code),
            Scene::TackleShop { .. } => self.handle_tackle_key(code),
            Scene::BaitShop { .. } => self.handle_bait_key(code),
            Scene::Shipwright { .. } => self.handle_shipwright_key(code),
            Scene::Chopping(_) => self.handle_chop_key(code),
            Scene::Cooking { .. } => self.handle_cooking_key(code),
            Scene::Achievements { cursor } => match code {
                KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                KeyCode::Char('j') | KeyCode::Down => {
                    let n = crate::achievements::chains().len();
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *cursor = cursor.saturating_sub(1);
                }
                _ => {}
            },
            Scene::BugCatch(_) => self.handle_bug_catch_key(code),
            Scene::Perf => {
                // Esc / q closes the overlay; every other key falls through
                // to the overworld handler so the player can keep walking
                // while watching the perf numbers update live.
                if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.scene = Scene::Overworld;
                } else {
                    self.handle_overworld(code);
                }
            }
            Scene::LureBench { cursor } => {
                let recipes = crate::lure_recipes::recipes();
                let n = recipes.len();
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                    KeyCode::Char('j') | KeyCode::Down => {
                        if n > 0 {
                            *cursor = (*cursor + 1).min(n - 1);
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let idx = *cursor;
                        self.craft_lure(idx);
                    }
                    _ => {}
                }
            }
            Scene::Scales { cursor } => {
                let n = Self::SCALES_AXES.len();
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => self.scene = Scene::Overworld,
                    KeyCode::Char('j') | KeyCode::Down => {
                        if n > 0 {
                            *cursor = (*cursor + 1).min(n - 1);
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        *cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let idx = *cursor;
                        if let Some(axis) = Self::SCALES_AXES.get(idx) {
                            let spent = self.scales_spent.get(*axis).copied().unwrap_or(0);
                            if spent >= 1000 {
                                self.narrator
                                    .say(format!("{axis} is already maxed (1000/1000)."));
                            } else if self.scales == 0 {
                                self.narrator.say("No scales to spend.".to_string());
                            } else {
                                self.scales -= 1;
                                self.scales_spent
                                    .entry((*axis).to_string())
                                    .and_modify(|v| *v += 1)
                                    .or_insert(1);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_bait_key(&mut self, code: KeyCode) {
        let stock = &self.player.bait;
        let defs: Vec<&'static crate::bait::BaitDef> = crate::bait::defs()
            .iter()
            .filter(|d| d.cost > 0 || stock.count(&d.id) > 0)
            .collect();
        let n = defs.len();
        let Scene::BaitShop { cursor } = &mut self.scene else { return };
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.scene = Scene::Overworld;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    *cursor = (*cursor + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *cursor = cursor.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(d) = defs.get(*cursor) {
                    if crate::bait::is_wild(&d.id) {
                        self.narrator
                            .say(format!("{} is wild, not sold here.", d.name));
                        return;
                    }
                    let gate = d.min_rod_tier();
                    if self.player.rods.max_owned < gate {
                        self.narrator.say(format!(
                            "Need rod tier {gate} for {}. (You have tier {}.)",
                            d.name, self.player.rods.max_owned
                        ));
                        return;
                    }
                    if self.player.valu < d.cost {
                        self.narrator.say(format!(
                            "Need {}$V for one {}. You have {}$V.",
                            d.cost, d.name, self.player.valu
                        ));
                        return;
                    }
                    self.player.valu -= d.cost;
                    self.player.bait.add(&d.id, 1);
                    self.narrator
                        .say(format!("Bought 1 {} ({}$V).", d.name, d.cost));
                }
            }
            KeyCode::Char('e') => {
                // Equip the bait under the cursor (if owned).
                if let Some(d) = defs.get(*cursor) {
                    if self.player.bait.count(&d.id) > 0 {
                        self.player.bait.active = Some(d.id.clone());
                        self.narrator.say(format!("Equipped {}.", d.name));
                    } else {
                        self.narrator.say("You don't own any of this bait.");
                    }
                }
            }
            KeyCode::Char('u') => {
                self.player.bait.active = None;
                self.narrator.say("Unequipped bait.");
            }
            _ => {}
        }
    }

    fn handle_tackle_key(&mut self, code: KeyCode) {
        use crate::tackle::Slot;
        let Scene::TackleShop { slot_idx, cursor } = &mut self.scene else { return };
        let slot = Slot::ALL[*slot_idx % Slot::ALL.len()];
        let defs = crate::tackle::defs_for_slot(slot);
        let n = defs.len();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.scene = Scene::Overworld;
                return;
            }
            KeyCode::Char('h') | KeyCode::Left => {
                *slot_idx = (*slot_idx + Slot::ALL.len() - 1) % Slot::ALL.len();
                *cursor = 0;
                return;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                *slot_idx = (*slot_idx + 1) % Slot::ALL.len();
                *cursor = 0;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    *cursor = (*cursor + 1).min(n - 1);
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *cursor = cursor.saturating_sub(1);
                return;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(d) = defs.get(*cursor) {
                    let owned = self.player.tackle.tier(slot);
                    if d.tier <= owned {
                        self.narrator
                            .say(format!("Already own {} (tier {}).", d.name, d.tier));
                        return;
                    }
                    if d.tier != owned + 1 {
                        self.narrator
                            .say("Buy the previous tier first.");
                        return;
                    }
                    let gate = d.min_rod_tier();
                    if self.player.rods.max_owned < gate {
                        self.narrator.say(format!(
                            "Need rod tier {gate} to use this tackle. (You have tier {}.)",
                            self.player.rods.max_owned
                        ));
                        return;
                    }
                    if self.player.valu < d.cost {
                        self.narrator.say(format!(
                            "Need {}$V. You have {}$V.",
                            d.cost, self.player.valu
                        ));
                        return;
                    }
                    self.player.valu -= d.cost;
                    self.player.tackle.set_tier(slot, d.tier);
                    self.narrator
                        .say(format!("Bought {} ({}$V).", d.name, d.cost));
                }
                return;
            }
            _ => return,
        }
    }

    fn handle_house_key(&mut self, code: KeyCode) {
        let Scene::HouseInterior { px, py, return_xy, seed, .. } = self.scene else { return };
        let (fx, fy) = self.player.facing;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.player.x = return_xy.0;
                self.player.y = return_xy.1;
                self.scene = Scene::Overworld;
                self.narrator.say("You step back outside.");
            }
            KeyCode::Char('x') => {
                // Inspect what you're facing, just like the overworld.
                let target = crate::house::tile_at(px + fx, py + fy, seed);
                self.narrator.say(target.describe());
            }
            KeyCode::Char('f') => {
                // Interact with the tile you're facing. The Exit door is
                // the only interactable furniture for now.
                let target = crate::house::tile_at(px + fx, py + fy, seed);
                if matches!(target, crate::house::Furn::Exit) {
                    self.player.x = return_xy.0;
                    self.player.y = return_xy.1;
                    self.scene = Scene::Overworld;
                    self.narrator.say("You step back outside.");
                } else {
                    self.narrator.say("Nothing to do here.");
                }
            }
            _ => {}
        }
    }

    fn handle_blacksmith_key(&mut self, code: KeyCode) {
        const OPTIONS: usize = 3;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.scene = Scene::Overworld;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Scene::Blacksmith { cursor } = &mut self.scene {
                    *cursor = ((*cursor as usize + 1) % OPTIONS) as u8;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Scene::Blacksmith { cursor } = &mut self.scene {
                    *cursor = ((*cursor as usize + OPTIONS - 1) % OPTIONS) as u8;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let pick = if let Scene::Blacksmith { cursor } = self.scene {
                    cursor
                } else {
                    return;
                };
                match pick {
                    0 => {
                        self.sell_ore_and_ingots();
                        self.scene = Scene::Overworld;
                    }
                    1 => {
                        if self.player.gear.owned.is_empty() {
                            self.narrator.say("You haven't forged anything yet.");
                        } else {
                            self.scene = Scene::SellGear { cursor: 0 };
                        }
                    }
                    _ => {
                        self.scene = Scene::Overworld;
                    }
                }
            }
            _ => {}
        }
    }

    /// Slots editable in the Gear scene. Cape is omitted — auto-managed.
    const GEAR_SLOTS: [crate::gear::Slot; 4] = [
        crate::gear::Slot::Feet,
        crate::gear::Slot::Neck,
        crate::gear::Slot::Ring,
        crate::gear::Slot::Pickaxe,
    ];

    fn handle_gear_key(&mut self, code: KeyCode) {
        let n_slots = Self::GEAR_SLOTS.len();
        let cur_slot = if let Scene::Gear { slot_idx, .. } = self.scene {
            Self::GEAR_SLOTS[slot_idx.min(n_slots - 1)]
        } else {
            return;
        };
        let owned_in_slot: Vec<String> = self
            .player
            .gear
            .owned
            .iter()
            .filter(|id| {
                crate::gear::def_by_id(id)
                    .and_then(|d| d.slot_enum())
                    .map(|s| s == cur_slot)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.scene = Scene::Overworld;
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Scene::Gear { slot_idx, item_idx } = &mut self.scene {
                    *slot_idx = (*slot_idx + n_slots - 1) % n_slots;
                    *item_idx = 0;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Scene::Gear { slot_idx, item_idx } = &mut self.scene {
                    *slot_idx = (*slot_idx + 1) % n_slots;
                    *item_idx = 0;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Scene::Gear { item_idx, .. } = &mut self.scene {
                    if !owned_in_slot.is_empty() {
                        *item_idx = (*item_idx + 1).min(owned_in_slot.len() - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Scene::Gear { item_idx, .. } = &mut self.scene {
                    *item_idx = item_idx.saturating_sub(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let idx = if let Scene::Gear { item_idx, .. } = self.scene {
                    item_idx
                } else {
                    return;
                };
                if let Some(id) = owned_in_slot.get(idx).cloned() {
                    self.player.gear.equip(cur_slot, Some(id.clone()));
                    if let Some(def) = crate::gear::def_by_id(&id) {
                        self.narrator.say(format!("Equipped {}.", def.name));
                    }
                }
            }
            KeyCode::Char('u') => {
                self.player.gear.equip(cur_slot, None);
                self.narrator
                    .say(format!("Unequipped {} slot.", cur_slot.label()));
            }
            _ => {}
        }
    }

    fn handle_sell_gear_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.scene = Scene::Blacksmith { cursor: 1 };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.player.gear.owned.len();
                if let Scene::SellGear { cursor } = &mut self.scene {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Scene::SellGear { cursor } = &mut self.scene {
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let idx = if let Scene::SellGear { cursor } = self.scene {
                    cursor
                } else {
                    return;
                };
                if self.sell_forged_gear_at(idx) && self.player.gear.owned.is_empty() {
                    self.scene = Scene::Blacksmith { cursor: 1 };
                }
            }
            _ => {}
        }
    }

    /// Sell the gear at `idx` in `player.gear.owned`. Price = sum of
    /// the recipe's ingot values × 1.10. Counts as 1 against today's
    /// blacksmith cap. Removes from owned + unequips if currently worn.
    /// Returns true if a sale completed (so caller can clamp the cursor).
    fn sell_forged_gear_at(&mut self, idx: usize) -> bool {
        self.tick_market_day_rollover();
        let cap = self.blacksmith_daily_cap();
        if self.ore_sold_today >= cap {
            self.narrator.say(format!(
                "Blacksmith: \"Books are closed for today. (cap {cap}/day.)\""
            ));
            return false;
        }
        let id = match self.player.gear.owned.get(idx).cloned() {
            Some(id) => id,
            None => return false,
        };
        let Some(def) = crate::gear::def_by_id(&id) else { return false };
        let ingot_total: u64 = def
            .recipe
            .ingots
            .iter()
            .map(|(name, qty)| {
                crate::mining::ore_by_name(name)
                    .map(|o| o.ingot_value() * (*qty as u64))
                    .unwrap_or(0)
            })
            .sum();
        let mult = self.buffs.price_mult() * self.skill_tree.valu_mult();
        let price = ((ingot_total as f32) * 1.10 * mult).round() as u64;
        // remove from owned + unequip if currently worn
        self.player.gear.owned.remove(idx);
        for slot in crate::gear::Slot::ALL {
            if self.player.gear.equipped(slot) == Some(&id) {
                self.player.gear.equip(slot, None);
            }
        }
        self.player.valu = self.player.valu.saturating_add(price);
        self.lifetime_valu = self.lifetime_valu.saturating_add(price);
        self.stats.valu_earned = self.stats.valu_earned.saturating_add(price);
        self.ore_sold_today = self.ore_sold_today.saturating_add(1);
        self.narrator.say(format!(
            "Sold {} for {}$V. ({}/{} today)",
            def.name, price, self.ore_sold_today, cap,
        ));
        true
    }

    fn handle_smelt_key(&mut self, code: KeyCode) {
        match code {
            // Only Esc quits — 'q' is a valid character in ore names
            // (turquoise) and must remain typeable.
            KeyCode::Esc => {
                self.scene = Scene::Overworld;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.smeltable_ores().len();
                if let Scene::Smelt { cursor, typed } = &mut self.scene {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n.saturating_sub(1));
                    }
                    typed.clear();
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Scene::Smelt { cursor, typed } = &mut self.scene {
                    *cursor = cursor.saturating_sub(1);
                    typed.clear();
                }
                return;
            }
            KeyCode::Backspace => {
                if let Scene::Smelt { typed, .. } = &mut self.scene {
                    typed.pop();
                }
                return;
            }
            KeyCode::Char(c) => {
                // Type the ORE NAME (one per row) — same flavor as the
                // mining minigame, so smelting copper feels different from
                // smelting diamond.
                let ore_picked: Option<&'static crate::mining::OreDef> = {
                    let avail = self.smeltable_ores();
                    if let Scene::Smelt { cursor, typed } = &mut self.scene {
                        let idx = (*cursor).min(avail.len().saturating_sub(1));
                        let target: Option<&'static str> =
                            avail.get(idx).map(|(o, _)| o.name);
                        if let Some(target) = target {
                            let next_idx = typed.chars().count();
                            if let Some(exp) = target.chars().nth(next_idx) {
                                if c.eq_ignore_ascii_case(&exp) {
                                    typed.push(exp);
                                }
                            }
                            if typed.len() >= target.len() {
                                typed.clear();
                                avail.get(idx).map(|(o, _)| *o)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(ore) = ore_picked {
                    self.perform_smelt(ore);
                    // If that drained the row, clamp the cursor.
                    let n = self.smeltable_ores().len();
                    if let Scene::Smelt { cursor, .. } = &mut self.scene {
                        if n == 0 {
                            self.scene = Scene::Overworld;
                            return;
                        }
                        *cursor = (*cursor).min(n - 1);
                    }
                }
                return;
            }
            _ => {}
        }
    }

    fn handle_forge_key(&mut self, code: KeyCode) {
        match code {
            // Only Esc quits — 'q' is needed in some forged-piece names.
            KeyCode::Esc => {
                self.scene = Scene::Overworld;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.forgeable_gear().len();
                if let Scene::Forge { cursor, typed } = &mut self.scene {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n.saturating_sub(1));
                    }
                    typed.clear();
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Scene::Forge { cursor, typed } = &mut self.scene {
                    *cursor = cursor.saturating_sub(1);
                    typed.clear();
                }
                return;
            }
            KeyCode::Backspace => {
                if let Scene::Forge { typed, .. } = &mut self.scene {
                    typed.pop();
                }
                return;
            }
            KeyCode::Char(c) => {
                let picked: Option<&'static crate::gear::GearDef> = {
                    let avail = self.forgeable_gear();
                    if let Scene::Forge { cursor, typed } = &mut self.scene {
                        let idx = (*cursor).min(avail.len().saturating_sub(1));
                        let target = avail.get(idx).map(|d| d.name.to_ascii_lowercase());
                        if let Some(t) = target.as_deref() {
                            let next_idx = typed.chars().count();
                            if let Some(exp) = t.chars().nth(next_idx) {
                                // skip spaces transparently — typing fluid
                                if exp == ' ' {
                                    typed.push(' ');
                                    if let Some(exp2) = t.chars().nth(typed.chars().count()) {
                                        if c.eq_ignore_ascii_case(&exp2) {
                                            typed.push(exp2);
                                        }
                                    }
                                } else if c.eq_ignore_ascii_case(&exp) {
                                    typed.push(exp);
                                }
                            }
                            if typed.len() >= t.len() {
                                typed.clear();
                                avail.get(idx).copied()
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(def) = picked {
                    self.perform_forge(def);
                    let n = self.forgeable_gear().len();
                    if let Scene::Forge { cursor, .. } = &mut self.scene {
                        if n == 0 {
                            self.scene = Scene::Overworld;
                            return;
                        }
                        *cursor = (*cursor).min(n - 1);
                    }
                }
                return;
            }
            _ => {}
        }
    }

    fn handle_mining_key(&mut self, code: KeyCode) {
        match code {
            // Only Esc quits — 'q' is a valid character in ore names
            // (turquoise) and must be available to type.
            KeyCode::Esc => {
                self.scene = Scene::Overworld;
            }
            KeyCode::Char(c) => {
                if self.stamina <= 0.0 && !self.skill_tree.stamina_second_wind() {
                    self.narrator.say("Too tired to swing. Fish first.");
                    self.scene = Scene::Overworld;
                    return;
                }
                let completed = {
                    let Scene::Mining(m) = &mut self.scene else { return };
                    m.type_char(c)
                };
                if completed {
                    self.spend_stamina(1.0);
                }
                let Scene::Mining(m) = &mut self.scene else { return };
                if completed {
                    let key = (m.dim, m.x, m.y);
                    let ore = m.ore;
                    crate::mining::record_mine(&mut self.veins, key);
                    let item = crate::item::Item {
                        name: ore.name.to_string(),
                        category: crate::item::Category::Mineral,
                        description: "Mined from a vein. A smith will weigh it for you.".to_string(),
                    };
                    self.player.items.push(item);
                    let base_weight: u64 = (ore.value / 20).max(2);
                    let tackle_xp = 1.0 + self.player.tackle.sum_effect("xp_mult");
                    let faceless_boost = if crate::mining::now_secs() < self.mining_boost_until {
                        1.25
                    } else {
                        1.0
                    };
                    let weight = ((base_weight as f32)
                        * self.skill_tree.global_xp_mult()
                        * self.skill_tree.mining_xp_mult()
                        * tackle_xp
                        * faceless_boost)
                        as u64;
                    let weight = weight.max(1);
                    let before = self.skills.mining_level();
                    self.skills.mining_xp += weight;
                    let after = self.skills.mining_level();
                    self.show_xp_gain("Mining", weight, self.skills.mining_xp, after);
                    if after > before {
                        self.narrator
                            .say(format!("Mining level up! Now level {after}."));
                    }
                    self.narrator
                        .say(format!("You chip a {} loose.", ore.name));
                    self.scene = Scene::Overworld;
                }
            }
            _ => {}
        }
    }

    fn normal_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('i') | KeyCode::Char('a') => {
                self.mode = Mode::Insert;
            }
            KeyCode::Char(':') => {
                self.mode = Mode::Command(String::new());
            }
            _ => {}
        }
    }

    fn command_key(&mut self, code: KeyCode) {
        let Mode::Command(buf) = &mut self.mode else {
            return;
        };
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let cmd = std::mem::take(buf);
                self.mode = Mode::Normal;
                self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => {
                buf.push(c);
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        // Hidden developer console gate. Any command longer than 5 chars is
        // hashed (SHA-512) and compared to a hardcoded digest; on match,
        // open the debug scene. The plaintext command is never written to
        // source, so reading this file doesn't reveal the magic string.
        if trimmed.len() > 5 && debug_command_matches(trimmed) {
            self.scene = Scene::Debug { cursor: 0 };
            self.mode = Mode::Insert;
            self.narrator.say("*** developer console opened ***");
            return;
        }
        match trimmed {
            "w" => {
                self.do_save();
            }
            "wq" | "x" => {
                self.do_save();
                self.running = false;
            }
            "q" => {
                self.do_save();
                self.running = false;
            }
            "t" | "tasks" => {
                self.scene = Scene::Quests { cursor: 0 };
                self.mode = Mode::Insert;
            }
            "q!" => {
                self.running = false;
            }
            "e" => {
                self.narrator.say("You leaf through the fishdex.");
                self.scene = Scene::Fishdex(Fishdex::new());
                self.mode = Mode::Insert;
            }
            "n" | "notes" => {
                self.narrator.say("You open your notebook.");
                self.scene = Scene::Notes(NotesBuf::from_text(&notes::load()));
                self.mode = Mode::Insert;
            }
            "i" | "inv" | "inventory" => {
                self.scene = Scene::Inventory { tab: 0 };
                self.mode = Mode::Insert;
            }
            "c" | "controls" => {
                self.scene = Scene::Help(HelpTopic::Controls);
                self.mode = Mode::Insert;
            }
            "help" => {
                self.scene = Scene::Help(HelpTopic::Commands);
                self.mode = Mode::Insert;
            }
            "s" | "stats" => {
                self.scene = Scene::Stats;
                self.mode = Mode::Insert;
            }
            "scales" => {
                self.scene = Scene::Scales { cursor: 0 };
                self.mode = Mode::Insert;
            }
            "perf" | "performancemenu" => {
                self.scene = Scene::Perf;
                self.mode = Mode::Insert;
            }
            "prestige" => {
                self.do_prestige();
            }
            cmd if cmd.starts_with("cook ") || cmd == "cook" => {
                let name = cmd.strip_prefix("cook").unwrap_or("").trim();
                if name.is_empty() {
                    // Open the recipe menu — but only if standing at a
                    // cooking pot. Otherwise the player has to track one
                    // down (Chef's pot at home, or a procedural village).
                    if !self.is_near_cooking_pot() {
                        self.narrator.say(
                            "You need to be at a cooking pot. Find the Chef's pot in the village.",
                        );
                    } else {
                        self.scene = Scene::Cooking {
                            cursor: 0,
                            filter: String::new(),
                            editing_filter: false,
                        };
                        self.mode = Mode::Insert;
                        self.tutorial_advance(6);
                    }
                } else {
                    self.do_cook(name);
                }
            }
            "recipes" | "cookbook" | "rb" => {
                if !self.is_near_cooking_pot() {
                    self.narrator.say(
                        "You need to be at a cooking pot. Find the Chef's pot in the village.",
                    );
                } else {
                    self.scene = Scene::Cooking {
                        cursor: 0,
                        filter: String::new(),
                        editing_filter: false,
                    };
                    self.mode = Mode::Insert;
                    self.tutorial_advance(6);
                }
            }
            cmd if cmd.starts_with("feed") => {
                let arg = cmd.strip_prefix("feed").unwrap_or("").trim();
                let n: u32 = arg.parse().unwrap_or(1).max(1);
                self.do_feed_crew(n);
                self.tutorial_advance(8);
            }
            cmd if cmd.starts_with("process") => {
                let arg = cmd.strip_prefix("process").unwrap_or("").trim();
                self.do_process_fish(arg);
            }
            cmd if cmd.starts_with("burn") => {
                let arg = cmd.strip_prefix("burn").unwrap_or("").trim();
                let n: u32 = arg.parse().unwrap_or(1).max(1);
                self.do_burn_biofuel(n);
                self.tutorial_advance(8);
            }
            "chop" => {
                self.do_chop();
            }
            "shipwright" | "yard" => {
                self.do_open_shipwright();
            }
            "boss" => {
                // Trigger a boss fight against two random caught fish.
                let pool: Vec<usize> = self
                    .caught
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c)
                    .map(|(i, _)| i)
                    .collect();
                if pool.len() < 2 {
                    self.narrator.say("Catch at least two fish first.");
                } else {
                    let r1 = crate::fish::next_rand_f32(&mut self.rng_state);
                    let r2 = crate::fish::next_rand_f32(&mut self.rng_state);
                    let a = pool[((r1 * pool.len() as f32) as usize).min(pool.len() - 1)];
                    let b = pool[((r2 * pool.len() as f32) as usize).min(pool.len() - 1)];
                    let fa = &crate::fishlist::fish()[a];
                    let fb = &crate::fishlist::fish()[b];
                    self.scene = Scene::Boss(crate::boss::Boss::new(
                        fa,
                        fb,
                        self.rng_state,
                        self.skills.fishing_level(),
                    ));
                    self.mode = Mode::Insert;
                    self.narrator.say(format!(
                        "BOSS: {} & {} approach. F/V left, J/N right.",
                        fa.name, fb.name
                    ));
                }
            }
            "bounty" => {
                if let Some(b) = &self.bounty {
                    self.narrator.say(format!(
                        "Active bounty: {} ({}/{}).",
                        b.title(),
                        b.progress,
                        b.count
                    ));
                } else if let Some(b) = crate::procedural_quests::roll(
                    &self.caught,
                    &mut self.rng_state,
                ) {
                    self.narrator.say(format!(
                        "New bounty: {}. Reward: +{}$V, +{} skill point(s).",
                        b.title(),
                        b.reward_valu,
                        b.reward_points
                    ));
                    self.bounty = Some(b);
                } else {
                    self.narrator
                        .say("Catch at least one non-unique fish first.");
                }
            }
            "abandon" => {
                if self.bounty.take().is_some() {
                    self.narrator.say("Bounty abandoned.");
                } else {
                    self.narrator.say("No bounty to abandon.");
                }
            }
            "saves" => {
                let metas = save::list_saves_meta();
                if metas.is_empty() {
                    self.narrator.say("No save files on disk yet.");
                } else {
                    self.narrator.say(format!("Saves on disk ({}):", metas.len()));
                    for (name, bytes) in &metas {
                        self.narrator.say(format!("  {} ({} B)", name, bytes));
                    }
                }
            }
            "m" | "map" => {
                self.mark_seen_around_player();
                self.scene = Scene::Map { offset: (0, 0) };
                self.mode = Mode::Insert;
            }
            "o" | "options" | "settings" => {
                self.scene = Scene::Settings;
                self.mode = Mode::Insert;
            }
            "h" => self.narrator.say("Try :help for commands, :c for controls."),
            "inspect" | "look" => self.inspect_surroundings(),
            "g" | "pickup" => self.pickup_here(),
            "tackle" | "tk" => {
                self.scene = Scene::TackleShop { slot_idx: 0, cursor: 0 };
                self.mode = Mode::Insert;
            }
            "bait" | "b" => {
                self.scene = Scene::BaitShop { cursor: 0 };
                self.mode = Mode::Insert;
            }
            "a" | "achievements" => {
                self.scene = Scene::Achievements { cursor: 0 };
                self.mode = Mode::Insert;
            }
            "k" | "pool" => {
                if self.player.rods.equipped >= 202 {
                    self.scene = Scene::LootPool { cursor: 0 };
                    self.mode = Mode::Insert;
                    self.narrator.say("THE ROD hums. Choose your pool.");
                } else {
                    self.narrator.say("Need The Rod (tier 202) to choose a pool.");
                }
            }
            "l" | "leave" | "surface" => {
                if self.world.dim == crate::world::Dimension::Surface {
                    self.narrator
                        .say("You are already on the surface.");
                } else {
                    self.world.dim = crate::world::Dimension::Surface;
                    self.narrator.say("You return to Sentinel.");
                }
            }
            "" => {}
            // :travel <name>  — jump to a specialty dim if your rod tier
            // qualifies. Surface is always free; leaving uses :l.
            other if other.starts_with("travel ") || other.starts_with("t ") => {
                let name = other.splitn(2, ' ').nth(1).unwrap_or("").trim();
                if let Some(dim) = crate::world::Dimension::from_name(name) {
                    let gate = dim.min_rod_tier();
                    if self.player.rods.max_owned < gate {
                        self.narrator.say(format!(
                            "{} needs rod tier {gate}. (You have tier {}.)",
                            dim.label(),
                            self.player.rods.max_owned
                        ));
                    } else {
                        self.world.dim = dim;
                        self.snap_player_to_walkable();
                        self.narrator.say(format!("You arrive at: {}.", dim.label()));
                    }
                } else {
                    self.narrator
                        .say(format!("Unknown destination: '{name}'."));
                }
            }
            "gear" | "eq" => {
                self.scene = Scene::Gear { slot_idx: 0, item_idx: 0 };
                self.mode = Mode::Insert;
            }
            "smelt" => {
                if !self.is_near_blacksmith() {
                    self.narrator
                        .say("You'd need a blacksmith nearby. Find one in any village.");
                } else if self.smeltable_ores().is_empty() {
                    self.narrator
                        .say("Nothing smeltable. Mine some ore first.");
                } else {
                    self.scene = Scene::Smelt { cursor: 0, typed: String::new() };
                    self.mode = Mode::Insert;
                }
            }
            "sellore" | "sell-ore" | "smelt-sell" => {
                if !self.is_near_blacksmith() {
                    self.narrator
                        .say("You'd need a blacksmith nearby. Find one in any village.");
                } else {
                    self.sell_ore_and_ingots();
                }
            }
            "forge" => {
                if !self.is_near_blacksmith() {
                    self.narrator
                        .say("You'd need a blacksmith nearby. Find one in any village.");
                } else if self.forgeable_gear().is_empty() {
                    self.narrator
                        .say("Nothing to forge. Smelt some ingots, level up Blacksmithing, or save up valu.");
                } else {
                    self.scene = Scene::Forge { cursor: 0, typed: String::new() };
                    self.mode = Mode::Insert;
                }
            }
            other => self.narrator.say(format!("Unknown command: :{other}")),
        }
    }

    /// (ore, raw_count) pairs for every ore the player has at least
    /// `ore.ore_per_ingot` raw chunks of. Used by the smelt UI.
    fn smeltable_ores(&self) -> Vec<(&'static crate::mining::OreDef, u32)> {
        let mut counts: std::collections::BTreeMap<&'static str, u32> = Default::default();
        for it in &self.player.items {
            if matches!(it.category, crate::item::Category::Mineral) {
                *counts.entry(canonical_ore_name(&it.name)).or_default() += 1;
            }
        }
        let mut out = Vec::new();
        for ore in crate::mining::ORES.iter() {
            let c = counts.get(ore.name).copied().unwrap_or(0);
            if c >= ore.ore_per_ingot {
                out.push((ore, c));
            }
        }
        out
    }

    /// Consume `ore.ore_per_ingot` raw ore items of this type from the
    /// player's inventory and produce one ingot. Returns true if performed.
    fn perform_smelt(&mut self, ore: &'static crate::mining::OreDef) -> bool {
        let need = ore.ore_per_ingot as usize;
        let mut indices: Vec<usize> = Vec::new();
        for (i, it) in self.player.items.iter().enumerate() {
            if matches!(it.category, crate::item::Category::Mineral)
                && canonical_ore_name(&it.name) == ore.name
            {
                indices.push(i);
                if indices.len() == need {
                    break;
                }
            }
        }
        if indices.len() < need {
            return false;
        }
        // Drain in reverse so indices stay valid.
        for &i in indices.iter().rev() {
            self.player.items.remove(i);
        }
        *self.player.ingots.entry(ore.name.to_string()).or_insert(0) += 1;
        let xp = (ore.tier as u64).pow(2) * 6 + 8;
        let before = self.skills.blacksmithing_level();
        self.skills.blacksmithing_xp += xp;
        let after = self.skills.blacksmithing_level();
        self.show_xp_gain("Blacksmithing", xp, self.skills.blacksmithing_xp, after);
        if after > before {
            self.narrator
                .say(format!("Blacksmithing level up! Now level {after}."));
        }
        self.narrator
            .say(format!("You smelt {} {} into a {} ingot.", need, ore.name, ore.name));
        true
    }

    /// Every GearDef the player currently meets the requirements for
    /// (skill level + ingot stockpile + valu). Used by the forge UI.
    fn forgeable_gear(&self) -> Vec<&'static crate::gear::GearDef> {
        crate::gear::defs()
            .iter()
            .filter(|d| self.can_forge(d))
            .collect()
    }

    fn can_forge(&self, def: &crate::gear::GearDef) -> bool {
        if self.skills.blacksmithing_level() < def.min_blacksmithing_level {
            return false;
        }
        if self.skills.mining_level() < def.min_mining_level {
            return false;
        }
        if self.player.valu < def.recipe.valu {
            return false;
        }
        for (id, qty) in &def.recipe.ingots {
            if self.player.ingots.get(id).copied().unwrap_or(0) < *qty {
                return false;
            }
        }
        true
    }

    fn perform_forge(&mut self, def: &'static crate::gear::GearDef) -> bool {
        if !self.can_forge(def) {
            return false;
        }
        // Pay costs.
        self.player.valu -= def.recipe.valu;
        for (id, qty) in &def.recipe.ingots {
            let entry = self.player.ingots.entry(id.clone()).or_insert(0);
            *entry = entry.saturating_sub(*qty);
        }
        // Bank the gear. Auto-equip *only* if the slot is currently empty
        // — never clobber a deliberately-worn piece. Use `:gear` to swap.
        self.player.gear.add_owned(&def.id);
        if let Some(slot) = def.slot_enum() {
            if self.player.gear.equipped(slot).is_none() {
                self.player.gear.equip(slot, Some(def.id.clone()));
            }
        }
        // BS XP scales with tier.
        let xp = (def.tier as u64).pow(2) * 25 + 50;
        let before = self.skills.blacksmithing_level();
        self.skills.blacksmithing_xp += xp;
        let after = self.skills.blacksmithing_level();
        self.show_xp_gain("Blacksmithing", xp, self.skills.blacksmithing_xp, after);
        if after > before {
            self.narrator
                .say(format!("Blacksmithing level up! Now level {after}."));
        }
        self.narrator
            .say(format!("You forge a {}.", def.name));
        true
    }

    /// Compute the current weather honouring season-filter rules. Used
    /// from all the formerly-direct `weather::weather_for(..)` callers so
    /// they all see the same season-aware value.
    fn current_weather(&self) -> crate::weather::Weather {
        let secs = self.total_play_secs();
        let day = crate::gametime::game_days(secs);
        let season = crate::gametime::season(secs);
        crate::weather::weather_for_with_season(
            day,
            self.world.dim,
            self.current_biome.unwrap_or(crate::world::Biome::Meadow),
            self.world.seed,
            season,
        )
    }

    /// True if the player has caught the named unique fish (Fish, Ish, Fsh, ...).
    fn has_unique(&self, name: &str) -> bool {
        fishlist::fish()
            .iter()
            .position(|f| f.unique && f.name == name)
            .and_then(|i| self.caught.get(i).copied())
            .unwrap_or(false)
    }

    /// Force-grants a unique fish to the player as if they'd caught it. Used
    /// for Pantheon gods that arrive via meta-progression (Fsh at 100 catches,
    /// Fih at 100h, Fis at 200 species) rather than the rod minigame.
    fn grant_unique(&mut self, name: &str, where_from: &str) {
        let Some(i) = fishlist::fish()
            .iter()
            .position(|f| f.unique && f.name == name)
        else {
            return;
        };
        if self.caught.get(i).copied().unwrap_or(false) {
            return;
        }
        let fish_ref = &fishlist::fish()[i];
        self.caught[i] = true;
        self.on_fish_first_discovered(i);
        if let Some(slot) = self.caught_at.get_mut(i) {
            if slot.is_none() {
                *slot = Some((where_from.to_string(), "-".to_string()));
            }
        }
        self.player.inventory.push(fish_ref);
        self.narrator
            .say(format!("*** {} ARRIVES. {} ***", name.to_uppercase(), where_from));
        self.narrator.say(format!("{}", fish_ref.description));
    }

    /// Check the Pantheon force-grant conditions. Idempotent: each god is
    /// granted at most once, and the check is cheap (early-exit on already-have).
    fn check_pantheon_unlocks(&mut self) {
        // Fsh: curiosity arrives after 100 lifetime catches.
        if !self.has_unique("Fsh") && self.stats.fish_caught >= 100 {
            self.grant_unique("Fsh", "Drawn from the forest by 100 catches");
        }
        // Fih: happiness arrives at 100 hours played.
        if !self.has_unique("Fih") && self.total_play_secs() >= 100 * 3600 {
            self.grant_unique("Fih", "Drawn from the swamp by 100 hours of life");
        }
        // Fis: wiseness arrives at 200 unique species (gods count too).
        let species = self.caught.iter().filter(|c| **c).count() as u64;
        if !self.has_unique("Fis") && species >= 200 {
            self.grant_unique("Fis", "Drawn from the tundra by 200 species");
        }
    }

    fn exit_subscene(&mut self) {
        match &self.scene {
            Scene::Fishing(g) => {
                let fish_ref: &'static crate::fish::FishDef = g.fish;
                let raw_caught = matches!(g.finished, Some(FishingResult::Caught));
                let mut escaped = matches!(g.finished, Some(FishingResult::Escaped));
                // Blessed (escape_save_pct): roll once on every escape, and
                // on success flip it into a catch instead.
                let mut caught = raw_caught;
                if escaped {
                    let p = self.skill_tree.escape_save_pct();
                    if p > 0.0 {
                        let r = crate::fish::next_rand_f32(&mut self.rng_state);
                        if r < p {
                            caught = true;
                            escaped = false;
                            self.narrator.say("Blessed: the line held. You bring it in anyway.");
                        }
                    }
                }
                if caught {
                    let mut already_had_unique = false;
                    if let Some(i) = fishlist::fish().iter().position(|f| std::ptr::eq(f, fish_ref)) {
                        if fish_ref.unique && self.caught.get(i).copied().unwrap_or(false) {
                            already_had_unique = true;
                        }
                        let first_time = !self.caught.get(i).copied().unwrap_or(false);
                        self.caught[i] = true;
                        if first_time {
                            self.on_fish_first_discovered(i);
                        }
                        if let Some(slot) = self.caught_at.get_mut(i) {
                            if slot.is_none() {
                                *slot = self.pending_catch_loc.clone();
                            }
                        }
                        // compute these BEFORE taking &mut slot so we don't
                        // hold conflicting borrows of self.
                        let secs = self.total_play_secs();
                        let w = self.current_weather();
                        let tod = crate::gametime::time_of_day(secs);
                        let season = crate::gametime::season(secs);
                        if let Some(slot) = self.caught_context.get_mut(i) {
                            if slot.is_none() {
                                *slot = Some((
                                    tod.label().to_string(),
                                    format!("{} {}", w.category(), w.value()),
                                    season.label().to_string(),
                                ));
                            }
                        }
                    }
                    if fish_ref.unique && already_had_unique {
                        self.narrator.say(format!(
                            "You see the {} again but you already have it. It slips back into the deep.",
                            fish_ref.name
                        ));
                    } else {
                        // Atlantis Population is a quantity multiplier:
                        // Low=1, Medium=2, High=3 copies per catch.
                        let w = self.current_weather();
                        let copies = match w {
                            crate::weather::Weather::PopMedium => 2,
                            crate::weather::Weather::PopHigh => 3,
                            _ => 1,
                        };
                        // Necklace perk: roll for a bonus copy (uniques excluded).
                        let dbl_chance = self.player.gear.combined_perks().double_fish_chance;
                        let bonus = if !fish_ref.unique
                            && dbl_chance > 0.0
                            && crate::fish::next_rand_f32(&mut self.rng_state) < dbl_chance
                        {
                            1
                        } else {
                            0
                        };
                        // unique fish never duplicate
                        let actual_copies = if fish_ref.unique { 1 } else { copies + bonus };
                        for _ in 0..actual_copies {
                            self.player.inventory.push(fish_ref);
                        }
                        if bonus > 0 {
                            self.narrator
                                .say("Your necklace hums — a second fish slides into the basket.");
                        }
                        let name = fish_ref.name.clone();
                        if fish_ref.unique {
                            self.narrator
                                .say(format!("*** YOU REEL IN THE {}! ***", name.to_uppercase()));
                        } else if actual_copies > 1 {
                            self.narrator.say(format!(
                                "You reel in {} {}s! (Atlantean Population bonus)",
                                actual_copies, name
                            ));
                        } else {
                            self.narrator
                                .say(format!("You reel in a {}!", name));
                        }
                        self.narrator.say(format!(
                            "Basket: {} fish.",
                            self.player.inventory.len()
                        ));
                    }
                    let base_xp = fish_catch_xp(fish_ref.difficulty);
                    let tackle_xp = 1.0 + self.player.tackle.sum_effect("xp_mult");
                    let bait_xp = match &self.bait_pending {
                        Some((e, m)) if e == "xp_mult" => 1.0 + *m,
                        _ => 1.0,
                    };
                    let dim_bonus = self.dim_bonus_mult();
                    let scales_xp = 1.0 + self.scales_bonus("xp_mult");
                    let prestige_xp = self.prestige_xp_mult();
                    let landmark_xp = 1.0 + self.landmark_bonus("xp_mult");
                    let gained = ((base_xp as f32)
                        * self.skill_tree.global_xp_mult()
                        * tackle_xp
                        * bait_xp
                        * dim_bonus
                        * scales_xp
                        * prestige_xp
                        * landmark_xp) as u64;
                    let gained = gained.max(1);
                    // Bait valu_mult: pay a one-time bonus equal to the fish's
                    // sell price * magnitude, right now (so the player doesn't
                    // have to remember which fish was caught with which bait).
                    if let Some((e, m)) = self.bait_pending.clone() {
                        if e == "valu_mult" {
                            let bonus = ((fish_ref.sell_price() as f32) * m) as u64;
                            self.player.valu = self.player.valu.saturating_add(bonus);
                            self.lifetime_valu = self.lifetime_valu.saturating_add(bonus);
                            self.stats.valu_earned = self.stats.valu_earned.saturating_add(bonus);
                            self.narrator.say(format!("+{bonus}$V bait bonus."));
                        }
                    }
                    self.bait_pending = None;
                    self.bait_pending_bite_speed = 0.0;
                    self.bait_pending_pool_pull = None;
                    // Combo chain ticks down after each cast resolves.
                    if self.buffs.combo_chain_left > 0 {
                        self.buffs.combo_chain_left -= 1;
                        if self.buffs.combo_chain_left == 0 {
                            self.buffs.combo_chain_mult = 0.0;
                            self.narrator.say("Combo chain expired.".to_string());
                        }
                    }
                    // Scales drop: ~5% per catch. Token currency is its own
                    // axis-shaped slow-burn upgrade — see `:scales`.
                    if crate::fish::next_rand_f32(&mut self.rng_state) < 0.05 {
                        self.scales = self.scales.saturating_add(1);
                        self.narrator.say("A scale catches in the line. (+1 scale)".to_string());
                    }
                    // Each catch gives a small fixed dose of stamina back
                    // — 5..=10, jittered per catch. Scaled by Relaxed.
                    let r = crate::fish::next_rand_f32(&mut self.rng_state);
                    let relax = (5.0 + r * 5.0) * self.skill_tree.stamina_fish_regen_mult();
                    self.grant_stamina(relax);
                    self.stats.fish_caught += 1;
                    self.stats.catch_streak = self.stats.catch_streak.saturating_add(1);
                    if self.stats.catch_streak > self.stats.max_catch_streak {
                        self.stats.max_catch_streak = self.stats.catch_streak;
                    }
                    // Shiny roll: classic 1/8192, halved odds (1/4096)
                    // while the Shiny Charm is owned. Purely cosmetic;
                    // no effect on stats, sale, mastery, or anything else.
                    let shiny_divisor: f32 =
                        if self.player.has_shiny_charm { 4096.0 } else { 8192.0 };
                    if crate::fish::next_rand_f32(&mut self.rng_state) < 1.0 / shiny_divisor {
                        self.stats.shiny_catches =
                            self.stats.shiny_catches.saturating_add(1);
                        let fish_idx = fishlist::fish()
                            .iter()
                            .position(|f| std::ptr::eq(f, fish_ref));
                        if let Some(i) = fish_idx {
                            if let Some(slot) = self.shiny_per_species.get_mut(i) {
                                *slot = slot.saturating_add(1);
                            }
                        }
                        self.narrator.say(format!(
                            "*** SHINY {} !! 1/{}. ***",
                            fish_ref.name.to_uppercase(),
                            shiny_divisor as u32
                        ));
                        // Milestone: 1000 lifetime shinies grants the
                        // Shiny Charm. Goes into Misc; doubles the rate;
                        // does nothing else.
                        if !self.player.has_shiny_charm
                            && self.stats.shiny_catches >= 1000
                        {
                            self.player.has_shiny_charm = true;
                            self.player.items.push(crate::item::Item {
                                name: "Shiny Charm".to_string(),
                                category: crate::item::Category::Misc,
                                description: String::new(),
                            });
                            self.narrator.say(
                                "*** SHINY CHARM acquired. Shiny rate doubled. ***"
                                    .to_string(),
                            );
                        }
                    }
                    // Crew hunger ticks up on every catch made while aboard
                    // the boat. Saturating at 100; the cast-block kicks in
                    // when it hits 100 so the player must :feed before the
                    // next cast.
                    if self.player.on_boat {
                        self.player.crew_hunger = (self.player.crew_hunger + 1).min(100);
                        if self.player.crew_hunger == 100 {
                            self.narrator.say(
                                "The crew is starving. Feed them (:feed <n>) before casting again."
                                    .to_string(),
                            );
                        }
                    }
                    // Roll a size class and apply mastery-challenge progress.
                    let size = crate::mastery_challenges::roll_size(
                        crate::fish::next_rand_f32(&mut self.rng_state),
                    );
                    if matches!(size,
                        crate::mastery_challenges::SizeClass::Large
                        | crate::mastery_challenges::SizeClass::Huge)
                    {
                        self.narrator.say(format!(
                            "...{} {}.",
                            size.label().to_uppercase(),
                            fish_ref.name
                        ));
                    }
                    self.tick_streak(&fish_ref.name);
                    self.tick_mastery_challenges_catch(&fish_ref.name, size);
                    // Fish mastery: bump per-species counter. The narrator
                    // celebrates the soft milestones (1/5/10/25/50/100), but
                    // only the deeper ones (50, 250) grant a skill point —
                    // otherwise 600+ species × 6 free points each drowns
                    // the tree in income and it self-completes from a single
                    // "catch one of everything" sweep.
                    if let Some(i) = fishlist::fish().iter().position(|f| std::ptr::eq(f, fish_ref)) {
                        let before_m = self.mastery.get(i).copied().unwrap_or(0);
                        if let Some(slot) = self.mastery.get_mut(i) {
                            *slot = slot.saturating_add(1);
                        }
                        let after_m = before_m + 1;
                        const NARRATE_AT: &[u32] = &[1, 5, 10, 25, 50, 100, 250];
                        const POINT_AT: &[u32] = &[50, 250];
                        if NARRATE_AT.contains(&after_m) {
                            let grants_point = POINT_AT.contains(&after_m);
                            if grants_point {
                                self.mastery_milestones =
                                    self.mastery_milestones.saturating_add(1);
                                self.narrator.say(format!(
                                    "*** Mastery {after_m} on {}! +1 skill point. ***",
                                    fish_ref.name
                                ));
                            } else {
                                self.narrator.say(format!(
                                    "*** Mastery {after_m} on {}! ***",
                                    fish_ref.name
                                ));
                            }
                        }
                    }
                    let before = self.skills.fishing_level();
                    self.skills.fishing_xp += gained;
                    let after = self.skills.fishing_level();
                    self.show_xp_gain("Fishing", gained, self.skills.fishing_xp, after);
                    if after > before {
                        self.narrator
                            .say(format!("Fishing level up! Now level {after}."));
                    }
                    self.check_encyclopedia_milestones();
                    if let Some(eff) = &fish_ref.effect {
                        if let Some((msg, kind)) = crate::buffs::apply_effect(&mut self.buffs, eff)
                        {
                            self.narrator.say(format!("*** {msg} ***"));
                            if let crate::buffs::EffectKind::FishingXp(xp) = kind {
                                let before2 = self.skills.fishing_level();
                                self.skills.fishing_xp += xp;
                                let after2 = self.skills.fishing_level();
                                self.show_xp_gain(
                                    "Fishing",
                                    xp,
                                    self.skills.fishing_xp,
                                    after2,
                                );
                                if after2 > before2 {
                                    self.narrator
                                        .say(format!("Fishing level up! Now level {after2}."));
                                }
                            }
                        }
                    }
                    self.quest_progress("catch", &fish_ref.name);
                    // also tick the generic "catch any fish" counter so
                    // quests like Shipwright's Hull (1250 catches) work
                    self.quest_progress_silent("catch", "any");
                    self.tick_bounty(&fish_ref.name);
                    if self.tutorial_step <= 3 {
                        self.tutorial_advance(3);
                    }
                } else if escaped {
                    self.narrator
                        .say("It slipped the line. You'll never know what.".to_string());
                    self.stats.fish_escaped += 1;
                    if self.stats.catch_streak > 0 {
                        self.narrator.say(format!(
                            "Streak broken at {}.",
                            self.stats.catch_streak
                        ));
                        self.stats.catch_streak = 0;
                    }
                } else {
                    self.narrator.say("You leave the line slack and step away.");
                }
            }
            Scene::RodShop { .. } | Scene::FishingSchool { .. } => {
                self.narrator.say("You step back outside.");
            }
            _ => {}
        }
        self.pending_catch_loc = None;
        self.scene = Scene::Overworld;
    }

    fn handle_overworld(&mut self, code: KeyCode) {
        // Bare overworld keys are deliberately minimal: movement (wasd /
        // hjkl / arrows handled elsewhere), `f` to interact, space to cast,
        // and Esc to bail out of a cast. EVERYTHING else uses :commands.
        match code {
            KeyCode::Char('f') => {
                if self.cast.is_some() {
                    // f during cast = no-op so player doesn't restart
                } else {
                    self.interact_facing();
                }
            }
            KeyCode::Char('x') => self.inspect_surroundings(),
            KeyCode::Char(' ') => self.cast_action(),
            KeyCode::Esc if self.cast.is_some() => self.cancel_cast(),
            _ => {}
        }
    }

    fn inspect_surroundings(&mut self) {
        let (dx, dy) = self.player.facing;
        let tx = self.player.x + dx;
        let ty = self.player.y + dy;
        if self.faceless.iter().any(|&(x, y)| x == tx && y == ty) {
            self.narrator
                .say(crate::inspect_text::get("faceless:inspect"));
            return;
        }
        if let Some(npc) = npc::npc_at_dim(tx, ty, self.world.dim, self.world.seed) {
            self.narrator
                .say(format!("{}: press f to talk.", npc.name));
            return;
        }
        let t = self.world.get(tx, ty);
        // Inspect-to-board: if the player has a boat and the inspected
        // tile is open water, step in and become the boat. Otherwise just
        // describe what's there — every tile has a describe() entry.
        if is_boatable(t) && self.player.has_boat && !self.player.on_boat {
            self.player.on_boat = true;
            self.player.x = tx;
            self.player.y = ty;
            self.narrator.say("You push off, riding the boat.");
            self.check_biome_change();
            self.mark_seen_around_player();
            return;
        }
        if matches!(t, Tile::Curio) {
            if let Some((entry, _idx)) = crate::world::curio_at(tx, ty, self.world.dim, self.world.seed) {
                let key = format!("curio:{}", entry.0);
                self.narrator.say(crate::inspect_text::get(&key));
                return;
            }
        }
        self.narrator.say(t.describe());
    }

    fn step(&mut self, dx: i32, dy: i32) {
        self.player.facing = (dx, dy);
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        if self.faceless.iter().any(|&(x, y)| x == nx && y == ny) {
            return; // faceless block; talk with f to engage
        }
        if npc::npc_at_dim(nx, ny, self.world.dim, self.world.seed).is_some() {
            return; // blocked by NPC; press f to interact
        }
        let t = self.world.get(nx, ny);
        let walkable = t.walkable() || (self.player.on_boat && is_boatable(t));
        if !walkable {
            return;
        }
        // Hull-tier depth gate: the boat won't push past the depth your
        // hull is rated for. Each tier opens deeper water; tier 6 reaches
        // the Fog Sea. Stepping back toward shore is always allowed.
        if self.player.on_boat
            && matches!(self.world.dim, crate::world::Dimension::Surface)
            && is_boatable(t)
            && !matches!(t, Tile::Dock)
        {
            let depth = ocean_depth_at(&self.world, nx, ny);
            let limit = crate::player::ocean_depth_max(self.player.hull_tier);
            if depth > limit {
                self.narrator.say(format!(
                    "The {} won't take open water this deep ({} > {}). Upgrade the hull.",
                    crate::player::hull_label(self.player.hull_tier),
                    depth,
                    limit,
                ));
                return;
            }
        }
        self.player.x = nx;
        self.player.y = ny;
        // Stepping onto a non-water tile dismounts the boat.
        if self.player.on_boat && !is_boatable(t) {
            self.player.on_boat = false;
            self.narrator.say("You step ashore, leaving the boat behind.");
        }
        // Biofuel drains 1 per step while aboard. Hitting empty dumps the
        // player back at the home pier — single-coord teleport (0, 5) at
        // the top of the dock — and disembarks. The crew's hunger stays as
        // is, so a long run back to land is still a punishment.
        if self.player.on_boat && self.player.hull_tier > 0 {
            self.player.biofuel = self.player.biofuel.saturating_sub(1);
            if self.player.biofuel == 0 {
                self.player.x = 0;
                self.player.y = 5;
                self.player.on_boat = false;
                self.narrator.say(
                    "*** Engine sputters dry. The boat drifts home empty. You wake on the pier. ***"
                        .to_string(),
                );
            }
        }
        self.check_biome_change();
        self.mark_seen_around_player();
        let weight: u64 = if dy != 0 { 2 } else { 1 };
        // Stamina drain is a random event roughly every 5..=20 steps
        // (Second Wind under 10% waives it). Vertical steps tick the
        // counter twice since they cover 2x as much ground.
        for _ in 0..weight {
            if self.steps_until_drain == 0 {
                self.reroll_steps_until_drain();
            }
            self.steps_until_drain = self.steps_until_drain.saturating_sub(1);
            if self.steps_until_drain == 0 {
                let drain = self.walk_event_drain();
                self.spend_stamina(drain);
                self.reroll_steps_until_drain();
            }
        }
        self.stats.steps += weight;
        if self.tutorial_step == 0 {
            self.tutorial_advance(0);
        } else if self.tutorial_step == 1 && self.player.y >= 8 {
            // close enough to the pier; nudge with the cast hint
            self.tutorial_advance(1);
        }
        let wxp = ((weight as f32) * self.skill_tree.global_xp_mult()) as u64;
        self.skills.walking_xp += wxp.max(weight);
        for _ in 0..weight {
            self.quest_progress_silent("walk", "any");
            if let Some(b) = self.current_biome {
                self.quest_progress_silent("walk", b.label());
            }
        }
    }

    fn pickup_here(&mut self) {
        self.spend_stamina(0.2);
        let candidates = [(0, 0), (0, -1), (0, 1), (-1, 0), (1, 0)];
        for (dx, dy) in candidates {
            let (tx, ty) = (self.player.x + dx, self.player.y + dy);
            let t = self.world.get(tx, ty);
            let item = match t {
                Tile::Flower => Some(Item {
                    name: "wildflower".into(),
                    category: Category::Plant,
                    description: "A small, soft-petalled wildflower.".into(),
                }),
                Tile::Pebble => Some(Item {
                    name: "pebble".into(),
                    category: Category::Mineral,
                    description: "A smooth pebble worth nothing in particular.".into(),
                }),
                _ => None,
            };
            if let Some(it) = item {
                self.narrator.say(format!("You pick up a {}.", it.name));
                self.player.items.push(it);
                self.stats.items_picked += 1;
                return;
            }
        }
        self.narrator.say("Nothing to pick up here.");
    }

    fn interact_facing(&mut self) {
        let (dx, dy) = self.player.facing;
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        if self.try_interact_faceless(nx, ny) {
            return;
        }
        if self.try_catch_bug_at(nx, ny) {
            return;
        }
        if self.try_dig_soil_at(nx, ny) {
            return;
        }
        if self.try_forage_at(nx, ny) {
            return;
        }
        if let Some(npc) = npc::npc_at_dim(nx, ny, self.world.dim, self.world.seed) {
            if npc.id == "sailor" {
                self.interact_sailor();
                return;
            }
            if npc.id == "fishmonger" || npc.id == "tut-monger" {
                self.scene = Scene::Fishmonger {
                    cursor: 0,
                    step: FishmongerStep::PickFish,
                };
                self.mode = Mode::Insert;
                return;
            }
            if npc.id == "shipwright" {
                self.interact_shipwright();
                return;
            }
            if npc.id == "miner" {
                self.interact_miner();
                return;
            }
            if npc.id == "old-angler" {
                if self.interact_old_angler() {
                    return;
                }
                // fall through to generic dialogue otherwise
            }
            if npc.id == "blacksmith" || npc.id == "blacksmith-template" {
                self.scene = Scene::Blacksmith { cursor: 0 };
                self.mode = Mode::Insert;
                return;
            }
            if npc.id == "fishmonger-template" {
                self.scene = Scene::Fishmonger {
                    cursor: 0,
                    step: FishmongerStep::PickFish,
                };
                self.mode = Mode::Insert;
                return;
            }
            self.narrator.say(format!("You greet {}.", npc.name));
            let id = npc.id.clone();
            self.scene = Scene::Dialogue { npc, line: 0 };
            self.quest_progress("talk", &id);
            self.stats.npcs_talked += 1;
            return;
        }
        match self.world.get(nx, ny) {
            Tile::DoorRod => {
                self.narrator.say("You step into the rod shop.");
                let last = (crate::rod::rods().len() as u32).saturating_sub(1);
                let cursor = self.player.rods.max_owned.min(last);
                self.scene = Scene::RodShop { cursor };
                self.mode = Mode::Insert;
            }
            Tile::DoorSchool => {
                self.narrator.say("You step into the fishing school.");
                self.scene = Scene::FishingSchool { cursor: 0, tab: 0 };
            }
            Tile::DimPortal => {
                if let Some(dest) =
                    crate::world::dim_portal_for(nx, ny, self.world.seed)
                {
                    let gate = dest.min_rod_tier();
                    if self.player.rods.max_owned < gate {
                        self.narrator.say(format!(
                            "{}: rod tier {gate} required. (You have tier {}.)",
                            dest.label(),
                            self.player.rods.max_owned
                        ));
                    } else {
                        self.world.dim = dest;
                        self.snap_player_to_walkable();
                        self.quest_progress("visit_dim", dest.label());
                        self.narrator
                            .say(format!("You arrive at: {}.", dest.label()));
                    }
                }
            }
            Tile::MineEntrance => {
                // Two flavours of entrance share the MineEntrance tile:
                // dry mineshafts (-> Mines, rod gate 3) and lakebed A-frames
                // on lake islands (-> Lakebed, rod gate 25). The lakebed
                // anchor check tells them apart.
                let is_lakebed = crate::world::is_lakebed_entrance_anchor(
                    nx, ny, self.world.seed,
                );
                if is_lakebed {
                    let gate = crate::world::Dimension::Lakebed.min_rod_tier();
                    if self.player.rods.max_owned < gate {
                        self.narrator.say(format!(
                            "The flooded shaft swallows the line. You need rod tier {gate} to descend safely."
                        ));
                        return;
                    }
                    self.world.dim = crate::world::Dimension::Lakebed;
                    self.quest_progress("visit_dim", "Lakebed Caves");
                    self.narrator
                        .say("You drop through the A-frame. The world goes blue and still.");
                } else {
                    const MINES_ROD_GATE: u32 = 3;
                    if self.player.rods.max_owned < MINES_ROD_GATE {
                        self.narrator.say(format!(
                            "The shaft groans. You need rod tier {MINES_ROD_GATE} to risk going down."
                        ));
                        return;
                    }
                    self.world.dim = crate::world::Dimension::Mines;
                    self.visited_mines = true;
                    self.quest_progress("visit_dim", "Mines");
                    self.narrator
                        .say("You descend the mineshaft. The light dies behind you.");
                }
            }
            Tile::MineExit => {
                self.world.dim = crate::world::Dimension::Surface;
                self.narrator.say("You climb back up to Sentinel's air.");
            }
            Tile::DoorHouse => {
                let seed = ((nx as u32).wrapping_mul(0x9E37_79B1))
                    .wrapping_add((ny as u32).wrapping_mul(0x85EB_CA77))
                    ^ 0xD1B5_4A32;
                self.narrator.say("You step inside.");
                // Interior layout uses the canonical 18x10 room defined in
                // house.rs; spawn at the exit door (bottom-center).
                self.scene = Scene::HouseInterior {
                    px: crate::house::EXIT_X,
                    py: crate::house::EXIT_Y - 1,
                    return_xy: (self.player.x, self.player.y + 1),
                    seed,
                };
            }
            Tile::Smelter => {
                if self.smeltable_ores().is_empty() {
                    self.narrator
                        .say("The smelter's hot, but you have nothing to smelt.");
                } else {
                    self.scene = Scene::Smelt { cursor: 0, typed: String::new() };
                    self.mode = Mode::Insert;
                }
            }
            Tile::Forge => {
                if self.forgeable_gear().is_empty() {
                    self.narrator
                        .say("The forge is warm, but you have no recipe you can complete.");
                } else {
                    self.scene = Scene::Forge { cursor: 0, typed: String::new() };
                    self.mode = Mode::Insert;
                }
            }
            Tile::CookingPot => {
                // Same flow as `:cook` / `:cookbook`: open the recipe
                // encyclopedia with empty filter, cursor at top.
                self.scene = Scene::Cooking {
                    cursor: 0,
                    filter: String::new(),
                    editing_filter: false,
                };
                self.mode = Mode::Insert;
                self.tutorial_advance(6);
            }
            Tile::BaitBench => {
                self.scene = Scene::LureBench { cursor: 0 };
                self.mode = Mode::Insert;
            }
            Tile::OreRock => {
                if !self.player.has_pickaxe {
                    self.narrator
                        .say("You'd need a pickaxe. Find the Miner east of the village.");
                    return;
                }
                let key = (self.world.dim, nx, ny);
                let ore = crate::mining::ore_at_vein(nx, ny, self.world.dim, self.world.seed);
                let pt = self.player.pickaxe_tier();
                if pt < ore.min_pickaxe_tier {
                    let needed = crate::gear::defs()
                        .iter()
                        .filter(|d| {
                            d.slot_enum() == Some(crate::gear::Slot::Pickaxe)
                                && d.perks.pickaxe_tier == ore.min_pickaxe_tier
                        })
                        .map(|d| d.name.clone())
                        .next()
                        .unwrap_or_else(|| format!("tier-{}", ore.min_pickaxe_tier));
                    self.narrator.say(format!(
                        "This {} vein needs a {} (you have tier {}). Forge one at the blacksmith.",
                        ore.name, needed, pt,
                    ));
                    return;
                }
                match crate::mining::vein_status(&self.veins, key) {
                    crate::mining::VeinStatus::OnCooldown(secs_left) => {
                        let mins = (secs_left + 59) / 60;
                        self.narrator
                            .say(format!("This vein is resting. ~{mins}m left."));
                        return;
                    }
                    crate::mining::VeinStatus::Ready => {}
                }
                let mut m = crate::mining::Mining::new(nx, ny, self.world.dim, ore);
                // Ring perk: prewrite N letters of the ore's name so the
                // typing minigame starts partially done.
                let prewrite = self.player.gear.combined_perks().ore_prewrite_letters as usize;
                if prewrite > 0 {
                    for c in ore.name.chars().take(prewrite) {
                        m.typed.push(c);
                    }
                }
                self.scene = Scene::Mining(m);
            }
            Tile::Dock
            | Tile::Water
            | Tile::Well
            | Tile::MineralWater
            | Tile::DeepWater
            | Tile::Lava => {
                // Crew won't haul a line while they're starving. Cast is
                // blocked until the player feeds them (-3 hunger per fish
                // via `:feed`). Land fishing is unaffected.
                if self.player.on_boat && self.player.crew_hunger >= 100 {
                    self.narrator.say(
                        "The crew refuses to row. They need feeding (:feed <n>) first."
                            .to_string(),
                    );
                    return;
                }
                // Wells unlock the inferno: at 100 lifetime well casts, the
                // next interaction with a well drops you into the inferno
                // instead of fishing.
                if matches!(self.world.get(nx, ny), Tile::Well)
                    && self.world.dim == crate::world::Dimension::Surface
                {
                    self.stats.well_casts = self.stats.well_casts.saturating_add(1);
                    self.quest_progress_silent("well_cast", "any");
                    // Only the *first* time well_casts crosses 100 do we
                    // teleport. Subsequent well casts still fish normally.
                    const INFERNO_ROD_GATE: u32 = 75;
                    if self.stats.well_casts == 100
                        && self.player.rods.max_owned < INFERNO_ROD_GATE
                    {
                        self.narrator.say(format!(
                            "The well shudders, but spits you back out. Need rod tier {INFERNO_ROD_GATE} to survive the fall."
                        ));
                        // Don't open the portal yet; well_casts can keep ticking.
                    } else if self.stats.well_casts == 100 {
                        self.narrator
                            .say("*** The well's bottom opens. You fall into the Inferno. ***");
                        self.world.dim = crate::world::Dimension::Inferno;
                        self.visited_inferno = true;
                        self.quest_progress("visit_dim", "Inferno");
                        self.player.x = 0;
                        self.player.y = 7;
                        self.narrator
                            .say("To the north: the Fallen Fish waits in his keep.");
                        return;
                    }
                }
                let (water_kind, biome) = fishing_context(&self.world, nx, ny);
                let weather = self.current_weather();
                let dim_pool = dim_default_pool(
                    self.world.dim,
                    self.world.get(nx, ny),
                    weather,
                    &mut self.rng_state,
                    (nx, ny),
                    self.world.seed,
                );
                // Fog Sea is a fishing_context-derived label, not a dim,
                // so it bypasses dim_default_pool. Route it explicitly to
                // the ghost-fish pool here.
                let fog_pool = if biome == "Fog Sea" {
                    Some("ghost".to_string())
                } else {
                    None
                };
                let pool_override = self
                    .current_pool_override
                    .clone()
                    .or(fog_pool)
                    .or_else(|| dim_pool.map(|s| s.to_string()));
                // Necklace perk: chance the bait is *not* consumed this cast.
                // Resolve before we touch the stock so the active bait still
                // applies its effect this cast.
                let nb_chance = self.player.gear.combined_perks().no_bait_consume_chance;
                let skip_bait = nb_chance > 0.0
                    && crate::fish::next_rand_f32(&mut self.rng_state) < nb_chance;
                let bait_effect_peek = self
                    .player
                    .bait
                    .active
                    .as_ref()
                    .and_then(|id| crate::bait::def_by_id(id))
                    .map(|b| (b.effect.clone(), b.magnitude, b.name.clone()));
                // Snapshot the active bait's secondary axes (bite_speed,
                // pool_pull) before consume, so the necklace skip-bait branch
                // keeps them too.
                let (bait_bite_speed, bait_pool_pull) = {
                    if let Some(active_id) = self.player.bait.active.clone() {
                        if let Some(d) = crate::bait::def_by_id(&active_id) {
                            let pp = if !d.pool_pull.is_empty() && d.pool_pull_mult > 0.0 {
                                Some((d.pool_pull.clone(), d.pool_pull_mult))
                            } else {
                                None
                            };
                            (d.bite_speed, pp)
                        } else {
                            (0.0, None)
                        }
                    } else {
                        (0.0, None)
                    }
                };
                let (bait_used, bait_effect) = if skip_bait {
                    if let Some((_, _, name)) = &bait_effect_peek {
                        self.narrator
                            .say(format!("Necklace pulses — your {name} survives the cast."));
                    }
                    let eff = bait_effect_peek
                        .as_ref()
                        .map(|(eff, mag, _)| (eff.clone(), *mag));
                    (None, eff)
                } else {
                    let used = self.player.bait.consume_active();
                    let eff = used.map(|b| (b.effect.clone(), b.magnitude));
                    (used, eff)
                };
                if let Some(b) = bait_used {
                    self.narrator.say(format!("Bait: {} consumed.", b.name));
                }
                self.bait_pending_bite_speed = bait_bite_speed;
                self.bait_pending_pool_pull = bait_pool_pull;
                // Combo-chain bait: arms a 3-cast catch_speed buff. Any
                // existing chain is overwritten (the new bait dictates the
                // magnitude).
                if let Some((e, m)) = &bait_effect {
                    if e == "combo_chain" {
                        self.buffs.combo_chain_left = 3;
                        self.buffs.combo_chain_mult = *m;
                        self.narrator
                            .say(format!("Combo chain armed: +{:.0}% catch speed for 3 casts.", m * 100.0));
                    }
                }
                let rare_boost = bait_effect
                    .as_ref()
                    .map(|(e, _)| e == "rare_chance")
                    .unwrap_or(false);
                self.bait_pending = bait_effect;
                let weather_now = self.current_weather();
                let weather_rare = crate::weather::weather_modifiers(weather_now).rare_pct > 0.0;
                let rare_window =
                    crate::gametime::time_of_day(self.total_play_secs()).is_rare_window()
                        || rare_boost
                        || weather_rare;
                let weather_name = weather.value();
                let depth = if water_kind == "ocean" {
                    ocean_depth_at(&self.world, nx, ny)
                } else {
                    0
                };
                let bait_pool_pull = self
                    .bait_pending_pool_pull
                    .as_ref()
                    .map(|(tag, mult)| (tag.as_str(), *mult));
                let f = crate::fish::pick_fish_full(
                    &mut self.rng_state,
                    fishlist::fish(),
                    &biome,
                    water_kind,
                    pool_override.as_deref(),
                    rare_window,
                    Some(weather_name),
                    self.stats.fish_caught,
                    self.skills.fishing_level(),
                    self.player.rods.max_owned,
                    depth,
                    bait_pool_pull,
                    false,
                );
                self.narrator.say("Casting line - aim for the green!");
                self.stats.casts += 1;
                self.cast = Some(CastState {
                    phase: CastPhase::Casting,
                    fish: f,
                    bobber: (nx, ny),
                    cast_pos: 0.0,
                    cast_vel: 0.10,
                    cast_strength: 0.0,
                    wait_ticks_left: 0,
                    bite_ticks_left: 0,
                    hotspot: false,
                });
            }
            _ => {
                // The Rod + a chosen pool: fish on any tile (Grass, CaveFloor,
                // even dry dirt). The rod bends reality.
                if self.player.rods.equipped >= 202
                    && self.current_pool_override.is_some()
                {
                    let pool_override = self.current_pool_override.clone();
                    let (water_kind, biome) = fishing_context(&self.world, nx, ny);
                    let rare_window =
                        crate::gametime::time_of_day(self.total_play_secs()).is_rare_window();
                    let w = self.current_weather();
                    let f = crate::fish::pick_fish_full(
                        &mut self.rng_state,
                        fishlist::fish(),
                        &biome,
                        water_kind,
                        pool_override.as_deref(),
                        rare_window,
                        Some(w.value()),
                        self.stats.fish_caught,
                        self.skills.fishing_level(),
                        self.player.rods.max_owned,
                        0,
                        None,
                        false,
                    );
                    self.narrator
                        .say(format!("THE ROD pierces reality. Pool: {}.", pool_override.as_deref().unwrap_or("?")));
                    self.stats.casts += 1;
                    self.cast = Some(CastState {
                        phase: CastPhase::Casting,
                        fish: f,
                        bobber: (nx, ny),
                        cast_pos: 0.0,
                        cast_vel: 0.10,
                        cast_strength: 0.0,
                        wait_ticks_left: 0,
                        bite_ticks_left: 0,
                        hotspot: false,
                    });
                } else {
                    self.narrator.say("Nothing to interact with.");
                }
            }
        }
    }

    /// Drive the Fishmonger menu. Takes ownership of the prior (cursor,
    /// step) and returns the next (cursor, Option<step>) — None means the
    /// caller should restore the Overworld scene.
    fn handle_fishmonger(
        &mut self,
        code: KeyCode,
        kind: KeyEventKind,
        mut cursor: usize,
        step: FishmongerStep,
    ) -> (usize, Option<FishmongerStep>) {
        let grouped = self.fishmonger_listing();
        match step {
            FishmongerStep::PickFish => match code {
                KeyCode::Esc | KeyCode::Char('q') => (cursor, None),
                KeyCode::Char('j') | KeyCode::Down => {
                    if !grouped.is_empty() {
                        cursor = (cursor + 1).min(grouped.len() - 1);
                    }
                    (cursor, Some(FishmongerStep::PickFish))
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    (cursor, Some(FishmongerStep::PickFish))
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some((name, _price, count)) = grouped.get(cursor) {
                        (
                            0,
                            Some(FishmongerStep::PickQuantity {
                                picked: name.clone(),
                                max: *count,
                            }),
                        )
                    } else {
                        (cursor, Some(FishmongerStep::PickFish))
                    }
                }
                _ => (cursor, Some(FishmongerStep::PickFish)),
            },
            FishmongerStep::PickQuantity { picked, max } => match code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    (cursor, Some(FishmongerStep::PickFish))
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    cursor = (cursor + 1).min(3);
                    (cursor, Some(FishmongerStep::PickQuantity { picked, max }))
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    (cursor, Some(FishmongerStep::PickQuantity { picked, max }))
                }
                KeyCode::Enter | KeyCode::Char(' ') => match cursor {
                    // Order matches the user-facing menu: ALL / ONE / X / Quit.
                    0 => {
                        let total = self.fishmonger_quote(&picked, max);
                        (0, Some(FishmongerStep::Confirm { picked, qty: max, total }))
                    }
                    1 => {
                        let total = self.fishmonger_quote(&picked, 1);
                        (0, Some(FishmongerStep::Confirm { picked, qty: 1, total }))
                    }
                    2 => (
                        0,
                        Some(FishmongerStep::EnterQuantity {
                            picked,
                            max,
                            buf: String::new(),
                        }),
                    ),
                    _ => (cursor, Some(FishmongerStep::PickFish)),
                },
                _ => (cursor, Some(FishmongerStep::PickQuantity { picked, max })),
            },
            FishmongerStep::EnterQuantity { picked, max, mut buf } => match code {
                KeyCode::Esc => (0, Some(FishmongerStep::PickQuantity { picked, max })),
                KeyCode::Backspace => {
                    buf.pop();
                    (cursor, Some(FishmongerStep::EnterQuantity { picked, max, buf }))
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    if matches!(kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && buf.len() < 9
                    {
                        buf.push(c);
                    }
                    (cursor, Some(FishmongerStep::EnterQuantity { picked, max, buf }))
                }
                KeyCode::Enter => {
                    let n: u64 = buf.parse().unwrap_or(0);
                    let n = n.min(max);
                    if n == 0 {
                        return (cursor, Some(FishmongerStep::PickFish));
                    }
                    let total = self.fishmonger_quote(&picked, n);
                    (0, Some(FishmongerStep::Confirm { picked, qty: n, total }))
                }
                _ => (cursor, Some(FishmongerStep::EnterQuantity { picked, max, buf })),
            },
            FishmongerStep::Confirm { picked, qty, total } => match code {
                KeyCode::Char('y') | KeyCode::Enter | KeyCode::Char(' ') => {
                    self.sell_fish_by_name(&picked, qty);
                    (0, Some(FishmongerStep::PickFish))
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                    (0, Some(FishmongerStep::PickFish))
                }
                _ => (cursor, Some(FishmongerStep::Confirm { picked, qty, total })),
            },
        }
    }

    /// Group the (non-unique) basket by species name. Returns
    /// (name, unit_price_with_buff, count). Stable ordering matches inv.
    // -------- faceless wanderers (Mines only) -----------------------------

    /// Number of faceless figures kept in the player's vision in the Mines.
    const FACELESS_COUNT: usize = 3;
    fn ensure_faceless_spawned(&mut self) {
        if self.world.dim != crate::world::Dimension::Mines {
            self.faceless.clear();
            return;
        }
        while self.faceless.len() < Self::FACELESS_COUNT {
            // Spawn close enough to be on-screen — not arm's reach but well
            // within the viewport — so there are always 2-3 visible.
            if let Some(p) = self.random_walkable_near(self.player.x, self.player.y, 5, 14) {
                self.faceless.push(p);
            } else {
                break;
            }
        }
    }

    fn random_walkable_near(
        &mut self,
        cx: i32,
        cy: i32,
        min_r: i32,
        max_r: i32,
    ) -> Option<(i32, i32)> {
        for _ in 0..32 {
            let r = crate::fish::next_rand_f32(&mut self.rng_state);
            let dx = ((r * (2.0 * max_r as f32 + 1.0)) as i32) - max_r;
            let r2 = crate::fish::next_rand_f32(&mut self.rng_state);
            let dy = ((r2 * (2.0 * max_r as f32 + 1.0)) as i32) - max_r;
            if dx.abs() < min_r && dy.abs() < min_r {
                continue;
            }
            let x = cx + dx;
            let y = cy + dy;
            if self.world.get(x, y).walkable() {
                return Some((x, y));
            }
        }
        None
    }

    fn tick_faceless(&mut self) {
        if self.world.dim != crate::world::Dimension::Mines {
            self.faceless.clear();
            return;
        }
        self.ensure_faceless_spawned();
        // Re-anchor any wanderer that drifted too far from the player.
        let px = self.player.x;
        let py = self.player.y;
        for i in 0..self.faceless.len() {
            let (x, y) = self.faceless[i];
            let far = (x - px).abs() > 18 || (y - py).abs() > 12;
            if far {
                if let Some(p) = self.random_walkable_near(px, py, 5, 14) {
                    self.faceless[i] = p;
                }
                continue;
            }
            // 60% chance to wander one tile per tick (called every 20 ticks).
            let r = crate::fish::next_rand_f32(&mut self.rng_state);
            if r > 0.60 {
                continue;
            }
            let dir = crate::fish::next_rand_f32(&mut self.rng_state);
            let (dx, dy) = if dir < 0.25 {
                (-1, 0)
            } else if dir < 0.50 {
                (1, 0)
            } else if dir < 0.75 {
                (0, -1)
            } else {
                (0, 1)
            };
            let nx = x + dx;
            let ny = y + dy;
            if self.world.get(nx, ny).walkable() {
                self.faceless[i] = (nx, ny);
            }
        }
    }

    fn try_interact_faceless(&mut self, nx: i32, ny: i32) -> bool {
        if self.world.dim != crate::world::Dimension::Mines {
            return false;
        }
        let idx = self.faceless.iter().position(|&(x, y)| x == nx && y == ny);
        let Some(idx) = idx else { return false };
        // Remove this faceless on encounter — they only appear once each.
        self.faceless.swap_remove(idx);
        let roll = crate::fish::next_rand_f32(&mut self.rng_state);
        if roll < 0.25 {
            // Blessing: +25% mining xp for 60 seconds. The friendly one
            // turns out to have a name — generated on the spot.
            self.mining_boost_until = crate::mining::now_secs() + 60;
            let name = random_friendly_name(&mut self.rng_state);
            let line = crate::inspect_text::get("faceless:blessing").replace("{name}", &name);
            self.narrator.say(line);
            self.narrator
                .say("(Mining XP +25% for 1 minute.)");
        } else {
            // Curse: spam the line and quit in ~3 seconds.
            for _ in 0..20 {
                self.narrator
                    .say(crate::inspect_text::get("faceless:curse"));
            }
            // ~3 seconds at 20fps = 60 ticks
            self.pending_quit_at = Some(self.anim_tick + 60);
        }
        true
    }

    fn tick_streak(&mut self, name: &str) {
        let new = match &self.streak_species {
            Some(prev) if prev == name => self.streak_count.saturating_add(1),
            _ => 1,
        };
        self.streak_species = Some(name.to_string());
        self.streak_count = new;
    }

    fn complete_challenge(&mut self, ch: &crate::mastery_challenges::Challenge) {
        if self.challenge_done.contains(&ch.id) {
            return;
        }
        self.challenge_done.push(ch.id.clone());
        self.challenge_bonus_points =
            self.challenge_bonus_points.saturating_add(ch.reward_points);
        self.narrator.say(format!(
            "*** {} (+{} skill point{}). ***",
            ch.title,
            ch.reward_points,
            if ch.reward_points == 1 { "" } else { "s" }
        ));
    }

    fn tick_mastery_challenges_catch(
        &mut self,
        name: &str,
        size: crate::mastery_challenges::SizeClass,
    ) {
        use crate::mastery_challenges::ChallengeKind::*;
        let chs = crate::mastery_challenges::challenges_for_name(name).to_vec();
        let streak = self.streak_count;
        for ch in chs {
            if self.challenge_done.contains(&ch.id) {
                continue;
            }
            let should_tick = match ch.kind {
                CatchLarge => matches!(size, crate::mastery_challenges::SizeClass::Large),
                CatchHuge => matches!(size, crate::mastery_challenges::SizeClass::Huge),
                Streak | BulkSale => false,
            };
            if should_tick {
                let entry = self.challenge_progress.entry(ch.id.clone()).or_insert(0);
                *entry = entry.saturating_add(1);
                if *entry >= ch.target {
                    self.complete_challenge(&ch);
                }
            }
            // Streak: completion test uses live streak count.
            if matches!(ch.kind, Streak) && streak >= ch.target {
                self.complete_challenge(&ch);
            }
        }
    }

    fn tick_mastery_challenges_sale(&mut self, name: &str, sold: u64) {
        use crate::mastery_challenges::ChallengeKind::*;
        let chs = crate::mastery_challenges::challenges_for_name(name).to_vec();
        for ch in chs {
            if !matches!(ch.kind, BulkSale) {
                continue;
            }
            if sold >= ch.target as u64 {
                self.complete_challenge(&ch);
            }
        }
    }

    /// Valu granted per unlocked achievement on each in-game month rollover.
    /// Tunable. With ~50 achievements you'd get 12,500$V/month at full
    /// unlock — meaningful stipend at endgame, modest reward early.
    const CAPE_VALU_PER_ACHIEVEMENT: u64 = 250;

    /// Synthetic cape "id" tracks the unlocked-achievement count. The cape
    /// is auto-equipped and unforgeable; the EquippedGear.cape slot stores
    /// this id so the inventory panel can render "Cape of N Memories".
    /// def_by_id lookups return None for capes — they don't compose with
    /// the standard gear perks.
    fn sync_cape(&mut self) {
        let n = self.achievements.unlocked.len();
        if n == 0 {
            self.player.gear.cape = None;
            return;
        }
        let id = format!("cape-of-{n}-memories");
        if self.player.gear.cape.as_deref() != Some(id.as_str()) {
            self.player.gear.cape = Some(id);
        }
    }

    /// Current in-game month id (monotonic across years).
    fn current_month_id(&self) -> u64 {
        let secs = self.total_play_secs();
        let m = crate::gametime::month_of_year(secs) as u64;
        let y = crate::gametime::year(secs);
        y * crate::gametime::MONTHS_PER_YEAR + m
    }

    fn current_day_id(&self) -> u64 {
        crate::gametime::game_days(self.total_play_secs())
    }

    /// Pay the cape stipend on the 1st of every in-game month. Skips the
    /// first observed month (sets baseline) so loading an old save doesn't
    /// retroactively dump a fat lump-sum.
    fn tick_cape_payout(&mut self) {
        let cur = self.current_month_id();
        if self.last_cape_payout_month == 0 {
            // Baseline: bump to (cur + 1) so the *next* month rollover triggers
            // the first payout — players get a clean monthly cadence.
            self.last_cape_payout_month = cur + 1;
            return;
        }
        if cur >= self.last_cape_payout_month {
            let cape_lv = self.achievements.unlocked.len() as u64;
            let pay = cape_lv.saturating_mul(Self::CAPE_VALU_PER_ACHIEVEMENT);
            if pay > 0 {
                self.player.valu = self.player.valu.saturating_add(pay);
                self.lifetime_valu = self.lifetime_valu.saturating_add(pay);
                self.narrator.say(format!(
                    "*** The month turns. Your cape stirs; {pay}$V flutter from its hem. ({cape_lv} memories.) ***"
                ));
            }
            self.last_cape_payout_month = cur + 1;
        }
    }

    /// Reset per-day merchant counters when the in-game day flips.
    fn tick_market_day_rollover(&mut self) {
        let cur = self.current_day_id();
        if cur != self.last_market_day {
            self.fish_sold_today = 0;
            self.ore_sold_today = 0;
            self.last_market_day = cur;
        }
    }

    /// Per-day quantity a fishmonger will buy. Roughly calibrated against
    /// the fishing loop: ~25s per catch, 2/3 of an in-game day continuous
    /// is ~64 fish at peak. Scaled by Fishing level so newer players are
    /// rate-bounded and endgame players can dump big hauls.
    ///
    /// Curve: `base + level^0.85 * scale`
    ///   lv  1 →  22
    ///   lv 10 →  38
    ///   lv 25 →  60
    ///   lv 50 →  90
    ///   lv100 → 145
    pub fn fishmonger_daily_cap(&self) -> u32 {
        let lv = self.skills.fishing_level() as f32;
        (20.0 + lv.powf(0.85) * 2.5) as u32
    }

    /// Per-day quantity a blacksmith merchant will buy (raw ore + ingots
    /// combined). Mining loop is ~4s/ore so a 2/3-day grind tops ~400 raw
    /// per peak day. Scaled by Blacksmithing level.
    ///
    /// Curve: `60 + level^0.85 * 7`
    ///   lv  1 →  67
    ///   lv 10 → 109
    ///   lv 25 → 174
    ///   lv 50 → 261
    ///   lv100 → 422
    pub fn blacksmith_daily_cap(&self) -> u32 {
        let lv = self.skills.blacksmithing_level() as f32;
        (60.0 + lv.powf(0.85) * 7.0) as u32
    }

    /// Dump every raw ore + ingot to the blacksmith merchant, up to today's
    /// quantity cap. Raw ore sells at `ore.value`, ingots at `ore.ingot_value()`.
    /// Each item counts as 1 against the cap regardless of value, so the cap
    /// represents merchant attention/throughput rather than wallet size.
    fn sell_ore_and_ingots(&mut self) {
        self.tick_market_day_rollover();
        let cap = self.blacksmith_daily_cap();
        let mut remaining = cap.saturating_sub(self.ore_sold_today);
        if remaining == 0 {
            self.narrator.say(format!(
                "Blacksmith: \"Cart's full for today — bring more tomorrow. (cap {cap}/day at your Blacksmithing level.)\""
            ));
            return;
        }
        let mult = self.buffs.price_mult() * self.skill_tree.valu_mult();
        let mut total = 0u64;
        let mut sold_count = 0u32;
        // Raw ore first (lower value per unit so player gets best burn rate)
        let mut keep: Vec<crate::item::Item> = Vec::with_capacity(self.player.items.len());
        for it in self.player.items.drain(..).collect::<Vec<_>>() {
            if remaining == 0 {
                keep.push(it);
                continue;
            }
            if matches!(it.category, crate::item::Category::Mineral) {
                if let Some(ore) = crate::mining::ore_by_name(&it.name) {
                    let price = (ore.value as f32 * mult).round() as u64;
                    total = total.saturating_add(price);
                    sold_count += 1;
                    remaining -= 1;
                    continue;
                }
            }
            keep.push(it);
        }
        self.player.items = keep;
        // Then ingots.
        let ingot_keys: Vec<String> = self.player.ingots.keys().cloned().collect();
        for k in ingot_keys {
            if remaining == 0 {
                break;
            }
            let Some(ore) = crate::mining::ore_by_name(&k) else { continue };
            let have = self.player.ingots.get(&k).copied().unwrap_or(0);
            let n = have.min(remaining);
            if n == 0 {
                continue;
            }
            let per = (ore.ingot_value() as f32 * mult).round() as u64;
            total = total.saturating_add(per.saturating_mul(n as u64));
            sold_count = sold_count.saturating_add(n);
            remaining -= n;
            if let Some(e) = self.player.ingots.get_mut(&k) {
                *e -= n;
            }
        }
        self.player.ingots.retain(|_, v| *v > 0);
        if sold_count == 0 {
            self.narrator
                .say("Blacksmith: \"You haven't got a chip of ore. Come back when you do.\"");
            return;
        }
        self.player.valu = self.player.valu.saturating_add(total);
        self.lifetime_valu = self.lifetime_valu.saturating_add(total);
        self.stats.valu_earned = self.stats.valu_earned.saturating_add(total);
        self.ore_sold_today = self.ore_sold_today.saturating_add(sold_count);
        self.narrator.say(format!(
            "Sold {} ore/ingots to the smith for {}$V. ({}/{} today)",
            sold_count, total, self.ore_sold_today, cap,
        ));
    }

    /// True if any of the 8 cells around the player (Chebyshev distance 1)
    /// holds a Blacksmith NPC. Used to gate `:smelt`/`:forge`/`:sellore`
    /// commands so the loop happens at the forge, not anywhere.
    fn is_near_blacksmith(&self) -> bool {
        let (px, py) = (self.player.x, self.player.y);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                if let Some(n) =
                    crate::npc::npc_at_dim(px + dx, py + dy, self.world.dim, self.world.seed)
                {
                    if n.id == "blacksmith" || n.id == "blacksmith-template" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// True when the player stands on or adjacent (8-neighbourhood) to a
    /// CookingPot tile. Cooking requires this — you can't reduce sauce in
    /// open air.
    fn is_near_cooking_pot(&self) -> bool {
        let (px, py) = (self.player.x, self.player.y);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = self.world.get(px + dx, py + dy);
                if matches!(t, crate::world::Tile::CookingPot) {
                    return true;
                }
            }
        }
        false
    }

    fn check_achievements(&mut self) {
        let snap = crate::achievements::Snapshot {
            catch_total: self.stats.fish_caught,
            casts: self.stats.casts,
            steps: self.stats.steps,
            valu_earned: self.stats.valu_earned,
            fish_sold: self.stats.fish_sold,
            unique_species: self.caught.iter().filter(|c| **c).count() as u32,
            mastery_total: self.mastery_milestones,
            rod_tier: self.player.rods.max_owned,
            mining_level: self.skills.mining_level(),
            fishing_level: self.skills.fishing_level(),
            play_hours: self.total_play_secs() / 3600,
            has_pickaxe: self.player.has_pickaxe,
            has_boat: self.player.has_boat,
            visited_mines: self.visited_mines,
            visited_atlantis: self.visited_atlantis,
            visited_inferno: self.visited_inferno,
            recipes_cooked: self.cooking_mastery.iter().map(|&m| m as u64).sum(),
            recipes_mastered: self.cooking_mastery.iter().filter(|&&m| m >= 5).count() as u32,
            recipes_discovered: self.recipe_discovered.iter().filter(|d| **d).count() as u32,
            wood_chopped: self.stats.wood_chopped,
            trees_felled: self.stats.trees_felled,
            encyclopedia_level: self.skills.encyclopedia_level(),
            cooking_level: self.skills.cooking_level(),
            woodcutting_level: self.skills.woodcutting_level(),
            hull_tier: self.player.hull_tier,
            max_catch_streak: self.stats.max_catch_streak,
            shiny_catches: self.stats.shiny_catches,
            already_unlocked: &self.achievements.unlocked,
        };
        let new_unlocks = crate::achievements::newly_unlocked(&snap);
        for (chain_id, tier_idx, points, title) in new_unlocks {
            self.achievements
                .unlocked
                .push(crate::achievements::unlocked_id(&chain_id, tier_idx));
            self.achievements.points_granted =
                self.achievements.points_granted.saturating_add(points);
            let plural = if points == 1 { "" } else { "s" };
            self.narrator.say(format!(
                "*** Achievement: {} (+{} skill point{}). ***",
                title, points, plural
            ));
            self.push_banner(
                BannerKind::Achievement,
                format!("{title}   +{points} skill point{plural}"),
                100,
            );
        }
    }

    /// Compute the would-be payout for selling `count` of `name` right now,
    /// without performing the sale. Used by the confirmation prompt.
    fn fishmonger_quote(&self, name: &str, count: u64) -> u64 {
        if count == 0 {
            return 0;
        }
        let biome_label = self
            .current_biome
            .map(|b| b.label())
            .unwrap_or("Meadow");
        let biome_mult = self.skill_tree.biome_value_mult(biome_label);
        let archivist_bonus = self.skill_tree.archivist_per_dex()
            * (self.caught.iter().filter(|c| **c).count() as u64);
        let mult = self.buffs.price_mult()
            * self.skill_tree.valu_mult()
            * biome_mult
            * (1.0 + self.player.tackle.sum_effect("valu_mult"))
            * (1.0 + self.scales_bonus("valu_mult"))
            * (1.0 + self.landmark_bonus("valu_mult"));
        let mut sold = 0u64;
        let mut total = 0u64;
        for f in self.player.inventory.iter() {
            if !f.unique && f.name == name && sold < count {
                let mbonus = self.mastery_value_bonus(f);
                let lvl_decay = self.level_value_mult(f);
                let wmods = crate::weather::weather_modifiers(self.current_weather());
                let dim_bonus = self.dim_bonus_mult();
                let price = ((f.sell_price() as f32)
                    * mult
                    * (1.0 + mbonus + wmods.valu_pct)
                    * lvl_decay
                    * dim_bonus)
                    .round() as u64
                    + archivist_bonus;
                total = total.saturating_add(price);
                sold += 1;
            }
        }
        total
    }

    /// +2% sale value per 5 mastery on that species, capped at +50% (125).
    fn mastery_value_bonus(&self, fish_ref: &crate::fish::FishDef) -> f32 {
        let m = fishlist::fish()
            .iter()
            .position(|f| std::ptr::eq(f, fish_ref))
            .and_then(|i| self.mastery.get(i).copied())
            .unwrap_or(0) as f32;
        ((m / 5.0) * 0.02).min(0.5)
    }

    /// All Blue grants a permanent +10% to every multiplier (valu, xp,
    /// rare-chance) just for being in the dim. Other dims = 1.0.
    fn dim_bonus_mult(&self) -> f32 {
        if matches!(self.world.dim, crate::world::Dimension::AllBlue) {
            1.10
        } else {
            1.0
        }
    }

    /// Over-level decay: when the player's fishing level greatly exceeds
    /// the fish's min level, the species is "trivial" and pays less so
    /// that grinding low-tier prey can't substitute for engaging harder
    /// fish. Floor at 30%.
    fn level_value_mult(&self, fish_ref: &crate::fish::FishDef) -> f32 {
        let lvl = self.skills.fishing_level();
        let min = fish_ref.min_fishing_level();
        if lvl <= min {
            return 1.0;
        }
        let over = (lvl - min) as f32;
        (1.0 - 0.005 * over).max(0.30)
    }

    fn fishmonger_listing(&self) -> Vec<(String, u64, u64)> {
        let biome_label = self
            .current_biome
            .map(|b| b.label())
            .unwrap_or("Meadow");
        let biome_mult = self.skill_tree.biome_value_mult(biome_label);
        let archivist_bonus = self.skill_tree.archivist_per_dex()
            * (self.caught.iter().filter(|c| **c).count() as u64);
        let mult = self.buffs.price_mult()
            * self.skill_tree.valu_mult()
            * biome_mult
            * (1.0 + self.player.tackle.sum_effect("valu_mult"))
            * (1.0 + self.scales_bonus("valu_mult"))
            * (1.0 + self.landmark_bonus("valu_mult"));
        let mut out: Vec<(String, u64, u64)> = Vec::new();
        for f in self.player.inventory.iter().filter(|f| !f.unique) {
            let mbonus = self.mastery_value_bonus(f);
            let lvl_decay = self.level_value_mult(f);
            let price = ((f.sell_price() as f32) * mult * (1.0 + mbonus) * lvl_decay)
                .round() as u64
                + archivist_bonus;
            if let Some(entry) = out.iter_mut().find(|(n, _, _)| n == &f.name) {
                entry.2 += 1;
            } else {
                out.push((f.name.clone(), price, 1));
            }
        }
        out
    }

    /// Sell up to `count` fish of the given species name.
    fn sell_fish_by_name(&mut self, name: &str, count: u64) {
        if count == 0 {
            return;
        }
        // Daily fishmonger cap: only as many fish as the village's
        // remaining headroom for today.
        self.tick_market_day_rollover();
        let cap = self.fishmonger_daily_cap();
        let remaining = cap.saturating_sub(self.fish_sold_today) as u64;
        if remaining == 0 {
            self.narrator.say(format!(
                "Fishmonger: \"Stall's full for today — bring 'em back tomorrow. (cap {cap}/day at your Fishing level.)\""
            ));
            return;
        }
        let count = count.min(remaining);
        let biome_label = self
            .current_biome
            .map(|b| b.label())
            .unwrap_or("Meadow");
        let biome_mult = self.skill_tree.biome_value_mult(biome_label);
        let archivist_bonus = self.skill_tree.archivist_per_dex()
            * (self.caught.iter().filter(|c| **c).count() as u64);
        let mult = self.buffs.price_mult()
            * self.skill_tree.valu_mult()
            * biome_mult
            * (1.0 + self.player.tackle.sum_effect("valu_mult"))
            * (1.0 + self.scales_bonus("valu_mult"))
            * (1.0 + self.landmark_bonus("valu_mult"));
        let mut sold = 0u64;
        let mut total = 0u64;
        let mut keep: Vec<&'static crate::fish::FishDef> =
            Vec::with_capacity(self.player.inventory.len());
        for f in self.player.inventory.iter() {
            if !f.unique && f.name == name && sold < count {
                let mbonus = self.mastery_value_bonus(f);
                let lvl_decay = self.level_value_mult(f);
                let wmods = crate::weather::weather_modifiers(self.current_weather());
                let dim_bonus = self.dim_bonus_mult();
                let price = ((f.sell_price() as f32)
                    * mult
                    * (1.0 + mbonus + wmods.valu_pct)
                    * lvl_decay
                    * dim_bonus)
                    .round() as u64
                    + archivist_bonus;
                total = total.saturating_add(price);
                sold += 1;
            } else {
                keep.push(*f);
            }
        }
        if sold == 0 {
            return;
        }
        self.player.inventory = keep;
        self.player.valu = self.player.valu.saturating_add(total);
        self.lifetime_valu = self.lifetime_valu.saturating_add(total);
        self.stats.valu_earned = self.stats.valu_earned.saturating_add(total);
        self.stats.fish_sold = self.stats.fish_sold.saturating_add(sold);
        self.fish_sold_today = self.fish_sold_today.saturating_add(sold as u32);
        let cap = self.fishmonger_daily_cap();
        self.narrator.say(format!(
            "Sold {} {} for {}$V. ({}/{} today)",
            sold, name, total, self.fish_sold_today, cap,
        ));
        self.tick_mastery_challenges_sale(name, sold);
    }

    /// Sell every (non-unique) fish in the basket at fishmonger price.
    /// Multiplied by the player's price_mult buff. Unique fish stay.
    #[allow(dead_code)]
    fn sell_all_fish(&mut self) {
        let mult = self.buffs.price_mult();
        let mut total: u64 = 0;
        let mut sold: u64 = 0;
        // Partition: keep unique, sell the rest.
        let keep: Vec<&'static crate::fish::FishDef> = self
            .player
            .inventory
            .iter()
            .filter(|f| f.unique)
            .copied()
            .collect();
        for f in self
            .player
            .inventory
            .iter()
            .filter(|f| !f.unique)
        {
            let price = f.sell_price();
            let scaled = ((price as f32) * mult).round() as u64;
            total = total.saturating_add(scaled);
            sold += 1;
        }
        if sold == 0 {
            self.narrator
                .say("Fishmonger: \"You've nothing to sell, friend.\"");
            return;
        }
        self.player.inventory = keep;
        self.player.valu = self.player.valu.saturating_add(total);
        self.lifetime_valu = self.lifetime_valu.saturating_add(total);
        self.stats.valu_earned = self.stats.valu_earned.saturating_add(total);
        self.stats.fish_sold = self.stats.fish_sold.saturating_add(sold);
        if self.tutorial_step <= 4 {
            self.tutorial_advance(4);
        }
        let avg = total / sold.max(1);
        self.narrator.say(format!(
            "Fishmonger: \"{} fish for {}$V (avg {}). Pleasure.\"",
            sold, total, avg
        ));
    }

    /// Sailor on the pier. Until 1000 fish lifetime, he just chats. After
    /// that, he offers to row you out and dive — instantly puts you in
    /// Atlantis.
    /// Shipwright: builds the player a boat once they've caught 1250 fish.
    /// One-time. After the build, the player can `:inspect` water to board.
    fn interact_miner(&mut self) {
        const PICKAXE_COST: u64 = 500;
        const PICKAXE_ROD_GATE: u32 = 3;
        let Some(npc) = npc::npcs().iter().find(|n| n.id == "miner") else { return };
        if self.player.has_pickaxe {
            self.narrator.say(npc.response("owns_pickaxe", &[]));
            return;
        }
        if self.player.rods.max_owned < PICKAXE_ROD_GATE {
            self.narrator.say(format!(
                "Miner: \"You're swinging a stick. Come back with a tier-{PICKAXE_ROD_GATE} rod, friend.\""
            ));
            return;
        }
        if self.player.valu < PICKAXE_COST {
            self.narrator.say(npc.response(
                "cannot_afford",
                &[
                    ("cost", PICKAXE_COST.to_string()),
                    ("have", self.player.valu.to_string()),
                ],
            ));
            return;
        }
        self.player.valu -= PICKAXE_COST;
        self.player.has_pickaxe = true;
        self.narrator
            .say(npc.response("sold", &[("cost", PICKAXE_COST.to_string())]));
    }

    /// Attempt to open the bug-catch micro-game on the faced cell. Returns
    /// true if a bug was found and the scene opened (consumes the `f`).
    fn try_catch_bug_at(&mut self, nx: i32, ny: i32) -> bool {
        if !self.player.has_bug_net {
            return false;
        }
        let dim = self.world.dim;
        if self.bugs_picked_today.contains(&(dim, nx, ny)) {
            return false;
        }
        let tile = self.world.get(nx, ny);
        if !crate::bugs::tile_hosts_bugs(tile) {
            return false;
        }
        let day_id = crate::gametime::game_days(self.total_play_secs());
        let biome = self.world.biome(nx, ny);
        let is_night = matches!(
            crate::gametime::time_of_day(self.total_play_secs()),
            crate::gametime::TimeOfDay::Night
                | crate::gametime::TimeOfDay::Midnight
                | crate::gametime::TimeOfDay::Dusk
        );
        let Some(bug) =
            crate::bugs::bug_at(nx, ny, dim, biome, is_night, day_id, self.world.seed)
        else {
            return false;
        };
        let widen = self.skill_tree.bug_target_widen();
        let bc = crate::bug_catch::BugCatch::new(
            bug.id.clone(),
            (nx, ny),
            &mut self.rng_state,
            self.anim_tick,
            widen,
        );
        self.scene = Scene::BugCatch(bc);
        self.mode = Mode::Insert;
        self.narrator
            .say(format!("You ready the net for the {}.", bug.name));
        true
    }

    fn handle_bug_catch_key(&mut self, code: KeyCode) {
        let Scene::BugCatch(b) = &mut self.scene else { return };
        match code {
            KeyCode::Esc => {
                self.scene = Scene::Overworld;
            }
            KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::Enter => {
                b.attempt();
                // resolve_bug_catch will fire from tick() next frame; calling
                // it directly here would mid-borrow self.scene, so let the
                // tick path handle it.
            }
            _ => {}
        }
    }

    /// Clear the picked-today set when the in-game day advances. Called
    /// periodically from `tick()`.
    fn tick_bugs_day_rollover(&mut self) {
        let today = crate::gametime::game_days(self.total_play_secs());
        if today != self.bugs_picked_day_id {
            self.bugs_picked_today.clear();
            self.soil_dug_today.clear();
            self.foraged_today.clear();
            self.bugs_picked_day_id = today;
        }
    }

    /// Forage the faced cell for bait. Rocks / trees / cacti / flowers /
    /// pebbles each have biome-aware drop tables. Returns true if the
    /// interaction was consumed.
    fn try_forage_at(&mut self, nx: i32, ny: i32) -> bool {
        let dim = self.world.dim;
        if self.foraged_today.contains(&(dim, nx, ny)) {
            return false;
        }
        let tile = self.world.get(nx, ny);
        let biome = self.world.biome(nx, ny);
        let Some((action, table)) = crate::forage::forage_at(tile, biome, dim) else {
            return false;
        };
        let Some(bait_id) = crate::forage::pick(table, &mut self.rng_state) else {
            return false;
        };
        let full_id = if bait_id.starts_with("bug:") {
            bait_id.to_string()
        } else if crate::bugs::def_by_id(bait_id).is_some() {
            format!("bug:{bait_id}")
        } else {
            bait_id.to_string()
        };
        self.player.bait.add(&full_id, 1);
        self.foraged_today.insert((dim, nx, ny));
        let name = crate::bait::def_by_id(&full_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| bait_id.to_string());
        self.narrator
            .say(format!("You {} and find a {name}.", action.verb()));
        // Forage counts as a bug catch in the mastery vec if the underlying
        // entry is a bug — keeps the bug-50/bug-500 landmark capes reachable
        // from foraging alone.
        if let Some(idx) = crate::bugs::index_of(bait_id) {
            if let Some(slot) = self.bugs_caught.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        true
    }

    /// Dig a soil patch on the faced cell. Returns true if the dig was
    /// performed (consumes the `f` key). Yields 1-3 earthworm bait per
    /// patch; the patch refreshes the next in-game day.
    fn try_dig_soil_at(&mut self, nx: i32, ny: i32) -> bool {
        if !self.player.has_bug_net {
            return false;
        }
        let dim = self.world.dim;
        if self.soil_dug_today.contains(&(dim, nx, ny)) {
            return false;
        }
        let tile = self.world.get(nx, ny);
        if !crate::bugs::tile_hosts_soil(tile) {
            return false;
        }
        let biome = self.world.biome(nx, ny);
        if !crate::bugs::soil_at(nx, ny, dim, biome, self.world.seed) {
            return false;
        }
        let roll = crate::fish::next_rand_f32(&mut self.rng_state);
        let yield_ = 1 + (roll * 3.0) as u32; // 1..=3
        self.player.bait.add("earthworm", yield_);
        self.soil_dug_today.insert((dim, nx, ny));
        self.narrator
            .say(format!("You dig up {} earthworm(s).", yield_));
        true
    }

    /// Apply a finished bug-catch result. Called from `tick()` once the
    /// scene's `result` field is populated (either by the player attempting
    /// or by the deadline expiring).
    fn resolve_bug_catch(&mut self) {
        let Scene::BugCatch(b) = &self.scene else { return };
        let result = b.result;
        let bug_id = b.bug_id.clone();
        let xy = b.world_xy;
        let dim = self.world.dim;
        self.scene = Scene::Overworld;
        match result {
            Some(crate::bug_catch::BugCatchResult::Caught) => {
                if let Some(idx) = crate::bugs::index_of(&bug_id) {
                    if let Some(slot) = self.bugs_caught.get_mut(idx) {
                        *slot = slot.saturating_add(1);
                    }
                }
                self.bugs_picked_today.insert((dim, xy.0, xy.1));
                let name = crate::bugs::def_by_id(&bug_id)
                    .map(|b| b.name.clone())
                    .unwrap_or_else(|| bug_id.clone());
                let bait_id = format!("bug:{}", bug_id);
                self.player.bait.add(&bait_id, 1);
                self.narrator.say(format!("Caught the {name}. (+1 to bait stock)"));
            }
            Some(crate::bug_catch::BugCatchResult::Missed) => {
                // Bug flies off for the day either way — missing still
                // disturbs it. Keeps the catch a real decision.
                self.bugs_picked_today.insert((dim, xy.0, xy.1));
                self.narrator.say("It got away.".to_string());
            }
            None => {}
        }
    }

    /// Old Angler hands the player a Bug Net once they've reached a small
    /// catch threshold. Returns true if the interaction was consumed (and
    /// the caller should NOT fall through to the generic dialogue scene).
    fn interact_old_angler(&mut self) -> bool {
        const BUG_NET_CATCH_GATE: u64 = 25;
        let Some(npc) = npc::npcs().iter().find(|n| n.id == "old-angler") else { return false };
        if self.player.has_bug_net {
            // Existing dialogue still triggers most talks; only flag the
            // first re-visit with the "owned" line so the player knows the
            // angler tracks it.
            self.narrator.say(npc.response("bug_net_owned", &[]));
            return false;
        }
        if self.stats.fish_caught < BUG_NET_CATCH_GATE {
            let need = BUG_NET_CATCH_GATE - self.stats.fish_caught;
            self.narrator
                .say(npc.response("bug_net_hint", &[("need", need.to_string())]));
            return false;
        }
        self.player.has_bug_net = true;
        self.narrator.say(npc.response("bug_net_grant", &[]));
        true
    }

    fn interact_shipwright(&mut self) {
        // Now opens the hull-upgrade menu directly. Tier 0 → 1 (the first
        // boat) costs 1k valu + 10 wood and replaces the old "1250 lifetime
        // fish caught" gate that the original gameplay had buried behind
        // an undocumented stat. Subsequent tiers (Coastal Cutter, etc.)
        // gate ocean depth zones up through the Fog Sea.
        self.do_open_shipwright();
    }

    fn interact_sailor(&mut self) {
        const GATE: u64 = 1000;
        let Some(npc) = npc::npcs().iter().find(|n| n.id == "sailor") else { return };
        if self.stats.fish_caught < GATE {
            self.narrator.say(npc.response(
                "not_enough",
                &[("count", self.stats.fish_caught.to_string())],
            ));
            return;
        }
        const ATLANTIS_ROD_GATE: u32 = 50;
        if self.player.rods.max_owned < ATLANTIS_ROD_GATE {
            self.narrator.say(format!(
                "Sailor: \"You'd snap your line down there. Get a tier-{ATLANTIS_ROD_GATE} rod first.\""
            ));
            return;
        }
        self.world.dim = crate::world::Dimension::Atlantis;
        self.visited_atlantis = true;
        self.quest_progress("visit_dim", "Atlantis");
        self.player.x = 0;
        self.player.y = 7;
        self.narrator.say(npc.response("taking_you", &[]));
        self.narrator
            .say("*** You dive. Khei opens. Atlantis spreads below you. ***");
        self.narrator
            .say("To the north: the Five Elders' castle. Walk in.");
    }

    fn mark_seen_around_player(&mut self) {
        const VIEW_W: i32 = 50;
        const VIEW_H: i32 = 18;
        // Mapped Mind / fog_radius_bonus widens the reveal — each rank adds
        // one coarse cell on every side.
        let fog_bonus = self.skill_tree.fog_radius_bonus() as i32;
        let extra_w = fog_bonus * crate::app::MAP_CELL_W;
        let extra_h = fog_bonus * crate::app::MAP_CELL_H;
        let (px, py) = (self.player.x, self.player.y);
        let dim = self.world.dim;
        let set = self.seen_cells.entry(dim).or_default();
        for dy in -VIEW_H / 2 - extra_h..=VIEW_H / 2 + extra_h {
            for dx in -VIEW_W / 2 - extra_w..=VIEW_W / 2 + extra_w {
                let cc = coarse_cell(px + dx, py + dy);
                set.insert(cc);
            }
        }
    }

    fn check_biome_change(&mut self) {
        // Non-surface dimensions don't use the biome system; show the dim
        // name in the popup once on entry.
        if self.world.dim != crate::world::Dimension::Surface {
            let label = self.world.dim.label();
            if self.current_location.as_deref() != Some(label) {
                self.current_location = Some(label.to_string());
                self.biome_popup_ticks = 60;
                self.narrator.say(format!("Entered: {label}"));
            }
            return;
        }
        let b = biome_at(self.player.x, self.player.y, self.world.seed);
        self.current_biome = Some(b);
        let label = crate::world::location_name_at(
            self.player.x,
            self.player.y,
            self.world.seed,
        )
        .unwrap_or_else(|| b.label().to_string());
        if self.current_location.as_deref() != Some(label.as_str()) {
            self.current_location = Some(label.clone());
            self.biome_popup_ticks = 60; // 3s at 20fps
            self.narrator.say(format!("Entered: {label}"));
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let _frame_scope = crate::perf::Scope::new("render.total");
        let term = frame.area();
        if term.width < MIN_W || term.height < MIN_H {
            render_too_small(frame, term);
            return;
        }
        let anim_tick = self.anim_tick;
        let caught_snapshot = self.caught.clone();
        let caught_at_snapshot = self.caught_at.clone();
        let caught_context_snapshot = self.caught_context.clone();
        // Perf overlay: keep rendering the world underneath so the per-cell
        // instrumentation actually measures real work while the menu is up.
        // We swap to Overworld for the main match, then restore + draw the
        // small perf panel on top after everything else.
        let perf_overlay = matches!(self.scene, Scene::Perf);
        let saved_scene = if perf_overlay {
            Some(std::mem::replace(&mut self.scene, Scene::Overworld))
        } else {
            None
        };
        match &mut self.scene {
            Scene::Overworld => {
                let area = viewport(frame);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(
                        " fishcli  ({}, {}) ",
                        self.player.x, self.player.y
                    ))
                    .border_style(Style::default().fg(Color::Cyan));
                let inner = block.inner(area);
                frame.render_widget(block, area);
                let _world_scope = crate::perf::Scope::new("render.world");
                let dim_now = self.world.dim;
                let picked_dim: Vec<(i32, i32)> = self
                    .bugs_picked_today
                    .iter()
                    .filter(|(d, _, _)| *d == dim_now)
                    .map(|(_, x, y)| (*x, *y))
                    .collect();
                let soil_dim: Vec<(i32, i32)> = self
                    .soil_dug_today
                    .iter()
                    .filter(|(d, _, _)| *d == dim_now)
                    .map(|(_, x, y)| (*x, *y))
                    .collect();
                frame.render_widget(
                    WorldView {
                        world: &self.world,
                        player: (self.player.x, self.player.y),
                        player_facing: self.player.facing,
                        tick: anim_tick,
                        player_on_boat: self.player.on_boat,
                        player_swimming: false,
                        faceless: &self.faceless,
                        day_id: crate::gametime::game_days(self.total_play_secs()),
                        is_night: matches!(
                            crate::gametime::time_of_day(self.total_play_secs()),
                            crate::gametime::TimeOfDay::Night
                                | crate::gametime::TimeOfDay::Midnight
                                | crate::gametime::TimeOfDay::Dusk
                        ),
                        bugs_picked: &picked_dim,
                        soil_dug: &soil_dim,
                    },
                    inner,
                );
                crate::perf::flush_world_atomics();
                if let Some(c) = &self.cast {
                    render_cast_overlay(
                        frame,
                        inner,
                        (self.player.x, self.player.y),
                        c,
                        anim_tick,
                    );
                }
                // Weather visual effects were removed for perf reasons.
                // The HUD still displays the current weather as text.
                render_world_hud(
                    frame,
                    inner,
                    self.total_play_secs(),
                    self.world.dim,
                    self.current_biome
                        .unwrap_or(crate::world::Biome::Meadow),
                    self.world.seed,
                );
            }
            Scene::RodShop { cursor } => render_rod_shop(
                frame,
                *cursor,
                self.player.rods.max_owned,
                self.player.rods.equipped,
                self.player.valu,
            ),
            Scene::FishingSchool { cursor, tab } => render_skill_tree(
                frame,
                *cursor,
                *tab,
                &self.skill_tree,
                self.skills.fishing_level(),
                self.achievements.points_granted + self.daily_bonus_points + self.challenge_bonus_points,
                self.mastery_milestones,
                self.skills.encyclopedia_level(),
            ),
            Scene::Fishing(g) => {
                // fishing scene gets the whole frame; log is hidden during reel
                g.render(frame, viewport(frame), anim_tick);
            }
            Scene::Boss(b) => {
                b.render(frame, viewport(frame));
            }
            Scene::Fishdex(d) => {
                let unique = caught_snapshot.iter().filter(|c| **c).count() as u32;
                let blurb = match FISHDEX_MILES.iter().find(|m| unique < m.threshold) {
                    Some(m) => format!(
                        "  next: {} @ {} ({}/{})",
                        m.label, m.threshold, unique, m.threshold
                    ),
                    None => "  all milestones earned".to_string(),
                };
                d.render(
                    frame,
                    &caught_snapshot,
                    &caught_at_snapshot,
                    &caught_context_snapshot,
                    &self.mastery,
                    &blurb,
                );
            }
            Scene::NamePrompt(buf) => render_name_prompt(frame, buf),
            Scene::Dialogue { npc, line } => render_dialogue(frame, npc, *line),
            Scene::Notes(buf) => render_notes(frame, buf),
            Scene::Inventory { tab } => render_inventory(
                frame,
                &self.player.inventory,
                &self.player.items,
                *tab,
                &self.recipe_discovered,
            ),
            Scene::Help(topic) => render_help(frame, *topic),
            Scene::Stats => render_stats(
                frame,
                &self.player.name,
                self.player.valu,
                self.lifetime_valu,
                self.caught.iter().filter(|c| **c).count(),
                fishlist::fish().len(),
                self.player.inventory.len(),
                self.player.items.len(),
                self.quest_done.len(),
                self.total_play_secs(),
                &self.stats,
                &self.skills,
                &self.buffs,
                &self.player.gear,
                self.player.ingots.values().sum::<u32>(),
                self.fish_sold_today,
                self.fishmonger_daily_cap(),
                self.ore_sold_today,
                self.blacksmith_daily_cap(),
                self.recipe_discovered.iter().filter(|d| **d).count(),
                self.cooking_mastery.iter().filter(|m| **m >= 5).count(),
                crate::recipes::recipes().len(),
                self.player.wood,
                self.player.hull_tier,
                self.player.biofuel,
                self.player.crew_hunger,
                self.skill_tree.available(
                    self.skills.fishing_level(),
                    self.achievements.points_granted
                        + self.daily_bonus_points
                        + self.challenge_bonus_points,
                    self.mastery_milestones,
                    self.skills.encyclopedia_level(),
                ),
                crate::skill_tree::SkillTree::earned(
                    self.skills.fishing_level(),
                    self.achievements.points_granted
                        + self.daily_bonus_points
                        + self.challenge_bonus_points,
                    self.mastery_milestones,
                    self.skills.encyclopedia_level(),
                ),
            ),
            Scene::Settings => render_settings(frame, self.settings_cursor, &self.settings),
            Scene::Quests { cursor } => render_quests(
                frame,
                *cursor,
                &self.quest_progress,
                &self.quest_done,
                self.pinned_quest.as_deref(),
                self.daily_progress,
                self.daily_completed,
            ),
            Scene::Map { offset } => {
                let empty = std::collections::HashSet::new();
                let set = self.seen_cells.get(&self.world.dim).unwrap_or(&empty);
                render_map(
                    frame,
                    &self.world,
                    (self.player.x, self.player.y),
                    *offset,
                    set,
                );
            }
            Scene::Debug { cursor } => render_debug_console(
                frame,
                *cursor,
                self.player.valu,
                self.world.dim,
                &self.stats,
                &self.skills,
                &self.buffs,
                (self.player.x, self.player.y),
            ),
            Scene::LootPool { cursor } => render_loot_pool(
                frame,
                *cursor,
                self.current_pool_override.as_deref(),
            ),
            Scene::Fishmonger { cursor, step } => {
                let cursor = *cursor;
                // own the step shape so we can drop the &mut self.scene
                // borrow before calling self.fishmonger_listing(&self)
                let step_snap: FishmongerStep = match step {
                    FishmongerStep::PickFish => FishmongerStep::PickFish,
                    FishmongerStep::PickQuantity { picked, max } => {
                        FishmongerStep::PickQuantity {
                            picked: picked.clone(),
                            max: *max,
                        }
                    }
                    FishmongerStep::EnterQuantity { picked, max, buf } => {
                        FishmongerStep::EnterQuantity {
                            picked: picked.clone(),
                            max: *max,
                            buf: buf.clone(),
                        }
                    }
                    FishmongerStep::Confirm { picked, qty, total } => {
                        FishmongerStep::Confirm {
                            picked: picked.clone(),
                            qty: *qty,
                            total: *total,
                        }
                    }
                };
                let listing = self.fishmonger_listing();
                render_fishmonger(frame, cursor, &step_snap, &listing, self.player.valu);
            }
            Scene::Mining(m) => render_mining(frame, m),
            Scene::Smelt { .. } | Scene::Forge { .. } | Scene::Blacksmith { .. } | Scene::SellGear { .. } | Scene::Gear { .. } => {
                // Handled below: these renderers need read access to
                // `self` for inventory/skill data which the `&mut self.scene`
                // borrow above forbids.
            }
            Scene::HouseInterior { px, py, seed, .. } => {
                render_house(frame, *px, *py, *seed, self.player.facing);
            }
            Scene::TackleShop { slot_idx, cursor } => {
                render_tackle_shop(
                    frame,
                    *slot_idx,
                    *cursor,
                    &self.player.tackle,
                    self.player.valu,
                );
            }
            Scene::BaitShop { cursor } => {
                render_bait_shop(frame, *cursor, &self.player.bait, self.player.valu);
            }
            Scene::Shipwright { cursor } => {
                render_shipwright(
                    frame,
                    *cursor,
                    self.player.hull_tier,
                    self.player.valu,
                    self.player.wood,
                );
            }
            Scene::Chopping(c) => {
                render_chopping(frame, c, self.anim_tick);
            }
            Scene::Cooking { cursor, filter, editing_filter } => {
                render_cookbook(
                    frame,
                    *cursor,
                    self.skills.cooking_level(),
                    &self.cooking_mastery,
                    &self.recipe_discovered,
                    &self.player.inventory,
                    filter,
                    *editing_filter,
                );
            }
            Scene::Achievements { .. } => {
                // Handled below to side-step the &mut self.scene borrow that
                // prevents reading other fields of self.
            }
            Scene::BugCatch(b) => render_bug_catch(frame, b, self.anim_tick),
            Scene::Scales { cursor } => render_scales(
                frame,
                *cursor,
                self.scales,
                &self.scales_spent,
            ),
            Scene::LureBench { cursor } => render_lure_bench(
                frame,
                *cursor,
                &self.player.bait,
                self.player.valu,
            ),
            Scene::Perf => {
                // Never hit: we swap to Overworld above so the world keeps
                // rendering, then draw the perf panel as an overlay at the
                // bottom of this function.
            }
        }
        if let Some(s) = saved_scene {
            self.scene = s;
        }

        if let Scene::Gear { slot_idx, item_idx } = &self.scene {
            let slot_idx = *slot_idx;
            let item_idx = *item_idx;
            render_gear_panel(
                frame,
                slot_idx,
                item_idx,
                &Self::GEAR_SLOTS,
                &self.player.gear,
            );
        }
        if let Scene::SellGear { cursor } = &self.scene {
            let cursor = *cursor;
            let owned = self.player.gear.owned.clone();
            let equipped: std::collections::HashSet<String> = crate::gear::Slot::ALL
                .iter()
                .filter_map(|s| self.player.gear.equipped(*s).map(|x| x.to_string()))
                .collect();
            let mult = self.buffs.price_mult() * self.skill_tree.valu_mult();
            render_sell_gear(frame, cursor, &owned, &equipped, mult, self.ore_sold_today, self.blacksmith_daily_cap());
        }
        if let Scene::Blacksmith { cursor } = &self.scene {
            render_blacksmith_menu(
                frame,
                *cursor,
                self.smeltable_ores().len(),
                self.forgeable_gear().len(),
                self.player.items.iter().filter(|it| matches!(it.category, crate::item::Category::Mineral)).count() as u32,
                self.player.ingots.values().sum::<u32>(),
                self.ore_sold_today,
                self.blacksmith_daily_cap(),
            );
        }
        if let Scene::Smelt { cursor, typed } = &self.scene {
            let typed = typed.clone();
            let cursor = *cursor;
            render_smelt(
                frame,
                cursor,
                &typed,
                self.smeltable_ores(),
                &self.player.ingots,
            );
        }
        if let Scene::Forge { cursor, typed } = &self.scene {
            let typed = typed.clone();
            let cursor = *cursor;
            render_forge(
                frame,
                cursor,
                &typed,
                self.forgeable_gear(),
                &self.player.ingots,
                self.player.valu,
                self.skills.blacksmithing_level(),
            );
        }

        if let Scene::Achievements { cursor } = self.scene {
            let snap = crate::achievements::Snapshot {
                catch_total: self.stats.fish_caught,
                casts: self.stats.casts,
                steps: self.stats.steps,
                valu_earned: self.stats.valu_earned,
                fish_sold: self.stats.fish_sold,
                unique_species: self.caught.iter().filter(|c| **c).count() as u32,
                mastery_total: self.mastery_milestones,
                rod_tier: self.player.rods.max_owned,
                mining_level: self.skills.mining_level(),
                fishing_level: self.skills.fishing_level(),
                play_hours: self.total_play_secs() / 3600,
                has_pickaxe: self.player.has_pickaxe,
                has_boat: self.player.has_boat,
                visited_mines: self.visited_mines,
                visited_atlantis: self.visited_atlantis,
                visited_inferno: self.visited_inferno,
                recipes_cooked: self.cooking_mastery.iter().map(|&m| m as u64).sum(),
                recipes_mastered: self.cooking_mastery.iter().filter(|&&m| m >= 5).count() as u32,
                recipes_discovered: self.recipe_discovered.iter().filter(|d| **d).count() as u32,
                wood_chopped: self.stats.wood_chopped,
                trees_felled: self.stats.trees_felled,
                encyclopedia_level: self.skills.encyclopedia_level(),
                cooking_level: self.skills.cooking_level(),
                woodcutting_level: self.skills.woodcutting_level(),
                hull_tier: self.player.hull_tier,
                max_catch_streak: self.stats.max_catch_streak,
                shiny_catches: self.stats.shiny_catches,
                already_unlocked: &self.achievements.unlocked,
            };
            render_achievements(frame, cursor, &snap, &self.achievements);
        }

        if matches!(self.scene, Scene::NamePrompt(_)) {
            return;
        }

        let full = viewport(frame);
        let cmdline_h = 1u16;
        let effective_h = full.height.saturating_sub(cmdline_h);
        // The log/valu HUD belongs on the Overworld and inside houses
        // (so the player can read inspect output). Every other scene is a
        // full-screen menu and shouldn't have the log slab on it. Perf is
        // a transparent overlay over the live overworld, so treat it like
        // Overworld here — the player needs to see the full HUD beneath.
        let in_modal = !matches!(
            self.scene,
            Scene::Overworld | Scene::HouseInterior { .. } | Scene::Perf
        );
        if in_modal {
            // only render cmdline at the very bottom
            if cmdline_h > 0 && full.height >= cmdline_h {
                let cmd_area = Rect {
                    x: full.x,
                    y: full.y + full.height - cmdline_h,
                    width: full.width,
                    height: cmdline_h,
                };
                render_cmdline(frame, cmd_area, &self.mode);
            }
            return;
        }

        let valu_str = format_valu(self.player.valu);
        let valu_w = (valu_str.len() as u16 + 4).max(14).min(full.width);
        let valu_h = 3u16.min(effective_h);
        let mut valu_w_taken = 0u16;
        if valu_w >= 8 && valu_h >= 3 {
            let valu_area = Rect {
                x: full.x + full.width - valu_w,
                y: full.y + effective_h - valu_h,
                width: valu_w,
                height: valu_h,
            };
            render_valu(frame, valu_area, &valu_str);
            valu_w_taken = valu_w;
        }

        let log_w = 42u16.min(full.width.saturating_sub(valu_w_taken));
        let log_h = self.settings.log_lines.clamp(5, 15).min(effective_h);
        if log_w > 4 && log_h > 2 {
            let log_area = Rect {
                x: full.x,
                y: full.y + effective_h - log_h,
                width: log_w,
                height: log_h,
            };
            self.narrator.render(frame, log_area);
        }

        // Stamina bar: 1-row strip just above the log, spanning the same
        // horizontal slice. Always visible in Overworld/HouseInterior.
        let stam_h = 1u16;
        if log_w > 4 && effective_h > log_h + stam_h {
            let stam_area = Rect {
                x: full.x,
                y: full.y + effective_h - log_h - stam_h,
                width: log_w,
                height: stam_h,
            };
            render_stamina_bar(frame, stam_area, self.stamina, self.stamina_max());
        }

        // Boat HUD strip: shows hull tier, crew hunger, biofuel. Only
        // rendered while aboard so land play stays uncluttered.
        let boat_h = 1u16;
        if self.player.on_boat
            && log_w > 4
            && effective_h > log_h + stam_h + boat_h
        {
            let boat_area = Rect {
                x: full.x,
                y: full.y + effective_h - log_h - stam_h - boat_h,
                width: log_w,
                height: boat_h,
            };
            // Surface-only: surface a current/max-depth pair so the player
            // can see how close the hull is to its gate. Off-surface we
            // just show 0/0 (depth isn't meaningful in the Mines etc.).
            let (cur_depth, max_depth) =
                if matches!(self.world.dim, crate::world::Dimension::Surface) {
                    (
                        ocean_depth_at(&self.world, self.player.x, self.player.y),
                        crate::player::ocean_depth_max(self.player.hull_tier),
                    )
                } else {
                    (0, 0)
                };
            render_boat_hud(
                frame,
                boat_area,
                self.player.hull_tier,
                self.player.crew_hunger,
                self.player.biofuel,
                self.player.wood,
                cur_depth,
                max_depth,
            );
        }

        if cmdline_h > 0 && full.height >= cmdline_h {
            let cmd_area = Rect {
                x: full.x,
                y: full.y + full.height - cmdline_h,
                width: full.width,
                height: cmdline_h,
            };
            render_cmdline(frame, cmd_area, &self.mode);
        }

        if self.biome_popup_ticks > 0 {
            if let Some(ref name) = self.current_location {
                render_location_popup(frame, name);
            }
        }

        if !self.banners.is_empty() {
            render_banners(frame, &self.banners);
        }

        if let Some(id) = self.pinned_quest.as_deref() {
            if let Some(q) = quest::quests().iter().find(|q| q.id == id) {
                if !self.quest_done.contains(&q.id) {
                    let progress = self.quest_progress.get(&q.id).copied().unwrap_or(0);
                    render_pinned_task(frame, q, progress);
                }
            }
        }
        if self.stats.catch_streak > 0 {
            render_streak_chip(
                frame,
                self.stats.catch_streak,
                self.stats.max_catch_streak,
            );
        }
        // Perf overlay — drawn last so it sits above world + HUD without
        // pausing them. The world view above re-ran with all its scopes,
        // so the numbers reflect real ongoing render cost.
        if matches!(self.scene, Scene::Perf) {
            render_perf_overlay(frame);
        }
    }
}

fn quest_title(id: &str) -> Option<&'static str> {
    quest::quests().iter().find(|q| q.id == id).map(|q| q.title.as_str())
}

fn active_quest_ids(done: &[String]) -> Vec<String> {
    quest::quests()
        .iter()
        .filter(|q| !done.contains(&q.id))
        .map(|q| q.id.clone())
        .collect()
}

/// Spawn a worker thread that drains autosave snapshots and writes them
/// to disk. Coalesces: if multiple snapshots are pending, only the newest
/// is written. Thread exits cleanly when the sender (App) is dropped.
/// Spin a background thread that pre-warms the global world cache around
/// the origin so the player's first few thousand cell views hit cache.
/// Non-blocking — the game starts immediately and progressively speeds up
/// as the warm completes.
fn kick_off_pregen(seed: u32) {
    std::thread::spawn(move || {
        let w = crate::world::World::new(seed);
        w.pregen_square(500);
    });
}

fn spawn_autosaver() -> mpsc::Sender<SaveData> {
    let (tx, rx) = mpsc::channel::<SaveData>();
    std::thread::spawn(move || {
        while let Ok(mut latest) = rx.recv() {
            // coalesce additional pending snapshots
            while let Ok(d) = rx.try_recv() {
                latest = d;
            }
            let _ = save::save_to_disk(&latest);
        }
    });
    tx
}

fn save_hash(data: &SaveData) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let bytes = serde_json::to_vec(data).unwrap_or_default();
    let mut h = DefaultHasher::new();
    h.write(&bytes);
    h.finish()
}

pub const MAP_CELL_W: i32 = 8;
pub const MAP_CELL_H: i32 = 4;

pub fn coarse_cell(x: i32, y: i32) -> (i32, i32) {
    (x.div_euclid(MAP_CELL_W), y.div_euclid(MAP_CELL_H))
}

fn render_map(
    frame: &mut Frame,
    world: &World,
    player: (i32, i32),
    offset: (i32, i32),
    seen: &std::collections::HashSet<(i32, i32)>,
) {
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Widget;
    struct MapView<'a> {
        world: &'a World,
        player: (i32, i32),
        offset: (i32, i32),
        seen: &'a std::collections::HashSet<(i32, i32)>,
    }
    impl<'a> Widget for MapView<'a> {
        fn render(self, area: Rect, buf: &mut Buffer) {
            let pcx = self.player.0.div_euclid(MAP_CELL_W) + self.offset.0;
            let pcy = self.player.1.div_euclid(MAP_CELL_H) + self.offset.1;
            let player_coarse = (
                self.player.0.div_euclid(MAP_CELL_W),
                self.player.1.div_euclid(MAP_CELL_H),
            );
            let half_w = (area.width as i32) / 2;
            let half_h = (area.height as i32) / 2;
            for sy in 0..area.height {
                for sx in 0..area.width {
                    let cx = area.x + sx;
                    let cy = area.y + sy;
                    let cell = &mut buf[(cx, cy)];
                    let mcx = pcx - half_w + sx as i32;
                    let mcy = pcy - half_h + sy as i32;
                    if (mcx, mcy) == player_coarse {
                        cell.set_char('@').set_style(
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(40, 40, 40)),
                        );
                        continue;
                    }
                    if !self.seen.contains(&(mcx, mcy)) {
                        cell.set_char(' ').set_style(Style::default().bg(Color::Rgb(8, 8, 12)));
                        continue;
                    }
                    let wx = mcx * MAP_CELL_W + MAP_CELL_W / 2;
                    let wy = mcy * MAP_CELL_H + MAP_CELL_H / 2;
                    let (g, s) = map_glyph_for(self.world, wx, wy);
                    cell.set_char(g).set_style(s);
                }
            }
        }
    }
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" map  (hjkl pan, q close)  @=you  M=mine  D=door  Δ§Ω†❄☼☄∞ΨΦ◊=portals ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        MapView {
            world,
            player,
            offset,
            seen,
        },
        inner,
    );

    // overlay village name labels for any village whose center is in view & seen
    let pcx = player.0.div_euclid(MAP_CELL_W) + offset.0;
    let pcy = player.1.div_euclid(MAP_CELL_H) + offset.1;
    let half_w = (inner.width as i32) / 2;
    let half_h = (inner.height as i32) / 2;
    let view_x_lo = (pcx - half_w) * MAP_CELL_W;
    let view_x_hi = (pcx + half_w) * MAP_CELL_W;
    let view_y_lo = (pcy - half_h) * MAP_CELL_H;
    let view_y_hi = (pcy + half_h) * MAP_CELL_H;
    for v in crate::world::villages_in_rect(view_x_lo, view_y_lo, view_x_hi, view_y_hi, world.seed)
    {
        let vcx = v.ax.div_euclid(MAP_CELL_W);
        let vcy = v.ay.div_euclid(MAP_CELL_H);
        if !seen.contains(&(vcx, vcy)) {
            continue;
        }
        let sx = vcx - (pcx - half_w);
        let sy = vcy - (pcy - half_h);
        if sx < 0 || sy < 0 || sx >= inner.width as i32 || sy >= inner.height as i32 {
            continue;
        }
        let label = crate::world::village_name(v.hash);
        // place label one row above the dot, centered
        let lw = label.len() as i32;
        let lx = (sx - lw / 2).max(0).min(inner.width as i32 - lw);
        let ly = (sy - 1).max(0);
        let buf = frame.buffer_mut();
        for (i, ch) in label.chars().enumerate() {
            let cx = (inner.x as i32 + lx + i as i32) as u16;
            let cy = (inner.y as i32 + ly) as u16;
            buf[(cx, cy)]
                .set_char(ch)
                .set_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
        }
    }
}

fn biome_map_bg(b: crate::world::Biome) -> Color {
    use crate::world::Biome;
    match b {
        Biome::Meadow => Color::Rgb(40, 70, 40),
        Biome::Forest => Color::Rgb(25, 50, 28),
        Biome::Rocky => Color::Rgb(75, 70, 55),
        Biome::Scrub => Color::Rgb(85, 78, 55),
        Biome::Desert => Color::Rgb(115, 90, 55),
        Biome::Tundra => Color::Rgb(110, 115, 120),
        Biome::Swamp => Color::Rgb(35, 50, 30),
    }
}

fn map_glyph_for(world: &World, x: i32, y: i32) -> (char, Style) {
    use crate::world::Dimension;
    let bg = match world.dim {
        Dimension::Surface => biome_map_bg(world.biome(x, y)),
        Dimension::Mines => Color::Rgb(28, 18, 14),
        Dimension::Atlantis => Color::Rgb(10, 28, 60),
        Dimension::Inferno => Color::Rgb(40, 12, 8),
        // specialty dims: dark backdrop matching their theme
        _ => Color::Rgb(20, 20, 30),
    };
    let t = world.get(x, y);
    let (g, fg) = match t {
        Tile::Water => ('~', Color::Rgb(120, 170, 220)),
        Tile::Sand => (',', Color::Rgb(220, 200, 145)),
        Tile::Dock => ('=', Color::Rgb(210, 175, 110)),
        Tile::Well => ('O', Color::Rgb(200, 200, 215)),
        Tile::Wall => ('#', Color::Rgb(180, 145, 95)),
        Tile::Roof => ('#', Color::Rgb(200, 100, 70)),
        Tile::DoorRod | Tile::DoorSchool => ('D', Color::Rgb(245, 215, 90)),
        Tile::DoorHouse => ('D', Color::Rgb(180, 150, 110)),
        Tile::DimPortal => {
            let dest = crate::world::dim_portal_for(x, y, world.seed)
                .unwrap_or(crate::world::Dimension::Surface);
            let glyph = match dest {
                crate::world::Dimension::Pyramid => 'Δ',
                crate::world::Dimension::HotSpring => '§',
                crate::world::Dimension::Iceshelf => '❄',
                crate::world::Dimension::SwampCave => 'Ω',
                crate::world::Dimension::BogCathedral => '†',
                crate::world::Dimension::MirrorLake => '☼',
                crate::world::Dimension::Crater => '☄',
                crate::world::Dimension::Colosseum => '∞',
                crate::world::Dimension::Sewer => 'Ψ',
                crate::world::Dimension::Wreckage => 'Φ',
                crate::world::Dimension::AllBlue => '◊',
                _ => '¤',
            };
            (glyph, Color::Rgb(230, 200, 255))
        }
        Tile::TreeCanopy | Tile::TreeTrunk => ('T', Color::Rgb(110, 200, 95)),
        Tile::BigRock | Tile::MediumRock | Tile::Rock => ('#', Color::Rgb(170, 170, 170)),
        Tile::Path => ('.', Color::Rgb(195, 170, 130)),
        Tile::Lamppost => ('i', Color::Rgb(240, 215, 130)),
        Tile::Bench => ('=', Color::Rgb(170, 115, 70)),
        Tile::Cactus => ('Y', Color::Rgb(130, 180, 100)),
        Tile::Pebble => ('.', Color::Rgb(170, 160, 130)),
        Tile::Flower => ('*', Color::Rgb(210, 190, 180)),
        Tile::Grass => ('.', Color::Rgb(130, 175, 130)),
        Tile::MineEntrance => ('M', Color::Rgb(255, 180, 80)),
        Tile::MineFrame => ('#', Color::Rgb(120, 80, 45)),
        Tile::CaveFloor | Tile::CaveWall | Tile::Stalactite | Tile::Stalagmite => {
            ('#', Color::Rgb(90, 65, 45))
        }
        Tile::OreRock => ('*', Color::Rgb(220, 200, 90)),
        Tile::MineralWater => ('~', Color::Rgb(120, 200, 240)),
        Tile::MineExit => ('<', Color::LightYellow),
        Tile::Seabed => (',', Color::Rgb(170, 190, 200)),
        Tile::CoralTrunk | Tile::CoralCanopy => ('*', Color::Rgb(240, 130, 150)),
        Tile::Kelp => ('i', Color::Rgb(80, 200, 130)),
        Tile::DeepWater => ('~', Color::Rgb(80, 130, 200)),
        Tile::Anemone => ('o', Color::Rgb(255, 150, 90)),
        Tile::InfernoWall | Tile::InfernoFloor => ('#', Color::Rgb(180, 70, 30)),
        Tile::Lava => ('~', Color::Rgb(255, 110, 30)),
        Tile::LandmarkWall => ('H', Color::Rgb(220, 220, 220)),
        Tile::LandmarkDoor => ('D', Color::LightYellow),
        Tile::Tombstone => ('T', Color::Rgb(180, 180, 190)),
        Tile::Smelter => ('S', Color::Rgb(255, 140, 60)),
        Tile::Forge => ('F', Color::Rgb(255, 90, 60)),
        Tile::CookingPot => ('O', Color::Rgb(255, 200, 120)),
        Tile::BaitBench => ('=', Color::Rgb(180, 130, 80)),
        Tile::Curio => ('*', Color::Rgb(220, 200, 160)),
        Tile::PortalFrame => ('#', Color::Rgb(190, 175, 200)),
    };
    // water cells override the biome bg with deep blue
    let final_bg = if matches!(t, Tile::Water) {
        Color::Rgb(10, 25, 65)
    } else {
        bg
    };
    (g, Style::default().fg(fg).bg(final_bg).add_modifier(Modifier::BOLD))
}

/// Small chip pinned to the top-right of the viewport showing the current
/// catch streak. Sits one row below the weather/HUD panel so it stops
/// covering the season/time/date lines.
fn render_streak_chip(frame: &mut Frame, current: u64, best: u64) {
    let area = viewport(frame);
    let body = if best > current {
        format!(" >>> streak {current}  (best {best}) ")
    } else {
        format!(" >>> streak {current} ")
    };
    let w = (body.len() as u16 + 2).min(area.width);
    let h = 3u16.min(area.height);
    if w < 6 || h < 3 {
        return;
    }
    // Weather/HUD panel is 4 rows tall and renders inside the world block's
    // inner area, which is itself offset by 1 row of border — so the panel
    // actually covers viewport rows 1..=4. Start the chip at row 5.
    let y_off = 5u16;
    if area.height <= y_off + h {
        return;
    }
    let rect = Rect {
        x: area.x + area.width.saturating_sub(w),
        y: area.y + y_off,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, rect);
    // Low-sat fiery orange — close to a banked-coal tone, not a saturated
    // alert orange. Stands out from the cyan/yellow borders used elsewhere.
    let fire = Color::Rgb(200, 110, 50);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(fire));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let line = ratatui::text::Line::from(ratatui::text::Span::styled(
        body,
        Style::default().fg(fire).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(vec![line]), inner);
}

fn render_pinned_task(frame: &mut Frame, q: &quest::QuestDef, progress: u32) {
    let area = viewport(frame);
    let title_line = format!(" {} ", q.title);
    let progress_line = format!(" {}/{} ", progress, q.objective.count);
    let w = (title_line.len().max(progress_line.len()) as u16 + 2).min(area.width);
    let h = 4u16.min(area.height);
    if w < 6 || h < 4 {
        return;
    }
    let rect = Rect {
        x: area.x,
        y: area.y,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" task ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines = vec![
        ratatui::text::Line::from(ratatui::text::Span::styled(
            title_line,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ratatui::text::Line::from(ratatui::text::Span::styled(
            progress_line,
            Style::default().fg(Color::LightYellow),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_quests(
    frame: &mut Frame,
    cursor: usize,
    progress: &HashMap<String, u32>,
    done: &[String],
    pinned: Option<&str>,
    daily_progress: u32,
    daily_completed: bool,
) {
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" tasks (j/k navigate, p pin/unpin, q/esc close) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    // Daily quest at the top so the player sees today's task immediately.
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  TODAY",
        Style::default()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(def) = crate::daily::today_def() {
        let (state_label, state_color) = if daily_completed {
            ("[done]", Color::Green)
        } else {
            ("[active]", Color::Yellow)
        };
        lines.push(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                "    ".to_string(),
                Style::default(),
            ),
            ratatui::text::Span::styled(
                state_label.to_string(),
                Style::default().fg(state_color).add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw("  "),
            ratatui::text::Span::styled(
                def.title.clone(),
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(format!("  {}/{}  ", daily_progress, def.count)),
            ratatui::text::Span::styled(
                def.description.clone(),
                Style::default().fg(Color::Gray),
            ),
            ratatui::text::Span::styled(
                format!("   (+{}$V, +{}sp)", def.reward_valu, def.reward_points),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    } else {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "    (no daily today)".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  ACTIVE",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    let active: Vec<&quest::QuestDef> = quest::quests().iter().filter(|q| !done.contains(&q.id)).collect();
    if active.is_empty() {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "    (none)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for (i, q) in active.iter().enumerate() {
        let cur = progress.get(&q.id).copied().unwrap_or(0);
        let is_pinned = pinned == Some(q.id.as_str());
        let prefix = if i == cursor { "> " } else { "  " };
        let pin_marker = if is_pinned { "[PIN] " } else { "      " };
        let line = ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                format!("{prefix}{pin_marker}"),
                Style::default().fg(Color::Yellow),
            ),
            ratatui::text::Span::styled(
                q.title.clone(),
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(format!("  {}/{}  ", cur, q.objective.count)),
            ratatui::text::Span::styled(
                q.description.clone(),
                Style::default().fg(Color::Gray),
            ),
        ]);
        lines.push(line);
    }

    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  COMPLETED",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    let mut any_done = false;
    for q in quest::quests().iter().filter(|q| done.contains(&q.id)) {
        lines.push(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                "    ".to_string(),
                Style::default(),
            ),
            ratatui::text::Span::styled(
                q.title.clone(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        any_done = true;
    }
    if !any_done {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "    (none yet)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_cast_overlay(
    frame: &mut Frame,
    area: Rect,
    player: (i32, i32),
    c: &CastState,
    anim_tick: u64,
) {
    let half_w = (area.width as i32) / 2;
    let half_h = (area.height as i32) / 2;
    let player_sx = area.x as i32 + half_w;
    let player_sy = area.y as i32 + half_h;

    match c.phase {
        CastPhase::Casting => {
            // bordered cast meter above the player. Inside the box a 2-tall
            // single-column cell slides up/down, bg color is the strength.
            let bar_h = 8i32;
            let box_top = player_sy - bar_h - 3;
            let box_bot = player_sy - 2;
            let box_x_left = player_sx - 1;
            let box_x_right = player_sx + 1;
            let buf = frame.buffer_mut();
            let in_area = |sx: i32, sy: i32| {
                sx >= area.x as i32
                    && sy >= area.y as i32
                    && sx < (area.x + area.width) as i32
                    && sy < (area.y + area.height) as i32
            };
            // black interior so the cast meter doesn't blend into terrain
            for sy in (box_top + 1)..=(box_bot - 1) {
                for sx in (box_x_left + 1)..=(box_x_right - 1) {
                    if !in_area(sx, sy) {
                        continue;
                    }
                    buf[(sx as u16, sy as u16)]
                        .set_char(' ')
                        .set_style(Style::default().bg(Color::Black));
                }
            }
            // vertical sides (skip corners)
            for sy in (box_top + 1)..=(box_bot - 1) {
                if in_area(box_x_left, sy) {
                    buf[(box_x_left as u16, sy as u16)]
                        .set_char('|')
                        .set_style(Style::default().fg(Color::Yellow).bg(Color::Black));
                }
                if in_area(box_x_right, sy) {
                    buf[(box_x_right as u16, sy as u16)]
                        .set_char('|')
                        .set_style(Style::default().fg(Color::Yellow).bg(Color::Black));
                }
            }
            // horizontal top/bottom (skip corners)
            for sx in (box_x_left + 1)..=(box_x_right - 1) {
                if in_area(sx, box_top) {
                    buf[(sx as u16, box_top as u16)]
                        .set_char('-')
                        .set_style(Style::default().fg(Color::Yellow).bg(Color::Black));
                }
                if in_area(sx, box_bot) {
                    buf[(sx as u16, box_bot as u16)]
                        .set_char('-')
                        .set_style(Style::default().fg(Color::Yellow).bg(Color::Black));
                }
            }
            // corners
            for (sx, sy) in [
                (box_x_left, box_top),
                (box_x_right, box_top),
                (box_x_left, box_bot),
                (box_x_right, box_bot),
            ] {
                if in_area(sx, sy) {
                    buf[(sx as u16, sy as u16)]
                        .set_char('+')
                        .set_style(Style::default().fg(Color::Yellow).bg(Color::Black));
                }
            }
            // 2-tall moving cell inside the box (range: box_top+1..=box_bot-1, with size 2)
            let inner_h = bar_h;
            let marker_top = box_top + 1 + ((1.0 - c.cast_pos) * (inner_h - 2) as f32).round() as i32;
            let marker_bot = marker_top + 1;
            let r = ((1.0 - c.cast_pos) * 230.0) as u8;
            let g = (c.cast_pos * 220.0) as u8;
            let color = Color::Rgb(r, g, 30);
            for sy in [marker_top, marker_bot] {
                if sy < area.y as i32 || sy >= (area.y + area.height) as i32 {
                    continue;
                }
                let sx = player_sx;
                if sx < area.x as i32 || sx >= (area.x + area.width) as i32 {
                    continue;
                }
                buf[(sx as u16, sy as u16)]
                    .set_char(' ')
                    .set_style(Style::default().bg(color));
            }
        }
        CastPhase::Waiting | CastPhase::Biting => {
            // bobber lives on the world cell. project to screen.
            let bsx = player_sx + (c.bobber.0 - player.0);
            let bsy = player_sy + (c.bobber.1 - player.1);
            if bsx < area.x as i32
                || bsy < area.y as i32
                || bsx >= (area.x + area.width) as i32
                || bsy >= (area.y + area.height) as i32
            {
                return;
            }
            let (ch, style) = match c.phase {
                CastPhase::Biting => (
                    '!',
                    Style::default()
                        .fg(Color::Red)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                _ => {
                    let on = (anim_tick / 10) % 2 == 0;
                    let col = if on { Color::Red } else { Color::White };
                    (
                        '*',
                        Style::default().fg(col).add_modifier(Modifier::BOLD),
                    )
                }
            };
            frame.buffer_mut()[(bsx as u16, bsy as u16)]
                .set_char(ch)
                .set_style(style);
        }
    }
}


fn render_rod_shop(
    frame: &mut Frame,
    cursor: u32,
    owned: u32,
    equipped: u32,
    valu: u64,
) {
    use crate::rod::rods;
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " rod shop  ({} owned, #{equipped} equipped)  j/k browse, enter buy, e equip, q close ",
            owned
        ))
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible = inner.height as usize;
    let total = rods().len();
    // center cursor in view
    let start = (cursor as usize).saturating_sub(visible / 2);
    let start = start.min(total.saturating_sub(visible));
    let mut lines: Vec<ratatui::text::Line> = Vec::with_capacity(visible);
    for i in start..(start + visible).min(total) {
        let rod = &rods()[i];
        let tier = rod.tier;
        let is_selected = tier == cursor + 1;
        let is_owned = tier <= owned;
        let is_equipped = tier == equipped;
        let is_next = tier == owned + 1;
        let prefix = if is_selected { "> " } else { "  " };
        let status = if is_equipped {
            "[E]"
        } else if is_owned {
            "[OWN]"
        } else if is_next {
            if valu >= rod.price() {
                "[BUY]"
            } else {
                "[$$$]"
            }
        } else {
            "[LCK]"
        };
        let color = if is_equipped {
            Color::LightGreen
        } else if is_owned {
            Color::Green
        } else if is_next && valu >= rod.price() {
            Color::LightYellow
        } else if is_next {
            Color::Red
        } else {
            Color::DarkGray
        };
        let style = if is_selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD).bg(Color::Rgb(40, 40, 40))
        } else {
            Style::default().fg(color)
        };
        let price_label = if tier == 201 {
            "1000000$V + THE FISH".to_string()
        } else if tier == 202 {
            "THE PANTHEON (Ish + Fsh + Fih + Fis)".to_string()
        } else {
            format!("{}$V", rod.price())
        };
        lines.push(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(format!("{prefix}{status} "), style),
            ratatui::text::Span::styled(
                format!("#{tier:>3} {:<28}", rod.name),
                style,
            ),
            ratatui::text::Span::styled(
                price_label,
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Render every active banner top-down, centered horizontally. Each
/// banner gets a bordered box sized to its content; the next banner
/// stacks immediately below the previous one so a burst of events all
/// stay visible at once.
fn render_banners(frame: &mut Frame, banners: &[Banner]) {
    use crate::stats::level_to_xp;
    let area = viewport(frame);
    let mut y = area.y + 1;
    for b in banners {
        let (border_color, title) = match &b.kind {
            BannerKind::Discovery => (Color::LightYellow, " new discovery "),
            BannerKind::Recipe => (Color::LightMagenta, " new recipe "),
            BannerKind::Achievement => (Color::Rgb(220, 180, 60), " achievement "),
            BannerKind::Xp { .. } => (Color::LightGreen, " xp gained "),
        };
        let h: u16 = match &b.kind {
            BannerKind::Xp { .. } => 4,
            _ => 3,
        };
        if y + h > area.y + area.height {
            break;
        }
        let inner_w_needed = (b.line.chars().count() as u16).saturating_add(2);
        let w = inner_w_needed
            .saturating_add(2)
            .clamp(20, area.width.saturating_sub(2));
        let x = area.x + area.width.saturating_sub(w) / 2;
        let rect = Rect { x, y, width: w, height: h };
        frame.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        match &b.kind {
            BannerKind::Xp { level, total_xp, .. } => {
                let cur_floor = level_to_xp(*level);
                let next = level_to_xp(level + 1);
                let span = (next - cur_floor).max(1);
                let progress = total_xp.saturating_sub(cur_floor);
                let bar_w = inner.width.saturating_sub(2) as usize;
                let filled =
                    ((progress as f32 / span as f32) * bar_w as f32) as usize;
                let bar: String = std::iter::repeat('=')
                    .take(filled)
                    .chain(std::iter::repeat('-').take(bar_w.saturating_sub(filled)))
                    .collect();
                let lines = vec![
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        b.line.clone(),
                        Style::default()
                            .fg(border_color)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .alignment(Alignment::Center),
                    ratatui::text::Line::from(format!(" [{bar}]  Lv {level}")),
                ];
                frame.render_widget(Paragraph::new(lines), inner);
            }
            _ => {
                let para = Paragraph::new(ratatui::text::Span::styled(
                    b.line.clone(),
                    Style::default()
                        .fg(border_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center);
                frame.render_widget(para, inner);
            }
        }
        y = y.saturating_add(h);
    }
}

fn render_location_popup(frame: &mut Frame, label: &str) {
    let area = viewport(frame);
    let w = (label.len() as u16 + 6).min(area.width);
    let h = 3u16.min(area.height);
    if w < 6 || h < 3 {
        return;
    }
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + 1;
    let popup = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let p = Paragraph::new(label)
        .style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(block);
    frame.render_widget(p, popup);
}

// (weather visual overlay removed — HUD-only)
// ---- top-right HUD: date / time / weather ---------------------------

fn render_world_hud(
    frame: &mut Frame,
    inner: ratatui::layout::Rect,
    total_play_secs: u64,
    dim: crate::world::Dimension,
    biome: crate::world::Biome,
    seed: u32,
) {
    use crate::gametime;
    use crate::weather;
    let season = gametime::season(total_play_secs);
    let tod = gametime::time_of_day(total_play_secs);
    let day = gametime::day_of_month(total_play_secs);
    let month = gametime::month_of_year(total_play_secs) + 1;
    let yr = gametime::year(total_play_secs);
    let hour = gametime::hour_of_day(total_play_secs);
    let minute = gametime::minute_of_hour(total_play_secs);
    let game_day = gametime::game_days(total_play_secs);
    let w = weather::weather_for(game_day, dim, biome, seed);

    // category label is white, the *value* takes the colour. Built as
    // styled spans on a single line, right-aligned inside `inner`.
    let lines: Vec<ratatui::text::Line> = vec![
        ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                format!("{} ", season.icon()),
                Style::default().fg(season.color()).add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(
                "Season ",
                Style::default().fg(Color::White),
            ),
            ratatui::text::Span::styled(
                season.label(),
                Style::default().fg(season.color()).add_modifier(Modifier::BOLD),
            ),
        ]),
        ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                format!("{} ", tod.icon()),
                Style::default().fg(tod.color()).add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(
                format!("{:02}:{:02} ", hour, minute),
                Style::default().fg(Color::White),
            ),
            ratatui::text::Span::styled(
                tod.label(),
                Style::default().fg(tod.color()).add_modifier(Modifier::BOLD),
            ),
        ]),
        ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                "  M",
                Style::default().fg(Color::White),
            ),
            ratatui::text::Span::styled(
                format!("{:02} D{:02} ", month, day),
                Style::default().fg(Color::White),
            ),
            ratatui::text::Span::styled(
                format!("Y{}", yr),
                Style::default().fg(Color::Gray),
            ),
        ]),
        ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(
                format!("{} ", w.icon()),
                Style::default().fg(w.value_color()).add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(
                format!("{} ", w.category()),
                Style::default().fg(Color::White),
            ),
            ratatui::text::Span::styled(
                w.value(),
                Style::default().fg(w.value_color()).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    // longest plausible line is ~28 chars; pad a couple
    let panel_w = 32u16.min(inner.width);
    let panel_h = (lines.len() as u16).min(inner.height);
    let rect = ratatui::layout::Rect {
        x: inner.x + inner.width.saturating_sub(panel_w),
        y: inner.y,
        width: panel_w,
        height: panel_h,
    };
    use ratatui::widgets::{Clear, Paragraph};
    // Clear wipes each cell's style fully before drawing — otherwise the
    // HUD text inherits whatever modifiers (BOLD, etc.) the underlying
    // world cell had, so digits would flicker bold over tree/wall cells.
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).alignment(ratatui::layout::Alignment::Right),
        rect,
    );
}

// ---- Fishing School skill tree ----------------------------------------

fn render_skill_tree(
    frame: &mut Frame,
    cursor: usize,
    tab: usize,
    tree: &crate::skill_tree::SkillTree,
    fishing_level: u32,
    achievements: u32,
    mastery_milestones: u32,
    encyclopedia_level: u32,
) {
    use crate::skill_tree::{SkillTree, TreeBranch};
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let earned = SkillTree::earned(
        fishing_level,
        achievements,
        mastery_milestones,
        encyclopedia_level,
    );
    let available = tree.available(
        fishing_level,
        achievements,
        mastery_milestones,
        encyclopedia_level,
    );
    let branches = TreeBranch::ALL;
    let tab = tab.min(branches.len() - 1);
    let active_branch = branches[tab];
    let title = format!(
        " fishing school  ({} points)  h/l tab  j/k pick  enter invest  q close ",
        available,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    // Tab strip: highlight the active branch.
    let mut tabspan: Vec<ratatui::text::Span> = Vec::new();
    tabspan.push(ratatui::text::Span::raw("  "));
    for (i, b) in branches.iter().enumerate() {
        let style = if i == tab {
            Style::default()
                .fg(Color::Black)
                .bg(tree_color(*b))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(tree_color(*b))
        };
        tabspan.push(ratatui::text::Span::styled(format!(" {} ", b.label()), style));
        tabspan.push(ratatui::text::Span::raw(" "));
    }
    lines.push(ratatui::text::Line::from(tabspan));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!("  Fishing level: {fishing_level}    Spent: {}/{}", tree.spent, earned),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(""));

    let nodes = crate::skill_tree::nodes_in_tree(active_branch);
    for (i, node) in nodes.iter().enumerate() {
        let rank = tree.rank(&node.id);
        let max = node.max_rank;
        let unlocked = tree.is_unlocked(node);
        let selected = i == cursor;
        let prefix = if selected { "> " } else { "  " };
        let status_color = if !unlocked {
            Color::DarkGray
        } else if rank >= max {
            Color::Green
        } else {
            Color::White
        };
        let line_style = if selected {
            Style::default()
                .bg(Color::Rgb(40, 40, 40))
                .fg(status_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(status_color)
        };
        let pips: String = (0..max)
            .map(|r| if r < rank { '*' } else { '.' })
            .collect();
        let lock = if !unlocked {
            " [LOCKED - invest in a parent first]"
        } else {
            ""
        };
        let label = format!(
            "{prefix}{} [{}] ({}/{}){}",
            node.label, pips, rank, max, lock
        );
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            label, line_style,
        )));
        if selected {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("    {}", node.description),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  j/k navigate, enter to invest 1 point, q/esc to leave",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), inner);
}

fn tree_color(t: crate::skill_tree::TreeBranch) -> Color {
    use crate::skill_tree::TreeBranch::*;
    match t {
        Angler => Color::Rgb(120, 200, 240),
        Naturalist => Color::Rgb(140, 220, 130),
        Mariner => Color::Rgb(110, 170, 240),
        Prospector => Color::Rgb(230, 195, 120),
        Spirit => Color::Rgb(220, 160, 240),
    }
}

// ---- Fishmonger sell menu ---------------------------------------------

fn render_fishmonger(
    frame: &mut Frame,
    cursor: usize,
    step: &FishmongerStep,
    listing: &[(String, u64, u64)],
    valu: u64,
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " fishmonger - basket: {} types, {}$V (q/esc back) ",
            listing.len(),
            valu
        ))
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match step {
        FishmongerStep::PickFish => {
            let mut lines: Vec<ratatui::text::Line> = Vec::new();
            if listing.is_empty() {
                lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                    "  Basket is empty.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for (i, (name, price, count)) in listing.iter().enumerate() {
                    let prefix = if i == cursor { "> " } else { "  " };
                    let style = if i == cursor {
                        Style::default()
                            .bg(Color::Rgb(40, 40, 40))
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    lines.push(ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            format!("{prefix}{name:<32} x{count:<5}"),
                            style,
                        ),
                        ratatui::text::Span::styled(
                            format!("{}$V each", price),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]));
                }
                lines.push(ratatui::text::Line::from(""));
                lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                    "  j/k navigate, enter to choose quantity, q/esc to leave",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            frame.render_widget(Paragraph::new(lines), inner);
        }
        FishmongerStep::PickQuantity { picked, max } => {
            let opts = ["Sell ALL", "Sell ONE", "Sell X (type a number)", "Quit"];
            let mut lines: Vec<ratatui::text::Line> = vec![
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    format!("  How many {picked}? (you have {max})"),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                ratatui::text::Line::from(""),
            ];
            for (i, label) in opts.iter().enumerate() {
                let prefix = if i == cursor { "> " } else { "  " };
                let style = if i == cursor {
                    Style::default()
                        .bg(Color::Rgb(40, 40, 40))
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                    format!("{prefix}{label}"),
                    style,
                )));
            }
            lines.push(ratatui::text::Line::from(""));
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                "  j/k navigate, enter to confirm, q/esc to go back",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(Paragraph::new(lines), inner);
        }
        FishmongerStep::EnterQuantity { picked, max, buf } => {
            let lines = vec![
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    format!("  How many {picked}? (max {max})"),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::raw("    "),
                    ratatui::text::Span::styled(
                        if buf.is_empty() { "_".to_string() } else { buf.clone() },
                        Style::default()
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    "  digits to type, enter to confirm, backspace to delete, esc to go back",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        FishmongerStep::Confirm { picked, qty, total } => {
            let lines = vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    format!("  Sell {qty} {picked} for {total}$V?"),
                    Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
                )),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    "  y / enter to confirm    n / esc to cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}

// ---- The Rod loot pool selector ---------------------------------------

/// (pool_id, label_shown_to_player). Empty pool_id = clear override.
const LOOT_POOLS: &[(&str, &str)] = &[
    ("", "Default (biome / water)"),
    ("forest", "Forest pool"),
    ("desert", "Desert pool"),
    ("tundra", "Tundra pool"),
    ("swamp", "Swamp pool"),
    ("cosmic", "Cosmic (Astral)"),
    ("infernal", "Divine - Infernal"),
    ("angelic", "Divine - Angelic"),
    ("mineral", "Mineral (Sapphire/Ruby/Topaz/Opal/Emerald/Onyx/Diamond)"),
    ("mineral_pool", "Mineral pool (mines water)"),
    ("lava", "Lava (inferno only)"),
    ("atlantis", "Atlantis"),
];

fn render_loot_pool(frame: &mut Frame, cursor: usize, current: Option<&str>) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" THE ROD - select loot pool (j/k browse, enter pick, q close) ")
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines: Vec<ratatui::text::Line> = LOOT_POOLS
        .iter()
        .enumerate()
        .map(|(i, (id, label))| {
            let selected = i == cursor;
            let active = current.map(|c| c == *id).unwrap_or(id.is_empty() && current.is_none());
            let mark = if active { "* " } else { "  " };
            let prefix = if selected { "> " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else if active {
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("{prefix}{mark}{label}"),
                style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

// ---- hidden debug console ---------------------------------------------

/// SHA-512 digest of the magic command string. Anyone reading this source
/// cannot recover the plaintext, only verify a guess. The guard threshold
/// `len > 5` keeps short common commands (`:w`, `:q!`, etc.) out of the
/// hashing path entirely.
const DEBUG_CMD_HASH: &str = "0d3793fad9237a4782b78a431be27eb8ede517670026151fbf94d6d0bbaadfade41aaf3396b325724d1e2d805616dd77ee9ef213392bccd5910cb09a33760e6b";

fn debug_command_matches(input: &str) -> bool {
    use sha2::{Digest, Sha512};
    let mut h = Sha512::new();
    h.update(input.as_bytes());
    let got = h.finalize();
    let hex: String = got.iter().map(|b| format!("{:02x}", b)).collect();
    hex == DEBUG_CMD_HASH
}

/// Editable / actionable rows in the debug console. Order is the row order
/// rendered + the cursor index. Adjust step values are tuned per-row.
#[derive(Clone, Copy)]
enum DebugEntry {
    DimCycle,
    PlayerX,
    PlayerY,
    SnapToWalkable,
    Valu,
    LifetimeValu,
    FishCaught,
    FishEscaped,
    FishSold,
    Casts,
    Steps,
    NpcsTalked,
    QuestsCompleted,
    FishingXp,
    WalkingXp,
    NegotiationXp,
    MiningXp,
    WoodcuttingXp,
    BlacksmithingXp,
    Stamina,
    RodTier,
    MasteryMilestones,
    DailyBonusPoints,
    ChallengeBonusPoints,
    TutorialStep,
    GrantUniqueFish,
    GrantUniqueIsh,
    GrantUniqueFsh,
    GrantUniqueFih,
    GrantUniqueFis,
    GrantUniqueFallen,
    MarkAllSpecies,
    ClearAllSpecies,
    UnlockAllAchievements,
    ClearBounty,
    GrantPickaxe,
    GrantBoat,
    RefillStamina,
    StartBossFight,
    CompleteTutorial,
    ResetTutorial,
}

fn debug_entries() -> &'static [DebugEntry] {
    &[
        DebugEntry::DimCycle,
        DebugEntry::PlayerX,
        DebugEntry::PlayerY,
        DebugEntry::SnapToWalkable,
        DebugEntry::Valu,
        DebugEntry::LifetimeValu,
        DebugEntry::RodTier,
        DebugEntry::Stamina,
        DebugEntry::RefillStamina,
        DebugEntry::FishCaught,
        DebugEntry::FishEscaped,
        DebugEntry::FishSold,
        DebugEntry::Casts,
        DebugEntry::Steps,
        DebugEntry::NpcsTalked,
        DebugEntry::QuestsCompleted,
        DebugEntry::MasteryMilestones,
        DebugEntry::DailyBonusPoints,
        DebugEntry::ChallengeBonusPoints,
        DebugEntry::FishingXp,
        DebugEntry::WalkingXp,
        DebugEntry::NegotiationXp,
        DebugEntry::MiningXp,
        DebugEntry::WoodcuttingXp,
        DebugEntry::BlacksmithingXp,
        DebugEntry::GrantPickaxe,
        DebugEntry::GrantBoat,
        DebugEntry::GrantUniqueFish,
        DebugEntry::GrantUniqueIsh,
        DebugEntry::GrantUniqueFsh,
        DebugEntry::GrantUniqueFih,
        DebugEntry::GrantUniqueFis,
        DebugEntry::GrantUniqueFallen,
        DebugEntry::MarkAllSpecies,
        DebugEntry::ClearAllSpecies,
        DebugEntry::UnlockAllAchievements,
        DebugEntry::ClearBounty,
        DebugEntry::StartBossFight,
        DebugEntry::TutorialStep,
        DebugEntry::CompleteTutorial,
        DebugEntry::ResetTutorial,
    ]
}

fn debug_entries_count() -> usize {
    debug_entries().len()
}

impl App {
    fn debug_adjust(&mut self, cursor: usize, step: i64) {
        use DebugEntry::*;
        let entry = match debug_entries().get(cursor) {
            Some(e) => *e,
            None => return,
        };
        // For value rows the step is a multiplier; for action rows it's a no-op.
        let bump = |v: &mut u64, s: i64, scale: i64| {
            let delta = s.saturating_mul(scale);
            if delta >= 0 {
                *v = v.saturating_add(delta as u64);
            } else {
                *v = v.saturating_sub((-delta) as u64);
            }
        };
        match entry {
            DimCycle => {
                if step != 0 {
                    self.world.dim = cycle_dim(self.world.dim, step);
                    self.snap_player_to_walkable();
                }
            }
            PlayerX => {
                self.player.x = self.player.x.saturating_add(step as i32);
            }
            PlayerY => {
                self.player.y = self.player.y.saturating_add(step as i32);
            }
            Valu => bump(&mut self.player.valu, step, 10_000),
            LifetimeValu => bump(&mut self.lifetime_valu, step, 10_000),
            FishCaught => bump(&mut self.stats.fish_caught, step, 1),
            FishEscaped => bump(&mut self.stats.fish_escaped, step, 1),
            FishSold => bump(&mut self.stats.fish_sold, step, 1),
            Casts => bump(&mut self.stats.casts, step, 1),
            Steps => bump(&mut self.stats.steps, step, 1),
            NpcsTalked => bump(&mut self.stats.npcs_talked, step, 1),
            QuestsCompleted => bump(&mut self.stats.quests_completed, step, 1),
            FishingXp => bump(&mut self.skills.fishing_xp, step, 100),
            WalkingXp => bump(&mut self.skills.walking_xp, step, 100),
            NegotiationXp => bump(&mut self.skills.negotiation_xp, step, 100),
            MiningXp => bump(&mut self.skills.mining_xp, step, 100),
            WoodcuttingXp => bump(&mut self.skills.woodcutting_xp, step, 100),
            BlacksmithingXp => bump(&mut self.skills.blacksmithing_xp, step, 100),
            Stamina => {
                let delta = (step as f32) * 10.0;
                let max = self.stamina_max();
                self.stamina = (self.stamina + delta).clamp(0.0, max);
            }
            RodTier => {
                let cur = self.player.rods.max_owned as i64;
                let next = (cur + step).clamp(1, crate::rod::rods().len() as i64) as u32;
                self.player.rods.max_owned = next;
                if self.player.rods.equipped > next {
                    self.player.rods.equipped = next;
                }
            }
            MasteryMilestones => {
                let v = self.mastery_milestones as i64 + step;
                self.mastery_milestones = v.max(0) as u32;
            }
            DailyBonusPoints => {
                let v = self.daily_bonus_points as i64 + step;
                self.daily_bonus_points = v.max(0) as u32;
            }
            ChallengeBonusPoints => {
                let v = self.challenge_bonus_points as i64 + step;
                self.challenge_bonus_points = v.max(0) as u32;
            }
            TutorialStep => {
                let v = self.tutorial_step as i64 + step;
                self.tutorial_step = v.clamp(0, TUTORIAL_STEPS as i64) as u32;
            }
            _ => {}
        }
    }

    fn debug_action(&mut self, cursor: usize) {
        use DebugEntry::*;
        let entry = match debug_entries().get(cursor) {
            Some(e) => *e,
            None => return,
        };
        match entry {
            GrantUniqueFish => self.grant_unique("Fish", "Debug console"),
            GrantUniqueIsh => self.grant_unique("Ish", "Debug console"),
            GrantUniqueFsh => self.grant_unique("Fsh", "Debug console"),
            GrantUniqueFih => self.grant_unique("Fih", "Debug console"),
            GrantUniqueFis => self.grant_unique("Fis", "Debug console"),
            GrantUniqueFallen => self.grant_unique("Fallen Fish", "Debug console"),
            MarkAllSpecies => {
                for c in self.caught.iter_mut() {
                    *c = true;
                }
                self.narrator.say("Debug: marked every species caught.");
            }
            ClearAllSpecies => {
                for c in self.caught.iter_mut() {
                    *c = false;
                }
                for s in self.caught_at.iter_mut() {
                    *s = None;
                }
                self.narrator.say("Debug: cleared fishdex.");
            }
            DimCycle => {
                self.world.dim = cycle_dim(self.world.dim, 1);
                self.snap_player_to_walkable();
            }
            SnapToWalkable => {
                self.snap_player_to_walkable();
                self.narrator
                    .say(format!("Debug: snapped to ({}, {}).", self.player.x, self.player.y));
            }
            PlayerX | PlayerY => {
                // adjust-only rows; Enter is a no-op so the user can dial in
                // coords without surprises. (SnapToWalkable handles unstucking.)
            }
            UnlockAllAchievements => {
                for chain in crate::achievements::chains() {
                    for (i, tier) in chain.tiers.iter().enumerate() {
                        let key = format!("{}:{}", chain.id, i + 1);
                        if !self.achievements.unlocked.contains(&key) {
                            self.achievements.unlocked.push(key);
                            self.achievements.points_granted =
                                self.achievements.points_granted.saturating_add(tier.reward_points);
                        }
                    }
                }
                self.narrator.say("Debug: every achievement tier unlocked.");
            }
            ClearBounty => {
                self.bounty = None;
                self.narrator.say("Debug: bounty cleared.");
            }
            GrantPickaxe => {
                self.player.has_pickaxe = true;
                self.narrator.say("Debug: pickaxe granted.");
            }
            GrantBoat => {
                self.player.has_boat = true;
                self.narrator.say("Debug: boat granted.");
            }
            RefillStamina => {
                self.stamina = self.stamina_max();
                self.narrator.say("Debug: stamina refilled.");
            }
            StartBossFight => {
                let pool: Vec<usize> = self
                    .caught
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c)
                    .map(|(i, _)| i)
                    .collect();
                if pool.len() < 2 {
                    // Fall back to the two hardest fish in the catalog.
                    let mut sorted: Vec<&'static crate::fish::FishDef> =
                        crate::fishlist::fish().iter().collect();
                    sorted.sort_by_key(|f| std::cmp::Reverse(f.difficulty));
                    if sorted.len() < 2 {
                        return;
                    }
                    self.scene = Scene::Boss(crate::boss::Boss::new(
                        sorted[0],
                        sorted[1],
                        self.rng_state,
                        self.skills.fishing_level(),
                    ));
                } else {
                    let r1 = crate::fish::next_rand_f32(&mut self.rng_state);
                    let r2 = crate::fish::next_rand_f32(&mut self.rng_state);
                    let a = pool[((r1 * pool.len() as f32) as usize).min(pool.len() - 1)];
                    let b = pool[((r2 * pool.len() as f32) as usize).min(pool.len() - 1)];
                    self.scene = Scene::Boss(crate::boss::Boss::new(
                        &crate::fishlist::fish()[a],
                        &crate::fishlist::fish()[b],
                        self.rng_state,
                        self.skills.fishing_level(),
                    ));
                }
                self.mode = Mode::Insert;
            }
            CompleteTutorial => {
                self.tutorial_step = TUTORIAL_STEPS;
                self.narrator.say("Debug: tutorial marked complete.");
            }
            ResetTutorial => {
                self.tutorial_step = 0;
                self.narrator.say("Debug: tutorial reset.");
            }
            _ => {}
        }
    }
}

/// Cycle through every Dimension in declaration order. Step direction picks
/// next (positive) or previous (negative).
fn cycle_dim(cur: crate::world::Dimension, step: i64) -> crate::world::Dimension {
    use crate::world::Dimension::*;
    const ORDER: &[crate::world::Dimension] = &[
        Surface, Mines, Atlantis, Inferno,
        Sewer, HotSpring, Pyramid, SwampCave, BogCathedral, MirrorLake,
        Iceshelf, Wreckage, Crater, Colosseum, AllBlue,
    ];
    let idx = ORDER.iter().position(|d| *d == cur).unwrap_or(0);
    let n = ORDER.len() as i64;
    let next = ((idx as i64 + step.signum()).rem_euclid(n)) as usize;
    ORDER[next]
}

fn render_debug_console(
    frame: &mut Frame,
    cursor: usize,
    valu: u64,
    dim: crate::world::Dimension,
    stats: &Stats,
    skills: &Skills,
    _buffs: &crate::buffs::Buffs,
    player_xy: (i32, i32),
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" developer console - h/l adjust, H/L big step, enter action, q/esc close ")
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let dim_label = dim.label();
    let rows: Vec<(String, String)> = debug_entries()
        .iter()
        .map(|e| match e {
            DebugEntry::DimCycle => ("Dimension (h/l/enter cycles)".to_string(), dim_label.to_string()),
            DebugEntry::PlayerX => ("Player X (h/l \u{00B1}1, H/L \u{00B1}100)".to_string(), player_xy.0.to_string()),
            DebugEntry::PlayerY => ("Player Y (h/l \u{00B1}1, H/L \u{00B1}100)".to_string(), player_xy.1.to_string()),
            DebugEntry::SnapToWalkable => {
                ("[enter] Snap to nearest walkable".to_string(), String::new())
            }
            DebugEntry::Valu => ("Valu".to_string(), valu.to_string()),
            DebugEntry::LifetimeValu => {
                ("Lifetime valu earned".to_string(), stats.valu_earned.to_string())
            }
            DebugEntry::FishCaught => ("Fish caught".to_string(), stats.fish_caught.to_string()),
            DebugEntry::FishEscaped => ("Fish escaped".to_string(), stats.fish_escaped.to_string()),
            DebugEntry::FishSold => ("Fish sold".to_string(), stats.fish_sold.to_string()),
            DebugEntry::Casts => ("Casts".to_string(), stats.casts.to_string()),
            DebugEntry::Steps => ("Steps walked".to_string(), stats.steps.to_string()),
            DebugEntry::NpcsTalked => ("NPCs talked".to_string(), stats.npcs_talked.to_string()),
            DebugEntry::QuestsCompleted => {
                ("Quests completed".to_string(), stats.quests_completed.to_string())
            }
            DebugEntry::FishingXp => ("Fishing XP".to_string(), skills.fishing_xp.to_string()),
            DebugEntry::WalkingXp => ("Walking XP".to_string(), skills.walking_xp.to_string()),
            DebugEntry::NegotiationXp => {
                ("Negotiation XP".to_string(), skills.negotiation_xp.to_string())
            }
            DebugEntry::MiningXp => ("Mining XP".to_string(), skills.mining_xp.to_string()),
            DebugEntry::WoodcuttingXp => {
                ("Woodcutting XP".to_string(), skills.woodcutting_xp.to_string())
            }
            DebugEntry::BlacksmithingXp => {
                ("Blacksmithing XP".to_string(), skills.blacksmithing_xp.to_string())
            }
            DebugEntry::GrantUniqueFish => ("[enter] Grant Fish".to_string(), String::new()),
            DebugEntry::GrantUniqueIsh => ("[enter] Grant Ish".to_string(), String::new()),
            DebugEntry::GrantUniqueFsh => ("[enter] Grant Fsh".to_string(), String::new()),
            DebugEntry::GrantUniqueFih => ("[enter] Grant Fih".to_string(), String::new()),
            DebugEntry::GrantUniqueFis => ("[enter] Grant Fis".to_string(), String::new()),
            DebugEntry::GrantUniqueFallen => {
                ("[enter] Grant Fallen Fish".to_string(), String::new())
            }
            DebugEntry::MarkAllSpecies => {
                ("[enter] Mark every species caught".to_string(), String::new())
            }
            DebugEntry::ClearAllSpecies => {
                ("[enter] Clear fishdex".to_string(), String::new())
            }
            DebugEntry::Stamina => ("Stamina".to_string(), "h/l \u{00B1} 10".to_string()),
            DebugEntry::RodTier => ("Rod tier (max owned)".to_string(), String::new()),
            DebugEntry::MasteryMilestones => {
                ("Mastery milestones".to_string(), String::new())
            }
            DebugEntry::DailyBonusPoints => {
                ("Daily bonus points".to_string(), String::new())
            }
            DebugEntry::ChallengeBonusPoints => {
                ("Challenge bonus points".to_string(), String::new())
            }
            DebugEntry::TutorialStep => ("Tutorial step (0..6)".to_string(), String::new()),
            DebugEntry::UnlockAllAchievements => {
                ("[enter] Unlock all achievements".to_string(), String::new())
            }
            DebugEntry::ClearBounty => ("[enter] Clear active bounty".to_string(), String::new()),
            DebugEntry::GrantPickaxe => ("[enter] Grant pickaxe".to_string(), String::new()),
            DebugEntry::GrantBoat => ("[enter] Grant boat".to_string(), String::new()),
            DebugEntry::RefillStamina => ("[enter] Refill stamina to max".to_string(), String::new()),
            DebugEntry::StartBossFight => {
                ("[enter] Start boss fight (2 random caught)".to_string(), String::new())
            }
            DebugEntry::CompleteTutorial => {
                ("[enter] Mark tutorial complete".to_string(), String::new())
            }
            DebugEntry::ResetTutorial => ("[enter] Reset tutorial".to_string(), String::new()),
        })
        .collect();
    let lines: Vec<ratatui::text::Line> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let prefix = if i == cursor { "> " } else { "  " };
            let style = if i == cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(format!("{prefix}{:<32}", label), style),
                ratatui::text::Span::styled(value, Style::default().fg(Color::LightYellow)),
            ])
        })
        .collect();
    // Slide the visible window so the cursor row stays on screen. Without
    // this, the bottom rows clip off any terminal shorter than ~40 lines.
    let total = lines.len();
    let visible = inner.height as usize;
    let scroll: u16 = if visible == 0 || total <= visible {
        0
    } else if cursor < visible / 2 {
        0
    } else if cursor + (visible - visible / 2) >= total {
        (total - visible) as u16
    } else {
        (cursor - visible / 2) as u16
    };
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// True if the player can ride a boat onto this tile. (Swimming isn't a
/// thing — fish are dangerous. Only a boat lets you cross water.)
/// Normalize an item-name string into the canonical ore name used in
/// `mining::ORES`. Ore items are pushed with `ore.name` already, so this
/// just lowercases and trims as a defensive measure against case drift.
fn canonical_ore_name(s: &str) -> &'static str {
    let lower = s.trim().to_ascii_lowercase();
    for ore in crate::mining::ORES.iter() {
        if ore.name == lower {
            return ore.name;
        }
    }
    ""
}

fn is_boatable(t: Tile) -> bool {
    matches!(t, Tile::Water | Tile::DeepWater | Tile::Seabed | Tile::Kelp | Tile::Anemone)
}

// Re-exported for legacy import paths inside this module.
use crate::world::ocean_depth_at;

fn water_kind_at(world: &World, x: i32, y: i32) -> &'static str {
    let t = world.get(x, y);
    if matches!(t, Tile::Well) {
        return "well";
    }
    if matches!(t, Tile::Dock) {
        // dock cells are over ocean
        return "ocean";
    }
    if matches!(t, Tile::MineralWater) {
        return "mineral_pool";
    }
    if matches!(t, Tile::Lava) {
        return "lava";
    }
    if matches!(t, Tile::DeepWater | Tile::Seabed | Tile::Kelp | Tile::Anemone) {
        return "atlantis";
    }
    if y >= 5 {
        return "ocean";
    }
    "lake"
}

/// Derive (water_kind, biome_label) for a fishing cast at (x, y) in the
/// current dimension. Surface uses real biome+water. Mines and Atlantis
/// short-circuit to their own pseudo-biome labels.
/// Default pool to draw from based on the dim + tile + the day's weather.
/// In the Inferno the Temperature decides Hot/Burning/Infernal. In the
/// Mines, Tectonic High forces mineral; Medium gives a 50% mineral chance;
/// Low only yields mineral when fishing actual mineral water.
fn dim_default_pool(
    dim: crate::world::Dimension,
    tile: Tile,
    weather: crate::weather::Weather,
    rng: &mut u32,
    cell: (i32, i32),
    seed: u32,
) -> Option<&'static str> {
    use crate::weather::Weather;
    match dim {
        crate::world::Dimension::Mines => {
            // Lakebed cave water: route to the special "lakebed" pool
            // where the Fallen Fish swims. Strictly per-cell so the player
            // is rewarded for fishing in a flooded zone, not normal mines.
            if matches!(tile, Tile::MineralWater)
                && crate::world::lakebed_region(cell.0, cell.1, seed)
            {
                return Some("lakebed");
            }
            match weather {
                Weather::TectonicHigh => Some("mineral"),
                Weather::TectonicMedium => {
                    if crate::fish::next_rand_f32(rng) < 0.5 {
                        Some("mineral")
                    } else if matches!(tile, Tile::MineralWater) {
                        Some("mineral")
                    } else {
                        None
                    }
                }
                _ => {
                    if matches!(tile, Tile::MineralWater) {
                        Some("mineral")
                    } else {
                        None
                    }
                }
            }
        }
        crate::world::Dimension::Inferno => match weather {
            Weather::TempLow => Some("hot"),
            Weather::TempMedium => Some("burning"),
            Weather::TempHigh => Some("infernal"),
            _ => Some("hot"),
        },
        // ---- specialty dim → pool tag ----
        // On a "High" weather day the dim's weather can override the pool
        // (e.g. Cathedral on Supernatural-High routes to "divine" instead
        // of the default cathedral pool — see weather::weather_modifiers).
        crate::world::Dimension::Sewer => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("sewer")),
        crate::world::Dimension::HotSpring => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("hotspring")),
        crate::world::Dimension::Pyramid => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("pyramid")),
        crate::world::Dimension::SwampCave => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("swampcave")),
        crate::world::Dimension::BogCathedral => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("cathedral")),
        crate::world::Dimension::MirrorLake => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("mirrorlake")),
        crate::world::Dimension::Iceshelf => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("iceshelf")),
        crate::world::Dimension::Wreckage => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("wreckage")),
        crate::world::Dimension::Crater => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("crater")),
        crate::world::Dimension::Colosseum => Some(crate::weather::weather_modifiers(weather).pool_override.unwrap_or("colosseum")),
        crate::world::Dimension::Lakebed => Some("lakebed"),
        // All Blue is the endgame "everything" pool — we route to "allblue"
        // for the rare apex fish AND occasionally fall back to None so the
        // entire fish list is reachable (the picker handles None).
        crate::world::Dimension::AllBlue => {
            if crate::fish::next_rand_f32(rng) < 0.3 {
                Some("allblue")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn fishing_context(world: &World, x: i32, y: i32) -> (&'static str, String) {
    match world.dim {
        crate::world::Dimension::Surface => {
            let water = water_kind_at(world, x, y);
            // Far-offshore casts ride the Fog Sea pseudo-biome: anything
            // past the hull tier-5 depth limit. The picker treats this as
            // a unique biome label so future ghost-pool routing in
            // `dim_default_pool` can lock on.
            if water == "ocean" && ocean_depth_at(world, x, y) > 32 {
                (water, "Fog Sea".to_string())
            } else {
                (water, biome_at(x, y, world.seed).label().to_string())
            }
        }
        crate::world::Dimension::Mines => ("mineral_pool", "Mines".to_string()),
        crate::world::Dimension::Atlantis => ("atlantis", "Atlantis".to_string()),
        crate::world::Dimension::Inferno => ("lava", "Inferno".to_string()),
        // specialty dims: pass the dim label as the biome and let the fish
        // picker fall back to default water type filtering.
        other => ("any", other.label().to_string()),
    }
}

fn direction_for(code: KeyCode) -> Option<(i32, i32)> {
    match code {
        KeyCode::Char('h') | KeyCode::Char('a') | KeyCode::Left => Some((-1, 0)),
        KeyCode::Char('j') | KeyCode::Char('s') | KeyCode::Down => Some((0, 1)),
        KeyCode::Char('k') | KeyCode::Char('w') | KeyCode::Up => Some((0, -1)),
        KeyCode::Char('l') | KeyCode::Char('d') | KeyCode::Right => Some((1, 0)),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_stats(
    frame: &mut Frame,
    name: &str,
    valu: u64,
    lifetime_valu: u64,
    unique_caught: usize,
    total_species: usize,
    fish_in_basket: usize,
    items_picked: usize,
    quests_done: usize,
    play_secs: u64,
    stats: &Stats,
    skills: &Skills,
    buffs: &crate::buffs::Buffs,
    gear: &crate::gear::EquippedGear,
    ingot_count: u32,
    fish_today: u32, fish_cap: u32,
    ore_today: u32, ore_cap: u32,
    recipes_discovered: usize,
    recipes_mastered: usize,
    total_recipes: usize,
    wood: u32,
    hull_tier: u32,
    biofuel: u32,
    crew_hunger: u32,
    skill_points_available: u32,
    skill_points_earned: u32,
) {
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" stats (q/esc to close) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let h = play_secs / 3600;
    let m = (play_secs % 3600) / 60;
    let s = play_secs % 60;
    let play = if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    };

    let who = if name.is_empty() { "angler" } else { name };

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(section("PROFILE"));
    lines.push(row("Name", who.to_string()));
    lines.push(row("Play time", play));
    lines.push(row("Valu", format_valu(valu)));
    lines.push(row("Lifetime valu earned", format_valu(lifetime_valu)));
    lines.push(row(
        "Skill points",
        format!(
            "{skill_points_available} available / {skill_points_earned} earned (visit the school)"
        ),
    ));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("PROGRESS"));
    let fishdex_pct = if total_species > 0 {
        (unique_caught * 100) / total_species
    } else {
        0
    };
    let recipe_disc_pct = if total_recipes > 0 {
        (recipes_discovered * 100) / total_recipes
    } else {
        0
    };
    lines.push(row(
        "Fishdex",
        format!("{}/{} species ({}%)", unique_caught, total_species, fishdex_pct),
    ));
    lines.push(row(
        "Cookbook discovered",
        format!(
            "{}/{} recipes ({}%)",
            recipes_discovered, total_recipes, recipe_disc_pct
        ),
    ));
    lines.push(row(
        "Cookbook mastered",
        format!("{}/{} (≥5 cooks)", recipes_mastered, total_recipes),
    ));
    lines.push(row("Fish in basket", fish_in_basket.to_string()));
    lines.push(row("Items picked up", items_picked.to_string()));
    lines.push(row("Quests completed", quests_done.to_string()));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("BOAT"));
    lines.push(row(
        "Hull",
        format!("tier {} — {}", hull_tier, crate::player::hull_label(hull_tier)),
    ));
    let max_depth = crate::player::ocean_depth_max(hull_tier);
    let depth_label = if max_depth == u32::MAX {
        "∞ (Fog Sea unlocked)".to_string()
    } else {
        format!("{} tiles offshore", max_depth)
    };
    lines.push(row("Reach", depth_label));
    lines.push(row("Biofuel", format!("{}/200", biofuel)));
    lines.push(row("Crew hunger", format!("{}/100", crew_hunger)));
    lines.push(row("Wood stash", wood.to_string()));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("ACTIVITY"));
    lines.push(row("Steps taken", stats.steps.to_string()));
    lines.push(row("Casts", stats.casts.to_string()));
    lines.push(row("Fish caught (lifetime)", stats.fish_caught.to_string()));
    lines.push(row("Fish escaped", stats.fish_escaped.to_string()));
    lines.push(row("Fish sold", stats.fish_sold.to_string()));
    lines.push(row("NPCs talked to", stats.npcs_talked.to_string()));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("SKILLS"));
    let entries = [
        ("Fishing", skills.fishing_level(), skills.fishing_xp),
        ("Walking", skills.walking_level(), skills.walking_xp),
        ("Negotiation", skills.negotiation_level(), skills.negotiation_xp),
        ("Mining", skills.mining_level(), skills.mining_xp),
        ("Woodcutting", skills.woodcutting_level(), skills.woodcutting_xp),
        ("Blacksmithing", skills.blacksmithing_level(), skills.blacksmithing_xp),
        ("Cooking", skills.cooking_level(), skills.cooking_xp),
        ("Encyclopedia", skills.encyclopedia_level(), skills.encyclopedia_xp),
    ];
    for (label, lvl, xp) in entries {
        let next = crate::stats::level_to_xp(lvl + 1);
        lines.push(row(
            label,
            format!("lv {lvl}  ({xp}/{next} xp)"),
        ));
    }

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("BUFFS"));
    lines.push(row(
        "Sell-price mult",
        format!("x{:.2}", buffs.price_mult()),
    ));
    lines.push(row("Free rods banked", buffs.free_rods.to_string()));
    lines.push(row(
        "Cast range bonus",
        format!("+{}", buffs.bobber_range_bonus),
    ));
    lines.push(row(
        "Wait time mult",
        format!("x{:.2}", buffs.wait_mult()),
    ));
    // Walk-speed: combine buffs.walk_mult (skill-tree + tackle) with gear's
    // move_speed_mult (boots), since both shorten the per-step interval.
    let combined_walk = (buffs.walk_mult() * gear.combined_perks().move_speed_mult).max(0.01);
    lines.push(row(
        "Walk speed (overall)",
        format!("x{:.2} faster", 1.0 / combined_walk),
    ));
    lines.push(row(
        "Luck bonus",
        format!("+{:.0}%", buffs.luck_bonus * 100.0),
    ));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("EQUIPPED GEAR"));
    let slots = [
        ("Feet", gear.feet.as_deref()),
        ("Neck", gear.neck.as_deref()),
        ("Ring", gear.ring.as_deref()),
        ("Pickaxe", gear.pickaxe.as_deref()),
        ("Cape", gear.cape.as_deref()),
    ];
    for (label, id_opt) in slots {
        let val = match id_opt {
            None => "(empty)".to_string(),
            Some(id) if id.starts_with("cape-of-") => {
                // synthetic cape id format: cape-of-{N}-memories
                let n = id
                    .strip_prefix("cape-of-")
                    .and_then(|r| r.strip_suffix("-memories"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                format!("Cape of {n} Memories")
            }
            Some(id) => crate::gear::def_by_id(id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| id.to_string()),
        };
        lines.push(row(label, val));
    }
    let perks = gear.combined_perks();
    // (combined walk-speed already shown in BUFFS — gear's component is folded in there)
    lines.push(row(
        "Stamina drain mult",
        format!("x{:.2}", perks.stamina_loss_mult),
    ));
    lines.push(row(
        "Double-fish chance",
        format!("{:.1}%", perks.double_fish_chance * 100.0),
    ));
    lines.push(row(
        "Bait-skip chance",
        format!("{:.0}%", perks.no_bait_consume_chance * 100.0),
    ));
    lines.push(row(
        "Ore letters prewritten",
        perks.ore_prewrite_letters.to_string(),
    ));
    lines.push(row(
        "Owned ingots",
        ingot_count.to_string(),
    ));

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("MERCHANT CAPS (today)"));
    lines.push(row(
        "Fishmonger",
        format!("{fish_today}/{fish_cap} sold"),
    ));
    lines.push(row(
        "Blacksmith",
        format!("{ore_today}/{ore_cap} sold"),
    ));

    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn section(title: &str) -> ratatui::text::Line<'static> {
    ratatui::text::Line::from(ratatui::text::Span::styled(
        format!("  {}", title),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn row(key: &str, val: String) -> ratatui::text::Line<'static> {
    ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            format!("    {:<22}", key),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        ratatui::text::Span::raw(val),
    ])
}

fn render_settings(frame: &mut Frame, cursor: usize, s: &Settings) {
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" settings  (j/k move, h/l adjust, q/esc close) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows: [(&str, String); 3] = [
        ("autosave every (sec)", s.autosave_interval_secs.to_string()),
        ("log lines (5..15)", s.log_lines.to_string()),
        ("high contrast", if s.high_contrast { "on".into() } else { "off".into() }),
    ];
    let mut body: Vec<ratatui::text::Line> = vec![ratatui::text::Line::from("")];
    for (i, (k, v)) in rows.iter().enumerate() {
        let style = if i == cursor {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::LightYellow)
        };
        body.push(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(format!("  {:<26}", k), style),
            ratatui::text::Span::raw(format!("< {} >", v)),
        ]));
    }
    body.push(ratatui::text::Line::from(""));
    body.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  saves: ./saves/  (latest + 3 backups, + 3 redundancy copies)",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        Paragraph::new(body).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_help(frame: &mut Frame, topic: HelpTopic) {
    let area = viewport(frame);
    let (title, lines): (&str, Vec<(&str, &str)>) = match topic {
        HelpTopic::Controls => (
            " controls (q/esc to close) ",
            vec![
                ("hjkl / wasd / arrows", "move (and turn to face)"),
                ("f", "interact with what you're facing (door, npc, water)"),
                ("g", "pick up nearby flower / pebble"),
                ("x", "inspect the tile you're facing"),
                ("e", "open fishdex (catch list, /-filter)"),
                ("space", "cast / set strength / hook on ! / cancel while waiting"),
                ("Esc", "switch from Insert -> Normal mode (also cancels a cast)"),
                ("i / a", "switch from Normal -> Insert mode"),
                (":", "in Normal mode, open command line"),
                ("discovery banner", "fish/recipe first-catches show top of screen (yellow / magenta)"),
                ("lakebed entrance", "blue 'V' A-frame on a lake island. dry mineshafts are brown '#'"),
            ],
        ),
        HelpTopic::Commands => (
            " :commands (q/esc to close) ",
            vec![
                (":w", "save"),
                (":wq / :x", "save and quit"),
                (":q", "save and quit"),
                (":q!", "quit without saving"),
                (":e", "open fishdex"),
                (":n  / :notes", "open notebook editor"),
                (":i  / :inv", "open inventory"),
                (":t  / :tasks", "open tasks menu"),
                (":c  / :controls", "show in-game controls"),
                (":help", "show this list"),
                (":s  / :stats", "stats screen"),
                (":m  / :map", "open the explored world map"),
                (":o  / :options", "settings"),
                (":gear / :eq", "manage equipped feet/neck/ring/pickaxe gear"),
                (":smelt", "blacksmith: smelt ore → ingots (must be near a smith)"),
                (":forge", "blacksmith: forge equipment (must be near a smith)"),
                (":sellore / :sell-ore", "dump ore + ingots to the smith (capped per day)"),
                (":chop", "chop the tree you're facing (woodcutting)"),
                (":feed [n]", "feed n fish to the crew (-3 hunger each)"),
                (":burn [n]", "burn n fish for biofuel (+5 × difficulty each)"),
                (":shipwright", "open the hull-upgrade menu"),
                (":cook / :cookbook", "open the recipe encyclopedia (must be at a cooking pot)"),
                (":cook <fish>", "quick: cook one fish for stamina (must be at a cooking pot)"),
            ],
        ),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let key_w: u16 = 20;
    let body: Vec<ratatui::text::Line> = lines
        .into_iter()
        .map(|(k, v)| {
            ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("  {:<width$}", k, width = key_w as usize),
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                ),
                ratatui::text::Span::raw(v.to_string()),
            ])
        })
        .collect();
    let p = Paragraph::new(body).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn group_line_full(
    name: &str,
    desc: &str,
    n: usize,
    recipes: usize,
    sell_price: u64,
) -> ratatui::text::Line<'static> {
    let label = if n > 1 {
        format!("({n}) {name}")
    } else {
        name.to_string()
    };
    let mut spans = vec![
        ratatui::text::Span::styled(label, Style::default().fg(Color::LightYellow)),
        ratatui::text::Span::raw("  "),
        ratatui::text::Span::styled(
            format!("{sell_price}$V/ea"),
            Style::default().fg(Color::LightGreen),
        ),
        ratatui::text::Span::raw("  - "),
        ratatui::text::Span::raw(desc.to_string()),
    ];
    if recipes > 0 {
        spans.push(ratatui::text::Span::styled(
            format!("   [in {recipes} recipe{}]", if recipes == 1 { "" } else { "s" }),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    ratatui::text::Line::from(spans)
}

fn group_line(name: &str, desc: &str, n: usize) -> ratatui::text::Line<'static> {
    let label = if n > 1 {
        format!("({n}) {name}")
    } else {
        name.to_string()
    };
    ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            label,
            Style::default().fg(Color::LightYellow),
        ),
        ratatui::text::Span::raw("  - "),
        ratatui::text::Span::raw(desc.to_string()),
    ])
}

fn render_inventory(
    frame: &mut Frame,
    fish_inv: &[&'static FishDef],
    items: &[Item],
    tab_idx: usize,
    recipe_discovered: &[bool],
) {
    let area = viewport(frame);
    let cats = Category::all();
    let cat = cats[tab_idx.min(cats.len() - 1)];
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " inventory - {} (h/l or arrows to switch, q to leave) ",
            cat.label()
        ))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let mut tab_spans: Vec<ratatui::text::Span> = Vec::with_capacity(cats.len() * 2);
    for (i, c) in cats.iter().enumerate() {
        let style = if i == tab_idx {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tab_spans.push(ratatui::text::Span::styled(
            format!(" {} ", c.label()),
            style,
        ));
        tab_spans.push(ratatui::text::Span::raw(" "));
    }
    let tab_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(tab_spans)),
        tab_area,
    );

    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    let lines: Vec<ratatui::text::Line> = match cat {
        Category::Fish => {
            // Collect (name, desc, count, base_sell_price). Group like before
            // but keep a pointer to the original FishDef so we can read
            // sell_price() per row.
            let mut grouped: Vec<(&'static crate::fish::FishDef, usize)> = Vec::new();
            for f in fish_inv.iter().filter(|f| !f.unique) {
                if let Some((_, n)) = grouped
                    .iter_mut()
                    .find(|(ff, _)| ff.name == f.name)
                {
                    *n += 1;
                } else {
                    grouped.push((*f, 1));
                }
            }
            grouped
                .into_iter()
                .map(|(f, n)| {
                    let recipe_count = crate::recipes::recipes()
                        .iter()
                        .enumerate()
                        .filter(|(i, r)| {
                            recipe_discovered.get(*i).copied().unwrap_or(false)
                                && r.ingredients
                                    .iter()
                                    .any(|(in_name, _)| in_name.eq_ignore_ascii_case(&f.name))
                        })
                        .count();
                    group_line_full(&f.name, &f.description, n, recipe_count, f.sell_price())
                })
                .collect()
        }
        Category::Misc => {
            // Unique fish like THE FISH live in Misc so they can't be sold or
            // accidentally treated as catch. Show them first, deduplicated.
            let mut grouped: Vec<(&str, &str, usize)> = Vec::new();
            for f in fish_inv.iter().filter(|f| f.unique) {
                if !grouped.iter().any(|(n, _, _)| *n == f.name.as_str()) {
                    grouped.push((f.name.as_str(), f.description.as_str(), 1));
                }
            }
            for it in items.iter().filter(|it| it.category == Category::Misc) {
                if let Some((_, _, n)) = grouped.iter_mut().find(|(n, _, _)| *n == it.name.as_str()) {
                    *n += 1;
                } else {
                    grouped.push((it.name.as_str(), it.description.as_str(), 1));
                }
            }
            grouped
                .into_iter()
                .map(|(name, desc, n)| group_line(name, desc, n))
                .collect()
        }
        other => {
            let mut grouped: Vec<(&str, &str, usize)> = Vec::new();
            for it in items.iter().filter(|it| it.category == other) {
                if let Some((_, _, n)) = grouped.iter_mut().find(|(n, _, _)| *n == it.name.as_str()) {
                    *n += 1;
                } else {
                    grouped.push((it.name.as_str(), it.description.as_str(), 1));
                }
            }
            grouped
                .into_iter()
                .map(|(name, desc, n)| group_line(name, desc, n))
                .collect()
        }
    };
    let body = if lines.is_empty() {
        vec![ratatui::text::Line::from(ratatui::text::Span::styled(
            "(empty)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        lines
    };
    frame.render_widget(
        Paragraph::new(body).wrap(ratatui::widgets::Wrap { trim: false }),
        list_area,
    );
}

fn render_notes(frame: &mut Frame, buf: &NotesBuf) {
    let area = viewport(frame);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" notebook (esc to save & leave) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines: Vec<ratatui::text::Line> = buf
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == buf.cursor_row {
                let col = buf.cursor_col.min(l.chars().count());
                let mut chars: Vec<char> = l.chars().collect();
                if col >= chars.len() {
                    chars.push(' ');
                }
                let pre: String = chars[..col].iter().collect();
                let at = chars[col];
                let post: String = chars[col + 1..].iter().collect();
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::raw(pre),
                    ratatui::text::Span::styled(
                        at.to_string(),
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::White),
                    ),
                    ratatui::text::Span::raw(post),
                ])
            } else {
                ratatui::text::Line::from(l.as_str())
            }
        })
        .collect();
    let p = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_house(frame: &mut Frame, px: i32, py: i32, seed: u32, facing: (i32, i32)) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" house ")
        .border_style(Style::default().fg(Color::Rgb(180, 150, 110)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Center the room inside `inner`.
    let room_w = crate::house::WIDTH as u16;
    let room_h = crate::house::HEIGHT as u16;
    if inner.width < room_w || inner.height < room_h {
        return;
    }
    let ox = inner.x + (inner.width - room_w) / 2;
    let oy = inner.y + (inner.height - room_h) / 2;

    let buf = frame.buffer_mut();
    for y in 0..crate::house::HEIGHT {
        for x in 0..crate::house::WIDTH {
            let f = crate::house::tile_at(x, y, seed);
            let (g, style) = furn_style(f);
            let sx = ox + x as u16;
            let sy = oy + y as u16;
            buf[(sx, sy)].set_char(g).set_style(style);
        }
    }
    // Player on top — facing-direction glyph matches the overworld.
    let sx = ox + px as u16;
    let sy = oy + py as u16;
    let glyph = match facing {
        (0, -1) => '^',
        (0, 1) => 'v',
        (-1, 0) => '<',
        (1, 0) => '>',
        _ => '@',
    };
    buf[(sx, sy)]
        .set_char(glyph)
        .set_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
}

fn furn_style(f: crate::house::Furn) -> (char, Style) {
    use crate::house::Furn;
    let (g, fg) = match f {
        Furn::Floor => ('.', Color::Rgb(120, 95, 70)),
        Furn::Wall => ('#', Color::Rgb(180, 145, 95)),
        Furn::Window => ('O', Color::Rgb(170, 210, 240)),
        Furn::Bed => ('=', Color::Rgb(200, 180, 150)),
        Furn::Pillow => ('*', Color::Rgb(240, 230, 210)),
        Furn::Stove => ('@', Color::Rgb(200, 90, 60)),
        Furn::Counter => ('~', Color::Rgb(170, 130, 90)),
        Furn::Sink => ('U', Color::Rgb(190, 200, 215)),
        Furn::Table => ('T', Color::Rgb(150, 105, 70)),
        Furn::Chair => ('h', Color::Rgb(150, 105, 70)),
        Furn::Rug => (',', Color::Rgb(130, 80, 75)),
        Furn::Exit => ('D', Color::Rgb(245, 215, 90)),
    };
    let mut style = Style::default().fg(fg);
    if matches!(
        f,
        Furn::Wall | Furn::Stove | Furn::Sink | Furn::Window | Furn::Exit
    ) {
        style = style.add_modifier(Modifier::BOLD);
    }
    (g, style)
}

fn render_achievements(
    frame: &mut Frame,
    cursor: usize,
    snap: &crate::achievements::Snapshot,
    progress: &crate::achievements::AchievementProgress,
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let title = format!(
        " achievements - {} points earned, q to leave ",
        progress.points_granted
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chains = crate::achievements::chains();
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    for (i, c) in chains.iter().enumerate() {
        let already = crate::achievements::tiers_unlocked(&progress.unlocked, &c.id);
        let total = c.tiers.len();
        let selected = i == cursor;
        let prefix = if selected { "> " } else { "  " };
        let val = crate::achievements::counter_for(snap, &c.kind).unwrap_or(0);
        let row_style = if selected {
            Style::default()
                .bg(Color::Rgb(40, 40, 40))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        if already >= total {
            // Maxed
            let suffix = if total == 1 {
                "(complete)".to_string()
            } else {
                format!("{} (MAX)", crate::achievements::roman(total as u32))
            };
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("{prefix}{} {suffix}", c.title),
                row_style.fg(Color::Green),
            )));
        } else {
            let tier = &c.tiers[already];
            let next_n = (already + 1) as u32;
            let display_title = if total == 1 {
                c.title.clone()
            } else {
                format!("{} {}", c.title, crate::achievements::roman(next_n))
            };
            let bar_w = 24usize;
            let pct = if tier.target > 0 {
                (val as f32 / tier.target as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let filled = (pct * bar_w as f32) as usize;
            let bar: String = (0..bar_w)
                .map(|i| if i < filled { '=' } else { '.' })
                .collect();
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!(
                    "{prefix}{}  [{}]  {}/{}  +{}sp",
                    display_title, bar, val, tier.target, tier.reward_points
                ),
                row_style,
            )));
        }
        if selected {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("    kind: {}    unlocked: {}/{}", c.kind, already, total),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_shipwright(
    frame: &mut Frame,
    cursor: usize,
    hull_tier: u32,
    valu: u64,
    wood: u32,
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let title = format!(
        " shipwright  j/k pick  enter buy  q close  |  {valu}$V  {wood}wood "
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::LightYellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!(
            "  Current hull: tier {hull_tier} — {}",
            crate::player::hull_label(hull_tier)
        ),
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
    )));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!(
            "  Ocean depth limit: {}  |  Wood stash: {wood}  |  Valu: {valu}",
            match crate::player::ocean_depth_max(hull_tier) {
                u32::MAX => "unlimited (Fog Sea)".to_string(),
                v => format!("{v} tiles"),
            }
        ),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(""));
    // Tier rows 0..=6 — show all upgrades + their state. cursor index is
    // the *from* tier, so cursor==hull_tier is the next buyable row.
    for from in 0u32..=5 {
        let selected = (from as usize) == cursor;
        let prefix = if selected { "> " } else { "  " };
        let (state, fg) = if from < hull_tier {
            ("[owned]   ", Color::DarkGray)
        } else if from == hull_tier {
            ("[next]    ", Color::LightGreen)
        } else {
            ("[locked]  ", Color::Rgb(80, 80, 80))
        };
        let (cv, cw) = crate::player::hull_upgrade_cost(from).unwrap_or((0, 0));
        let to = from + 1;
        let line_str = format!(
            "{prefix}{state}tier {from} -> {to}  {}  ({cv}$V  + {cw}wood)",
            crate::player::hull_label(to)
        );
        let style = if selected {
            Style::default()
                .fg(fg)
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(fg)
        };
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            line_str, style,
        )));
    }
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  Crew Hunger ticks up 1 per catch made on the boat. :feed <n> drops 3 each.".to_string(),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  Biofuel drains 1 per boat-step. :burn <n> yields 5 × difficulty per fish.".to_string(),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  Empty tank = dumped back at the home pier.".to_string(),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_bait_shop(
    frame: &mut Frame,
    cursor: usize,
    stock: &crate::bait::BaitStock,
    valu: u64,
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let title = format!(
        " bait  j/k pick  enter buy  e equip  u unequip  q close  |  {} valu ",
        valu
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::LightGreen));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let defs: Vec<&'static crate::bait::BaitDef> = crate::bait::defs()
        .iter()
        .filter(|d| d.cost > 0 || stock.count(&d.id) > 0)
        .collect();
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    let active_label = stock
        .active
        .as_deref()
        .and_then(crate::bait::def_by_id)
        .map(|d| d.name.as_str())
        .unwrap_or("(none)");
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!("  Active: {}", active_label),
        Style::default().fg(Color::LightGreen),
    )));
    lines.push(ratatui::text::Line::from(""));
    for (i, d) in defs.iter().enumerate() {
        let owned = stock.count(&d.id);
        let selected = i == cursor;
        let prefix = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let cost_label = if crate::bait::is_wild(&d.id) {
            "wild".to_string()
        } else {
            format!("{}$V", d.cost)
        };
        let line = format!(
            "{prefix}{} - {}  (own {}) +{:.0}% {}",
            d.name,
            cost_label,
            owned,
            d.magnitude * 100.0,
            d.effect
        );
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(line, style)));
        if selected {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("    {}", d.description),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_tackle_shop(
    frame: &mut Frame,
    slot_idx: usize,
    cursor: usize,
    equipped: &crate::tackle::EquippedTackle,
    valu: u64,
) {
    use crate::tackle::Slot;
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    let slot = Slot::ALL[slot_idx % Slot::ALL.len()];
    let owned = equipped.tier(slot);
    let title = format!(
        " tackle  ({})  h/l slot  j/k pick  enter buy  q close  |  {} valu ",
        slot.label(),
        valu
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    // Tab bar
    let mut tab_spans: Vec<ratatui::text::Span> = Vec::new();
    for (i, s) in Slot::ALL.iter().enumerate() {
        let active = i == slot_idx;
        let style = if active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tab_spans.push(ratatui::text::Span::styled(format!(" {} ", s.label()), style));
        tab_spans.push(ratatui::text::Span::raw(" "));
    }
    lines.push(ratatui::text::Line::from(tab_spans));
    lines.push(ratatui::text::Line::from(""));

    let defs = crate::tackle::defs_for_slot(slot);
    for (i, d) in defs.iter().enumerate() {
        let selected = i == cursor;
        let prefix = if selected { "> " } else { "  " };
        let status = if d.tier <= owned {
            "[OWNED]"
        } else if d.tier == owned + 1 {
            "[next]"
        } else {
            "[locked]"
        };
        let color = if d.tier <= owned {
            Color::Green
        } else if d.tier == owned + 1 {
            Color::White
        } else {
            Color::DarkGray
        };
        let style = if selected {
            Style::default()
                .fg(color)
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        let line = format!(
            "{prefix}T{} {} {} - {}$V  (+{:.0}% {})",
            d.tier,
            d.name,
            status,
            d.cost,
            d.magnitude * 100.0,
            d.effect
        );
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(line, style)));
        if selected {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("    {}", d.description),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), inner);
}

fn render_mining(frame: &mut Frame, m: &crate::mining::Mining) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" mining ")
        .border_style(Style::default().fg(m.ore.color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let typed_len = m.typed.chars().count();
    let mut spans: Vec<ratatui::text::Span> = Vec::new();
    for (i, c) in m.ore.name.chars().enumerate() {
        let style = if i < typed_len {
            Style::default()
                .fg(m.ore.color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(60, 60, 60))
        };
        spans.push(ratatui::text::Span::styled(c.to_string(), style));
    }
    let title_line = ratatui::text::Line::from(spans).alignment(Alignment::Center);
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "type the ore's name",
        Style::default().fg(Color::DarkGray),
    )).alignment(Alignment::Center));
    lines.push(ratatui::text::Line::from(""));
    lines.push(title_line);
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "(esc to leave)",
        Style::default().fg(Color::DarkGray),
    )).alignment(Alignment::Center));
    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

/// Indices of recipes that match the cookbook filter. Empty filter →
/// every recipe in catalog order. Matches against recipe name and
/// ingredient names (case-insensitive substring).
fn cookbook_visible_indices(filter: &str) -> Vec<usize> {
    let q = filter.to_ascii_lowercase();
    let recs = crate::recipes::recipes();
    if q.is_empty() {
        return (0..recs.len()).collect();
    }
    recs.iter()
        .enumerate()
        .filter(|(_, r)| {
            r.name.to_ascii_lowercase().contains(&q)
                || r.ingredients
                    .iter()
                    .any(|(name, _)| name.to_ascii_lowercase().contains(&q))
        })
        .map(|(i, _)| i)
        .collect()
}

fn render_cookbook(
    frame: &mut Frame,
    cursor: usize,
    cooking_level: u32,
    mastery: &[u32],
    discovered: &[bool],
    basket: &[&'static crate::fish::FishDef],
    filter: &str,
    editing_filter: bool,
) {
    use ratatui::widgets::Paragraph;
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let recs = crate::recipes::recipes();
    let discovered_total = discovered.iter().filter(|d| **d).count();
    let unlocked = recs
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            discovered.get(*i).copied().unwrap_or(false)
                && cooking_level >= r.min_cooking_level
        })
        .count();
    let mastered = mastery.iter().filter(|m| **m >= 5).count() as u32;
    // Next milestone for the header
    let next_mile = COOKBOOK_MILES
        .iter()
        .find(|m| mastered < m.threshold);
    let mile_blurb = match next_mile {
        Some(m) => format!(
            " | next milestone: {} @ {} mastered ({}/{})",
            m.label, m.threshold, mastered, m.threshold
        ),
        None => " | all milestones earned".to_string(),
    };
    let level_bonus_pct = ((cooking_level as f32 * 0.5).min(30.0)) as u32;
    let visible = cookbook_visible_indices(filter);
    let title = if editing_filter {
        format!(" cookbook / {filter}_   (Enter apply, Esc clear) ")
    } else if !filter.is_empty() {
        format!(
            " cookbook  filter: {filter} ({} match)  j/k pick  enter cook  / edit filter  esc clear  q close ",
            visible.len(),
        )
    } else {
        format!(
            " cookbook  cooking lv {cooking_level} (+{level_bonus_pct}% all dishes)  |  discovered {discovered_total}/{}  |  unlocked {unlocked}  |  mastered {mastered}{mile_blurb}  j/k pick  enter cook  / filter  q close ",
            recs.len(),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::LightYellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    for (vi, &i) in visible.iter().enumerate() {
        let r = &recs[i];
        let selected = vi == cursor;
        let prefix = if selected { "> " } else { "  " };
        let is_discovered = discovered.get(i).copied().unwrap_or(false);
        let unlocked = is_discovered && cooking_level >= r.min_cooking_level;
        let cookable = unlocked && crate::recipes::can_cook(r, basket);
        let m = mastery.get(i).copied().unwrap_or(0);
        let scale = 1.0 + ((m as f32 / 5.0).floor() * 0.05).min(0.50);
        let mastery_tag = if m >= 5 {
            format!(" m{m}  ×{:.2}", scale)
        } else if m > 0 {
            format!(" m{m}")
        } else {
            String::new()
        };

        let status = if !is_discovered {
            "[???]".to_string()
        } else if !unlocked {
            format!("[lvl {}]", r.min_cooking_level)
        } else if cookable {
            "[cook]".to_string()
        } else {
            "[need]".to_string()
        };

        let row_fg = if !is_discovered {
            Color::Rgb(45, 45, 45)
        } else if !unlocked {
            Color::Rgb(70, 70, 70)
        } else if cookable {
            Color::White
        } else {
            Color::DarkGray
        };
        let row_style = if selected {
            Style::default()
                .fg(row_fg)
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(row_fg)
        };
        let ing: Vec<String> = r
            .ingredients
            .iter()
            .map(|(n, q)| format!("{q}× {n}"))
            .collect();
        let eff = r
            .effect
            .as_deref()
            .map(|e| format!(" +{e}"))
            .unwrap_or_default();
        let display_name = if is_discovered {
            r.name.clone()
        } else {
            "???".to_string()
        };
        let display_ing = if is_discovered { ing.join(", ") } else { "???".to_string() };
        let display_eff = if is_discovered { eff } else { String::new() };
        let display_st = if is_discovered { r.stamina.to_string() } else { "?".to_string() };
        let row = format!(
            "{prefix}{status}  {:<28}  +{}st{display_eff}  ({}){mastery_tag}",
            display_name, display_st, display_ing
        );
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            row, row_style,
        )));
        if selected && unlocked {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("    {}", r.description),
                Style::default().fg(Color::Rgb(120, 120, 120)),
            )));
            // Per-ingredient own/need breakdown so the player can see at a
            // glance which fish they're missing. Lit-yellow if covered,
            // red if not.
            for (in_name, qty) in &r.ingredients {
                let have = basket
                    .iter()
                    .filter(|f| f.name.eq_ignore_ascii_case(in_name))
                    .count() as u32;
                let met = have >= *qty;
                let color = if met { Color::LightGreen } else { Color::LightRed };
                lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                    format!("      {}: {}/{}", in_name, have, qty),
                    Style::default().fg(color),
                )));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_perf_overlay(frame: &mut Frame) {
    let full = viewport(frame);
    let snap = crate::perf::snapshot();
    // Compact centered panel — leave the world visible around it.
    let w = 78u16.min(full.width.saturating_sub(4));
    let h = ((snap.len() as u16) + 6).min(full.height.saturating_sub(4));
    if w < 40 || h < 6 {
        return;
    }
    let area = Rect {
        x: full.x + (full.width.saturating_sub(w)) / 2,
        y: full.y + (full.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(Clear, area);
    let title = format!(" perf — {} phases — esc/q close ", snap.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::LightMagenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut rows: Vec<(&'static str, u64, u64, u64, u64)> = snap
        .iter()
        .map(|(name, r)| (*name, r.mean(), r.p95(), r.max(), r.last()))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!(
            "  {:<28} {:>9} {:>9} {:>9} {:>9}",
            "phase", "mean(us)", "p95(us)", "max(us)", "last(us)"
        ),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));
    lines.push(ratatui::text::Line::from(""));
    for (name, mean, p95, max, last) in rows {
        let hot = mean >= 5_000;
        let style = if hot {
            Style::default().fg(Color::Rgb(220, 110, 60)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            format!("  {name:<28} {mean:>9} {p95:>9} {max:>9} {last:>9}"),
            style,
        )));
    }
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  hot rows highlighted (mean >= 5ms). budget = 50ms/frame at 20fps.",
        Style::default().fg(Color::DarkGray),
    )));
    let p = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_lure_bench(
    frame: &mut Frame,
    cursor: usize,
    stock: &crate::bait::BaitStock,
    valu: u64,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let title = format!(" bait bench — j/k pick — enter craft — esc leave — {valu}$V ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Rgb(200, 160, 100)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let recipes = crate::lure_recipes::recipes();
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    for (i, r) in recipes.iter().enumerate() {
        let prefix = if i == cursor { "> " } else { "  " };
        let style = if i == cursor {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(40, 30, 18))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let out_label = crate::bait::def_by_id(&r.output_bait_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| r.output_bait_id.clone());
        let head = format!("{prefix}{}  -> {} (x{})  [{}$V]",
            r.name, out_label, r.output_count, r.valu_cost);
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(head, style)));
        if i == cursor {
            for inp in &r.inputs {
                let have = stock.count(&inp.bait_id);
                let label = crate::bait::def_by_id(&inp.bait_id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| inp.bait_id.clone());
                let ok = have >= inp.count;
                let row = format!("    - {}x {} (have {})", inp.count, label, have);
                let row_color = if ok { Color::DarkGray } else { Color::Red };
                lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                    row,
                    Style::default().fg(row_color),
                )));
            }
        }
    }
    let p = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_scales(
    frame: &mut Frame,
    cursor: usize,
    bank: u64,
    spent: &std::collections::BTreeMap<String, u32>,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let title = format!(" scales — j/k pick — enter spend 1 — esc leave — bank {bank} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::LightCyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    for (i, axis) in App::SCALES_AXES.iter().enumerate() {
        let s = spent.get(*axis).copied().unwrap_or(0);
        let prefix = if i == cursor { "> " } else { "  " };
        let pct = (s as f32) * 0.05;
        let line = format!("{prefix}{axis:14} {s:4}/1000  +{pct:.2}%");
        let style = if i == cursor {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(40, 40, 40))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(line, style)));
    }
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  each spent scale = +0.05% on the chosen axis. cap 1000 / axis.",
        Style::default().fg(Color::DarkGray),
    )));
    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_bug_catch(frame: &mut Frame, b: &crate::bug_catch::BugCatch, tick: u64) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let secs_left = if b.deadline_tick > tick {
        ((b.deadline_tick - tick) as f32 / 20.0).ceil() as u32
    } else {
        0
    };
    let in_zone = b.in_target();
    let title = format!(
        " bug net — space/f to swing — esc to leave — {secs_left}s — {}/{} swings ",
        b.attempts_left,
        crate::bug_catch::MAX_ATTEMPTS,
    );
    let border = if in_zone { Color::LightGreen } else { Color::Yellow };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bar_width: usize = 60;
    let lo = (b.target_lo * bar_width as f32) as usize;
    let hi = ((b.target_hi * bar_width as f32) as usize).min(bar_width.saturating_sub(1));
    let cursor = (b.cursor * bar_width as f32) as usize;
    let mut spans: Vec<ratatui::text::Span> = Vec::with_capacity(bar_width + 2);
    spans.push(ratatui::text::Span::raw("["));
    for i in 0..bar_width {
        if i == cursor {
            spans.push(ratatui::text::Span::styled(
                "|",
                Style::default()
                    .fg(if in_zone { Color::LightGreen } else { Color::White })
                    .add_modifier(Modifier::BOLD),
            ));
        } else if i >= lo && i <= hi {
            spans.push(ratatui::text::Span::styled(
                "=",
                Style::default().fg(Color::Green),
            ));
        } else {
            spans.push(ratatui::text::Span::styled(
                "-",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
    spans.push(ratatui::text::Span::raw("]"));
    let bar_line = ratatui::text::Line::from(spans).alignment(Alignment::Center);

    let bug_name = crate::bugs::def_by_id(&b.bug_id)
        .map(|d| d.name.as_str())
        .unwrap_or(&b.bug_id);
    let label = ratatui::text::Line::from(ratatui::text::Span::styled(
        format!("after the {bug_name}"),
        Style::default().fg(Color::DarkGray),
    ))
    .alignment(Alignment::Center);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(""));
    lines.push(label);
    lines.push(ratatui::text::Line::from(""));
    lines.push(bar_line);
    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_chopping(frame: &mut Frame, c: &crate::chop::Chopping, tick: u64) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let lockout_secs = if c.is_locked(tick) {
        ((c.lockout_until_tick - tick) as f32 / 20.0).ceil() as u32
    } else {
        0
    };
    let title_color = if lockout_secs > 0 {
        Color::Red
    } else {
        Color::LightGreen
    };
    let title = if lockout_secs > 0 {
        format!(" chopping — LOCKED OUT {lockout_secs}s — Esc to leave ")
    } else {
        " chopping — type the F/G/H/J sequence — Esc to leave ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(title_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Sequence row: typed prefix bright green, current letter highlighted,
    // remaining dim gray.
    let mut spans: Vec<ratatui::text::Span> = Vec::with_capacity(c.sequence.len() * 2);
    for (i, ch) in c.sequence.iter().enumerate() {
        let label = format!(" {} ", ch.to_ascii_uppercase());
        let style = if i < c.typed {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        } else if i == c.typed {
            if lockout_secs > 0 {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            }
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(ratatui::text::Span::styled(label, style));
    }
    let seq_line = ratatui::text::Line::from(spans).alignment(Alignment::Center);

    let progress = format!(
        " {}/{}  (yield: +{} wood) ",
        c.typed,
        c.sequence.len(),
        c.wood_yield
    );

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(""));
    lines.push(seq_line);
    lines.push(ratatui::text::Line::from(""));
    lines.push(
        ratatui::text::Line::from(ratatui::text::Span::styled(
            progress,
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center),
    );
    if lockout_secs > 0 {
        lines.push(ratatui::text::Line::from(""));
        lines.push(
            ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("wrong key. keys ignored for {lockout_secs}s."),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );
    }
    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn render_gear_panel(
    frame: &mut Frame,
    slot_idx: usize,
    item_idx: usize,
    slots: &[crate::gear::Slot],
    gear: &crate::gear::EquippedGear,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" gear — h/l switch slot, j/k pick, Enter equip, u unequip, q leave ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let cur_slot = slots[slot_idx.min(slots.len() - 1)];

    // Slot tab row
    let mut tab_spans: Vec<ratatui::text::Span> = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let style = if i == slot_idx {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        tab_spans.push(ratatui::text::Span::styled(
            format!(" {} ", s.label()),
            style,
        ));
        tab_spans.push(ratatui::text::Span::raw(" "));
    }
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(tab_spans));
    lines.push(ratatui::text::Line::from(""));
    // Equipped header
    let equipped_id = gear.equipped(cur_slot);
    let equipped_name = equipped_id
        .and_then(crate::gear::def_by_id)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "(empty)".to_string());
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!("  equipped: {equipped_name}"),
        Style::default().fg(Color::LightYellow),
    )));
    lines.push(ratatui::text::Line::from(""));
    // Owned list for this slot
    let owned: Vec<&String> = gear
        .owned
        .iter()
        .filter(|id| {
            crate::gear::def_by_id(id)
                .and_then(|d| d.slot_enum())
                .map(|s| s == cur_slot)
                .unwrap_or(false)
        })
        .collect();
    if owned.is_empty() {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "  (none owned — forge one at the blacksmith)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, id) in owned.iter().enumerate() {
            let def = crate::gear::def_by_id(id);
            let name = def.map(|d| d.name.clone()).unwrap_or_else(|| (*id).clone());
            let is_equipped = Some(id.as_str()) == equipped_id;
            let tier = def.map(|d| d.tier).unwrap_or(0);
            let prefix = if i == item_idx { "> " } else { "  " };
            let style = if i == item_idx {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_equipped {
                Style::default().fg(Color::LightYellow)
            } else {
                Style::default().fg(Color::White)
            };
            let mark = if is_equipped { " *" } else { "" };
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("{prefix}{name:<28} t{tier}{mark}"),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_sell_gear(
    frame: &mut Frame,
    cursor: usize,
    owned: &[String],
    equipped: &std::collections::HashSet<String>,
    valu_mult: f32,
    ore_sold_today: u32,
    cap: u32,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" sell forged gear — j/k pick, Enter sell, q back · today {ore_sold_today}/{cap} "))
        .border_style(Style::default().fg(Color::LightRed));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    if owned.is_empty() {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "you haven't forged anything yet",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, id) in owned.iter().enumerate() {
            let def = crate::gear::def_by_id(id);
            let name = def.map(|d| d.name.clone()).unwrap_or_else(|| id.clone());
            let ingot_total: u64 = def
                .map(|d| {
                    d.recipe
                        .ingots
                        .iter()
                        .map(|(n, q)| {
                            crate::mining::ore_by_name(n)
                                .map(|o| o.ingot_value() * (*q as u64))
                                .unwrap_or(0)
                        })
                        .sum::<u64>()
                })
                .unwrap_or(0);
            let price = ((ingot_total as f32) * 1.10 * valu_mult).round() as u64;
            let worn = if equipped.contains(id) { " (equipped — will unequip)" } else { "" };
            let prefix = if i == cursor { "> " } else { "  " };
            let style = if i == cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("{prefix}{:<28} {price}$V{}", name, worn),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_blacksmith_menu(
    frame: &mut Frame,
    cursor: u8,
    _smeltable_rows: usize,
    _forgeable_rows: usize,
    raw_ore: u32,
    ingots: u32,
    ore_sold_today: u32,
    cap: u32,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" blacksmith — j/k pick, Enter select, q/esc leave ")
        .border_style(Style::default().fg(Color::LightRed));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        format!(
            "  raw ore: {raw_ore}    ingots: {ingots}    today's cart: {ore_sold_today}/{cap}",
        ),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(""));
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        "  (smelt at the Smelter (S), forge at the Forge (F) — both next to the smith)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(ratatui::text::Line::from(""));
    let options = [
        ("Sell ore + ingots (up to today's cap)".to_string()),
        ("Sell forged gear (10% over ingot value)".to_string()),
        ("Leave".to_string()),
    ];
    for (i, label) in options.iter().enumerate() {
        let prefix = if i as u8 == cursor { "> " } else { "  " };
        let style = if i as u8 == cursor {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            format!("{prefix}{label}"),
            style,
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_smelt(
    frame: &mut Frame,
    cursor: usize,
    typed: &str,
    avail: Vec<(&'static crate::mining::OreDef, u32)>,
    ingots: &std::collections::BTreeMap<String, u32>,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" smelting — j/k pick ore · type the ore's name to smelt one ingot · esc to leave ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    if avail.is_empty() {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "no ore stacks large enough to smelt",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, (ore, raw)) in avail.iter().enumerate() {
            let ing = ingots.get(ore.name).copied().unwrap_or(0);
            let prefix = if i == cursor { "> " } else { "  " };
            let style = if i == cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(ore.color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(ore.color)
            };
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!(
                        "{prefix}{:<10}  raw {:>4}  ingots {:>4}  (cost {} raw / ingot, tier {})",
                        ore.name, raw, ing, ore.ore_per_ingot, ore.tier,
                    ),
                    style,
                ),
            ]));
        }
    }
    lines.push(ratatui::text::Line::from(""));
    // typed-progress bar for the selected ore's name
    let target = avail
        .get(cursor.min(avail.len().saturating_sub(1)))
        .map(|(o, _)| o.name)
        .unwrap_or("");
    let typed_n = typed.chars().count();
    let mut spans: Vec<ratatui::text::Span> = Vec::new();
    spans.push(ratatui::text::Span::raw("type: "));
    for (i, c) in target.chars().enumerate() {
        let style = if i < typed_n {
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(ratatui::text::Span::styled(c.to_string(), style));
    }
    lines.push(ratatui::text::Line::from(spans));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_forge(
    frame: &mut Frame,
    cursor: usize,
    typed: &str,
    avail: Vec<&'static crate::gear::GearDef>,
    ingots: &std::collections::BTreeMap<String, u32>,
    valu: u64,
    bs_level: u32,
) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" forging — Blacksmithing lv {bs_level} · j/k pick · type the item name · esc to leave "))
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
    if avail.is_empty() {
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "no eligible recipes — smelt ingots, level up Blacksmithing, or save valu",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let _ = valu;
        let _ = ingots;
        for (i, def) in avail.iter().enumerate() {
            let prefix = if i == cursor { "> " } else { "  " };
            let style = if i == cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let recipe = def
                .recipe
                .ingots
                .iter()
                .map(|(id, q)| format!("{q}x {id}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!(
                    "{prefix}{:<28} t{}  bs≥{}  cost {}: {}",
                    def.name, def.tier, def.min_blacksmithing_level, def.recipe.valu, recipe,
                ),
                style,
            )));
        }
    }
    lines.push(ratatui::text::Line::from(""));
    // typed-progress bar for the selected def name
    let target = avail
        .get(cursor.min(avail.len().saturating_sub(1)))
        .map(|d| d.name.to_ascii_lowercase())
        .unwrap_or_default();
    let typed_n = typed.chars().count();
    let mut spans: Vec<ratatui::text::Span> = Vec::new();
    spans.push(ratatui::text::Span::raw("type: "));
    for (i, c) in target.chars().enumerate() {
        let style = if i < typed_n {
            Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(ratatui::text::Span::styled(c.to_string(), style));
    }
    lines.push(ratatui::text::Line::from(spans));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_dialogue(frame: &mut Frame, npc: &Npc, line: usize) {
    // Fullscreen, top-down. All previously-seen lines render above the
    // current one (waterfall style), so the player sees the full conversation.
    let area = viewport(frame);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", npc.name))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let total = npc.dialogue.len();
    let shown = (line + 1).min(total);
    let mut lines: Vec<ratatui::text::Line> = Vec::with_capacity(shown + 4);
    for (i, dline) in npc.dialogue.iter().take(shown).enumerate() {
        // dim past lines, bold the current one
        let style = if i + 1 == shown {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            dline.clone(),
            style,
        )));
        lines.push(ratatui::text::Line::from(""));
    }
    lines.push(ratatui::text::Line::from(""));
    let footer = if line + 1 >= total {
        "(enter/space to leave)".to_string()
    } else {
        format!(
            "({}/{} - enter/space to continue, q to leave)",
            line + 1,
            total
        )
    };
    lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
        footer,
        Style::default().fg(Color::DarkGray),
    )));
    let p = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_name_prompt(frame: &mut Frame, buf: &str) {
    let area = viewport(frame);
    frame.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" fishcli ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let mid_y = inner.y + inner.height / 2;
    let title = "    /\\___\\";
    let body_lines = [
        "",
        "          welcome.",
        "",
        "         what's your name, angler?",
        "",
        &format!("            > {buf}_"),
        "",
        "         (enter to confirm)",
    ];
    let title_p = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Center);
    let title_area = Rect {
        x: inner.x,
        y: mid_y.saturating_sub(8),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(title_p, title_area);

    let body: Vec<ratatui::text::Line> = body_lines
        .iter()
        .map(|l| ratatui::text::Line::from(*l))
        .collect();
    let body_p = Paragraph::new(body)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);
    let body_area = Rect {
        x: inner.x,
        y: mid_y.saturating_sub(6),
        width: inner.width,
        height: 10.min(inner.height.saturating_sub(2)),
    };
    frame.render_widget(body_p, body_area);
}

/// Tiny consonant-vowel name generator. Picks 2-3 CV pairs to make names
/// like "Tibo" or "Karuna". Used to give friendly faceless figures a name.
fn random_friendly_name(rng: &mut u32) -> String {
    const C: &[&str] = &[
        "b", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "r", "s", "t", "v", "w", "z",
    ];
    const V: &[&str] = &["a", "e", "i", "o", "u"];
    let pairs = 2 + (crate::fish::next_rand_f32(rng) * 2.0) as usize;
    let mut s = String::new();
    for _ in 0..pairs {
        let ci = (crate::fish::next_rand_f32(rng) * C.len() as f32) as usize % C.len();
        let vi = (crate::fish::next_rand_f32(rng) * V.len() as f32) as usize % V.len();
        s.push_str(C[ci]);
        s.push_str(V[vi]);
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => s,
    }
}

fn render_cmdline(frame: &mut Frame, area: Rect, mode: &Mode) {
    frame.render_widget(Clear, area);
    let (text, style) = match mode {
        Mode::Insert => (
            "-- INSERT --".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Mode::Normal => (
            "-- NORMAL --".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Mode::Command(buf) => (
            format!(":{buf}"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    };
    let p = Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Left);
    frame.render_widget(p, area);
}

fn render_valu(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" valu ")
        .border_style(Style::default().fg(Color::Yellow));
    let p = Paragraph::new(text.to_string())
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Right)
        .block(block);
    frame.render_widget(p, area);
}

/// 1-row horizontal gauge: bracketed filled/empty cells and a centered
/// numeric readout. Colors shift green -> yellow -> red as stamina drops
/// so the player can read the bar at a glance.
fn render_boat_hud(
    frame: &mut Frame,
    area: Rect,
    hull_tier: u32,
    crew_hunger: u32,
    biofuel: u32,
    wood: u32,
    cur_depth: u32,
    max_depth: u32,
) {
    if area.width < 10 {
        return;
    }
    let hull = crate::player::hull_label(hull_tier);
    // Hunger ramps red-orange as it climbs; biofuel goes red as it drops.
    let hunger_color = if crew_hunger >= 80 {
        Color::Red
    } else if crew_hunger >= 50 {
        Color::Yellow
    } else {
        Color::LightGreen
    };
    let fuel_color = if biofuel <= 20 {
        Color::Red
    } else if biofuel <= 60 {
        Color::Yellow
    } else {
        Color::LightCyan
    };
    // Depth painted yellow as you approach the hull limit, red when you're
    // about to be refused the next step. Fog Sea (>=32, hull tier 6+) shows
    // as 'FOG' instead of a number since the limit is uncapped.
    let max_label = if max_depth == u32::MAX {
        "∞".to_string()
    } else {
        max_depth.to_string()
    };
    let in_fog = cur_depth >= 32;
    let depth_color = if in_fog {
        Color::Rgb(180, 180, 220)
    } else if max_depth != u32::MAX && cur_depth + 1 >= max_depth {
        Color::Red
    } else if max_depth != u32::MAX && cur_depth * 3 >= max_depth * 2 {
        Color::Yellow
    } else {
        Color::LightBlue
    };
    let line = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            format!("[{hull}] "),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
        ratatui::text::Span::styled(
            format!("depth {cur_depth:>2}/{max_label} "),
            Style::default().fg(depth_color),
        ),
        ratatui::text::Span::styled(
            format!("hunger {crew_hunger:>3}/100 "),
            Style::default().fg(hunger_color),
        ),
        ratatui::text::Span::styled(
            format!("fuel {biofuel:>3}/200 "),
            Style::default().fg(fuel_color),
        ),
        ratatui::text::Span::styled(
            format!("wood {wood} "),
            Style::default().fg(Color::Rgb(180, 120, 60)),
        ),
    ]);
    let para = Paragraph::new(line).alignment(Alignment::Left);
    frame.render_widget(para, area);
}

fn render_stamina_bar(frame: &mut Frame, area: Rect, current: f32, max: f32) {
    if area.width < 6 || max <= 0.0 {
        return;
    }
    let pct = (current / max).clamp(0.0, 1.0);
    let color = if pct > 0.5 {
        Color::Green
    } else if pct > 0.20 {
        Color::Yellow
    } else if pct > 0.0 {
        Color::Red
    } else {
        Color::DarkGray
    };
    // Reserve "[" + "]" + " 045/100" = 10 chars; the rest is the bar
    // body. Off-by-one would leave the trailing cell uncolored and the
    // world tile underneath would bleed through.
    let total_cells = area.width.saturating_sub(10) as usize;
    let filled = ((total_cells as f32) * pct).round() as usize;
    let bar: String = std::iter::repeat('#').take(filled)
        .chain(std::iter::repeat('-').take(total_cells.saturating_sub(filled)))
        .collect();
    let label = format!("[{bar}] {:>3}/{:>3}", current as u32, max as u32);
    let para = Paragraph::new(label)
        .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
    frame.render_widget(para, area);
}

pub fn format_valu(v: u64) -> String {
    fn short(n: f64, suffix: &str) -> String {
        let s = format!("{:.1}", n);
        let trimmed = s.strip_suffix(".0").unwrap_or(&s);
        format!("{}{}$V", trimmed, suffix)
    }
    if v < 1_000 {
        return format!("{}$V", v);
    }
    if v < 1_000_000 {
        return short(v as f64 / 1_000.0, "k");
    }
    if v < 1_000_000_000 {
        return short(v as f64 / 1_000_000.0, "M");
    }
    if v < 1_000_000_000_000 {
        return short(v as f64 / 1_000_000_000.0, "B");
    }
    short(v as f64 / 1_000_000_000_000.0, "T")
}

#[cfg(test)]
mod tests {
    use super::format_valu;

    #[test]
    fn formats_smartly() {
        assert_eq!(format_valu(0), "0$V");
        assert_eq!(format_valu(999), "999$V");
        assert_eq!(format_valu(2_500), "2.5k$V");
        assert_eq!(format_valu(29_000_000), "29M$V");
        assert_eq!(format_valu(1_200_000_000), "1.2B$V");
    }
}
