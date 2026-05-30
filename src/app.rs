use crate::fish;
use crate::fishdex::Fishdex;
use crate::fishing::{Fishing, FishingResult};
use crate::fishlist;
use crate::narrator::Narrator;
use crate::fish::FishDef;
use crate::item::{Category, Item};
use crate::notes;
use crate::npc::{self, Npc};
use crate::player::Player;
use crate::quest;
use crate::save::{self, SaveData};
use crate::stats::{fish_catch_xp, level_to_xp, Skills, Stats};
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

pub enum Scene {
    Overworld,
    RodShop { cursor: u32 },
    FishingSchool,
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
}

/// Horizontal movement interval (ticks/step). Smaller because terminal cells
/// are roughly 2:1 - a vertical step covers ~2x the visual distance of a horizontal one.
const MOVE_INTERVAL_H: u64 = 2;
const MOVE_INTERVAL_V: u64 = 4;

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
    /// xp gain popup: (skill_name, gained_xp, current_total_xp, level, ticks_remaining)
    pub xp_popup: Option<(&'static str, u64, u64, u32, u32)>,
    /// total valu earned lifetime (sum of quest rewards + sales)
    pub lifetime_valu: u64,
    /// time when this session started (for play-time stat)
    pub session_start: std::time::Instant,
    /// play time loaded from save (excluding this session)
    pub saved_play_secs: u64,
    /// id of the quest currently pinned to the top-left tracker
    pub pinned_quest: Option<String>,
    /// coarse map cells (each 4w x 2h world cells) the player has explored
    pub seen_cells: std::collections::HashSet<(i32, i32)>,
    pub stats: Stats,
    pub skills: Skills,
    pub buffs: crate::buffs::Buffs,
    /// background autosave channel - thread coalesces and writes.
    autosave_tx: mpsc::Sender<SaveData>,
    last_autosave_at: Instant,
    last_autosave_hash: u64,
    pub cast: Option<CastState>,
    held_dir: Option<(i32, i32)>,
    held_until_tick: u64,
    last_step_tick: u64,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self::fresh();
        if let Some(data) = save::load_from_disk() {
            app.apply_save(&data);
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
        narrator.say("hjkl/arrows: move    f: interact    g: pick up    x: inspect    e: fishdex    esc: normal");
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
            pending_catch_loc: None,
            narrator,
            quest_progress: HashMap::new(),
            quest_done: Vec::new(),
            current_biome: None,
            current_location: None,
            biome_popup_ticks: 0,
            xp_popup: None,
            lifetime_valu: 0,
            session_start: std::time::Instant::now(),
            saved_play_secs: 0,
            pinned_quest: None,
            seen_cells: std::collections::HashSet::new(),
            stats: Stats::default(),
            skills: Skills::default(),
            buffs: crate::buffs::Buffs::default(),
            autosave_tx: spawn_autosaver(),
            last_autosave_at: Instant::now(),
            last_autosave_hash: 0,
            cast: None,
            held_dir: None,
            held_until_tick: 0,
            last_step_tick: 0,
        }
    }

    pub fn total_play_secs(&self) -> u64 {
        self.saved_play_secs + self.session_start.elapsed().as_secs()
    }

    fn quest_progress(&mut self, kind: &str, target: &str) {
        self.tick_quest_progress(kind, target, false);
    }

    fn quest_progress_silent(&mut self, kind: &str, target: &str) {
        self.tick_quest_progress(kind, target, true);
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
        self.world = World::new(data.world_seed);
        self.world.dim = data.dim;
        if data.rng_state != 0 {
            self.rng_state = data.rng_state;
        }
        self.quest_progress = data.quest_progress.iter().cloned().collect();
        self.quest_done = data.quest_done.clone();
        self.player.items = data.items.clone();
        self.lifetime_valu = data.lifetime_valu_earned;
        self.saved_play_secs = data.play_time_secs;
        self.pinned_quest = data.pinned_quest.clone();
        self.seen_cells = data.seen_cells.iter().copied().collect();
        self.stats = data.stats.clone();
        self.skills = data.skills.clone();
        self.buffs = data.buffs.clone();
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
            seen_cells: self.seen_cells.iter().copied().collect(),
            stats: self.stats.clone(),
            skills: self.skills.clone(),
            rods: self.player.rods,
            caught_at: self.caught_at.clone(),
            buffs: self.buffs.clone(),
            dim: self.world.dim,
        }
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
        self.xp_popup = Some((skill, gained, total_xp, level, 100));
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
                }
            }
        }
    }

    fn cast_action(&mut self) {
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
                // geometric wait length
                let r = crate::fish::next_rand_f32(&mut self.rng_state);
                let k = (1.0f32 - r * 0.9999).ln() / 0.75f32.ln();
                let secs = (k.ceil() as u32).clamp(1, 30) as f32;
                let scaled = secs * (1.0 - c.cast_strength * 0.5) * self.buffs.wait_mult();
                c.wait_ticks_left = (scaled * 20.0).max(20.0) as u32;
                c.phase = CastPhase::Waiting;
                self.narrator
                    .say(format!("Cast lands {} tiles out. Waiting...", bd));
            }
            CastPhase::Biting => {
                let fish = c.fish;
                let (bx, by) = c.bobber;
                let biome = biome_at(bx, by, self.world.seed).label().to_string();
                let water = water_kind_at(&self.world, bx, by).to_string();
                self.pending_catch_loc = Some((biome, water));
                self.cast = None;
                self.narrator
                    .say(format!("Hooked a {}!", fish.name));
                self.scene = Scene::Fishing(Fishing::new(
                    fish,
                    self.rng_state,
                    self.skills.fishing_level(),
                    self.player.rods.equipped,
                ));
            }
            CastPhase::Waiting => {}
        }
    }

    fn cancel_cast(&mut self) {
        if self.cast.is_some() {
            self.cast = None;
            self.narrator.say("Reeled in the empty line.");
        }
    }

    fn maybe_autosave(&mut self) {
        // every ~5s, send a snapshot to the background thread, but only if
        // the save has actually changed since the last write.
        if self.last_autosave_at.elapsed() < Duration::from_secs(5) {
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
        self.anim_tick = self.anim_tick.wrapping_add(1);
        if self.biome_popup_ticks > 0 {
            self.biome_popup_ticks -= 1;
        }
        if let Some((_, _, _, _, ref mut t)) = self.xp_popup {
            if *t > 0 {
                *t -= 1;
            } else {
                self.xp_popup = None;
            }
        }
        self.tick_cast();
        self.maybe_autosave();
        // Pantheon meta-progression checks: cheap, idempotent. Only fires when
        // a threshold is crossed and that god isn't already granted.
        if self.anim_tick % 20 == 0 {
            self.check_pantheon_unlocks();
        }

        let movement_allowed =
            matches!(self.mode, Mode::Insert) && matches!(self.scene, Scene::Overworld);
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
                    let interval =
                        ((base as f32) * self.buffs.walk_mult()).round().max(1.0) as u64;
                    if self.anim_tick.saturating_sub(self.last_step_tick) >= interval {
                        self.step(dir.0, dir.1);
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
            if let Scene::Fishing(g) = &mut self.scene {
                match key.code {
                    KeyCode::Char('k') | KeyCode::Up => {
                        g.input_up(key.kind);
                        return;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        g.input_down(key.kind);
                        return;
                    }
                    _ => {}
                }
            }
            if matches!(self.scene, Scene::Overworld) {
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
            Mode::Insert => self.insert_key(key.code),
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
                self.narrator.say(format!("Welcome, {name}."));
                self.narrator
                    .say("Try :w to save your progress whenever.");
                self.scene = Scene::Overworld;
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
                    self.step(dir.0, dir.1);
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

    fn insert_key(&mut self, code: KeyCode) {
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
        // shortcut from any insert-mode scene that doesn't consume ':' to command mode
        if code == KeyCode::Char(':')
            && !matches!(self.scene, Scene::Notes(_) | Scene::NamePrompt(_) | Scene::Dialogue { .. })
        {
            self.mode = Mode::Command(String::new());
            return;
        }
        match &mut self.scene {
            Scene::Overworld => self.handle_overworld(code),
            Scene::Fishdex(d) => match code {
                KeyCode::Char('j') | KeyCode::Down => d.cursor_down(),
                KeyCode::Char('k') | KeyCode::Up => d.cursor_up(),
                KeyCode::Char('q') | KeyCode::Char('e') => self.exit_subscene(),
                _ => {}
            },
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
            Scene::FishingSchool => {
                if matches!(code, KeyCode::Char('q')) {
                    self.exit_subscene();
                }
            }
            Scene::Fishing(_) => {
                if matches!(code, KeyCode::Char('q')) {
                    self.exit_subscene();
                }
            }
            Scene::Help(_) | Scene::Stats | Scene::Settings => {
                if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.scene = Scene::Overworld;
                }
            }
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
            other => self.narrator.say(format!("Unknown command: :{other}")),
        }
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
        if let Some(slot) = self.caught_at.get_mut(i) {
            if slot.is_none() {
                *slot = Some((where_from.to_string(), "—".to_string()));
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

    fn list_quests(&mut self) {
        let mut any = false;
        for q in quest::quests() {
            if self.quest_done.contains(&q.id) {
                continue;
            }
            let progress = self.quest_progress.get(&q.id).copied().unwrap_or(0);
            self.narrator.say(format!(
                "[{}/{}] {} - {}",
                progress, q.objective.count, q.title, q.description
            ));
            any = true;
        }
        if !any {
            self.narrator.say("All tasks complete!");
        }
    }

    fn exit_subscene(&mut self) {
        match &self.scene {
            Scene::Fishing(g) => {
                let fish_ref: &'static crate::fish::FishDef = g.fish;
                let caught = matches!(g.finished, Some(FishingResult::Caught));
                let escaped = matches!(g.finished, Some(FishingResult::Escaped));
                if caught {
                    let mut already_had_unique = false;
                    if let Some(i) = fishlist::fish().iter().position(|f| std::ptr::eq(f, fish_ref)) {
                        if fish_ref.unique && self.caught.get(i).copied().unwrap_or(false) {
                            already_had_unique = true;
                        }
                        self.caught[i] = true;
                        if let Some(slot) = self.caught_at.get_mut(i) {
                            if slot.is_none() {
                                *slot = self.pending_catch_loc.clone();
                            }
                        }
                    }
                    if fish_ref.unique && already_had_unique {
                        self.narrator.say(format!(
                            "You see the {} again but you already have it. It slips back into the deep.",
                            fish_ref.name
                        ));
                    } else {
                        self.player.inventory.push(fish_ref);
                        let name = fish_ref.name.clone();
                        if fish_ref.unique {
                            self.narrator
                                .say(format!("*** YOU REEL IN THE {}! ***", name.to_uppercase()));
                        } else {
                            self.narrator
                                .say(format!("You reel in a {}!", name));
                        }
                        self.narrator.say(format!(
                            "Added to your basket ({} fish).",
                            self.player.inventory.len()
                        ));
                    }
                    let gained = fish_catch_xp(fish_ref.difficulty);
                    self.stats.fish_caught += 1;
                    let before = self.skills.fishing_level();
                    self.skills.fishing_xp += gained;
                    let after = self.skills.fishing_level();
                    self.show_xp_gain("Fishing", gained, self.skills.fishing_xp, after);
                    if after > before {
                        self.narrator
                            .say(format!("Fishing level up! Now level {after}."));
                    }
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
                } else if escaped {
                    self.narrator
                        .say(format!("The {} slips away.", fish_ref.name));
                    self.stats.fish_escaped += 1;
                } else {
                    self.narrator.say("You leave the line slack and step away.");
                }
            }
            Scene::RodShop { .. } | Scene::FishingSchool => {
                self.narrator.say("You step back outside.");
            }
            _ => {}
        }
        self.pending_catch_loc = None;
        self.scene = Scene::Overworld;
    }

    fn handle_overworld(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('e') => {
                self.narrator.say("You leaf through the fishdex.");
                self.scene = Scene::Fishdex(Fishdex::new());
            }
            KeyCode::Char('x') => self.inspect_surroundings(),
            KeyCode::Char('g') => self.pickup_here(),
            KeyCode::Char('f') => {
                if self.cast.is_some() {
                    // f during cast = no-op so player doesn't restart
                } else {
                    self.interact_facing();
                }
            }
            KeyCode::Char(' ') => self.cast_action(),
            KeyCode::Esc if self.cast.is_some() => self.cancel_cast(),
            _ => {}
        }
    }

    fn inspect_surroundings(&mut self) {
        let (dx, dy) = self.player.facing;
        let tx = self.player.x + dx;
        let ty = self.player.y + dy;
        if self.world.dim == crate::world::Dimension::Surface {
            if let Some(npc) = npc::npc_at(tx, ty) {
                self.narrator.say(format!("{}: {}", npc.name, "An ordinary villager. Press f to talk."));
                return;
            }
        }
        let t = self.world.get(tx, ty);
        self.narrator.say(t.describe());
    }

    fn step(&mut self, dx: i32, dy: i32) {
        self.player.facing = (dx, dy);
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        if self.world.dim == crate::world::Dimension::Surface
            && npc::npc_at(nx, ny).is_some()
        {
            return; // blocked by NPC; press f to interact
        }
        let t = self.world.get(nx, ny);
        if t.walkable() {
            self.player.x = nx;
            self.player.y = ny;
            self.check_biome_change();
            self.mark_seen_around_player();
            let weight: u64 = if dy != 0 { 2 } else { 1 };
            self.stats.steps += weight;
            self.skills.walking_xp += weight;
            for _ in 0..weight {
                self.quest_progress_silent("walk", "any");
                if let Some(b) = self.current_biome {
                    self.quest_progress_silent("walk", b.label());
                }
            }
        }
    }

    fn pickup_here(&mut self) {
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
        // NPCs only live on the Surface; below ground or underwater the npc
        // table is irrelevant.
        if self.world.dim == crate::world::Dimension::Surface {
            if let Some(npc) = npc::npc_at(nx, ny) {
                if npc.id == "sailor" {
                    self.interact_sailor();
                    return;
                }
                self.narrator.say(format!("You greet {}.", npc.name));
                let id = npc.id.clone();
                self.scene = Scene::Dialogue { npc, line: 0 };
                self.quest_progress("talk", &id);
                self.stats.npcs_talked += 1;
                return;
            }
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
                self.scene = Scene::FishingSchool;
            }
            Tile::MineEntrance => {
                self.world.dim = crate::world::Dimension::Mines;
                self.narrator.say("You descend the mineshaft. The light dies behind you.");
            }
            Tile::MineExit => {
                self.world.dim = crate::world::Dimension::Surface;
                self.narrator.say("You climb back up to Sentinel's air.");
            }
            Tile::Dock
            | Tile::Water
            | Tile::Well
            | Tile::MineralWater
            | Tile::DeepWater => {
                let (water_kind, biome) = fishing_context(&self.world, nx, ny);
                let f = fish::pick_fish(&mut self.rng_state, fishlist::fish(), &biome, water_kind);
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
                });
            }
            _ => self.narrator.say("Nothing to interact with."),
        }
    }

    /// Sailor on the pier. Until 1000 fish lifetime, he just chats. After
    /// that, he offers to row you out and dive — instantly puts you in
    /// Atlantis.
    fn interact_sailor(&mut self) {
        const GATE: u64 = 1000;
        if self.stats.fish_caught < GATE {
            self.narrator.say(format!(
                "Sailor: \"You've landed {} fish. Bring me a thousand and I'll show you the deep.\"",
                self.stats.fish_caught
            ));
            return;
        }
        self.world.dim = crate::world::Dimension::Atlantis;
        // spawn the player at a fixed Atlantis arrival point so they don't
        // appear inside a coral wall
        self.player.x = 0;
        self.player.y = 0;
        self.narrator
            .say("Sailor: \"Hold your breath. Or don't.\"");
        self.narrator
            .say("*** You dive. Khei opens. Atlantis spreads below you. ***");
    }

    fn mark_seen_around_player(&mut self) {
        const VIEW_W: i32 = 50;
        const VIEW_H: i32 = 18;
        let (px, py) = (self.player.x, self.player.y);
        for dy in -VIEW_H / 2..=VIEW_H / 2 {
            for dx in -VIEW_W / 2..=VIEW_W / 2 {
                let cc = coarse_cell(px + dx, py + dy);
                self.seen_cells.insert(cc);
            }
        }
    }

    fn check_biome_change(&mut self) {
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
        let anim_tick = self.anim_tick;
        let caught_snapshot = self.caught.clone();
        let caught_at_snapshot = self.caught_at.clone();
        match &mut self.scene {
            Scene::Overworld => {
                let area = frame.area();
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(
                        " fishcli  ({}, {}) ",
                        self.player.x, self.player.y
                    ))
                    .border_style(Style::default().fg(Color::Cyan));
                let inner = block.inner(area);
                frame.render_widget(block, area);
                frame.render_widget(
                    WorldView {
                        world: &self.world,
                        player: (self.player.x, self.player.y),
                        player_facing: self.player.facing,
                        tick: anim_tick,
                    },
                    inner,
                );
                if let Some(c) = &self.cast {
                    render_cast_overlay(
                        frame,
                        inner,
                        (self.player.x, self.player.y),
                        c,
                        anim_tick,
                    );
                }
            }
            Scene::RodShop { cursor } => render_rod_shop(
                frame,
                *cursor,
                self.player.rods.max_owned,
                self.player.rods.equipped,
                self.player.valu,
            ),
            Scene::FishingSchool => render_placeholder(
                frame,
                " fishing school ",
                "techniques coming soon\n\nq or esc: leave",
            ),
            Scene::Fishing(g) => {
                // fishing scene gets the whole frame; log is hidden during reel
                g.render(frame, frame.area(), anim_tick);
            }
            Scene::Fishdex(d) => d.render(frame, &caught_snapshot, &caught_at_snapshot),
            Scene::NamePrompt(buf) => render_name_prompt(frame, buf),
            Scene::Dialogue { npc, line } => render_dialogue(frame, npc, *line),
            Scene::Notes(buf) => render_notes(frame, buf),
            Scene::Inventory { tab } => render_inventory(
                frame,
                &self.player.inventory,
                &self.player.items,
                *tab,
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
            ),
            Scene::Settings => render_settings(frame),
            Scene::Quests { cursor } => render_quests(
                frame,
                *cursor,
                &self.quest_progress,
                &self.quest_done,
                self.pinned_quest.as_deref(),
            ),
            Scene::Map { offset } => render_map(
                frame,
                &self.world,
                (self.player.x, self.player.y),
                *offset,
                &self.seen_cells,
            ),
            Scene::Debug { cursor } => render_debug_console(
                frame,
                *cursor,
                self.player.valu,
                self.world.dim,
                &self.stats,
                &self.skills,
                &self.buffs,
            ),
        }

        if matches!(self.scene, Scene::NamePrompt(_)) {
            return;
        }

        let full = frame.area();
        let cmdline_h = 1u16;
        let effective_h = full.height.saturating_sub(cmdline_h);
        // hide log/valu inside the fishing reel scene
        let in_fishing = matches!(self.scene, Scene::Fishing(_));
        if in_fishing {
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
        let log_h = 10u16.min(effective_h);
        if log_w > 4 && log_h > 2 {
            let log_area = Rect {
                x: full.x,
                y: full.y + effective_h - log_h,
                width: log_w,
                height: log_h,
            };
            self.narrator.render(frame, log_area);
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

        if let Some((skill, gained, total_xp, level, _)) = self.xp_popup {
            render_xp_popup(frame, skill, gained, total_xp, level);
        }

        if let Some(id) = self.pinned_quest.as_deref() {
            if let Some(q) = quest::quests().iter().find(|q| q.id == id) {
                if !self.quest_done.contains(&q.id) {
                    let progress = self.quest_progress.get(&q.id).copied().unwrap_or(0);
                    render_pinned_task(frame, q, progress);
                }
            }
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
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" map (arrows or hjkl to pan, q/esc to close) ")
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
    let bg = biome_map_bg(world.biome(x, y));
    let t = world.get(x, y);
    let (g, fg) = match t {
        Tile::Water => ('~', Color::Rgb(120, 170, 220)),
        Tile::Sand => (',', Color::Rgb(220, 200, 145)),
        Tile::Dock => ('=', Color::Rgb(210, 175, 110)),
        Tile::Well => ('O', Color::Rgb(200, 200, 215)),
        Tile::Wall => ('#', Color::Rgb(180, 145, 95)),
        Tile::Roof => ('#', Color::Rgb(200, 100, 70)),
        Tile::DoorRod | Tile::DoorSchool => ('D', Color::Rgb(245, 215, 90)),
        Tile::TreeCanopy | Tile::TreeTrunk => ('T', Color::Rgb(110, 200, 95)),
        Tile::BigRock | Tile::MediumRock | Tile::Rock => ('#', Color::Rgb(170, 170, 170)),
        Tile::Path => ('.', Color::Rgb(195, 170, 130)),
        Tile::Lamppost => ('i', Color::Rgb(240, 215, 130)),
        Tile::Bench => ('=', Color::Rgb(170, 115, 70)),
        Tile::Cactus => ('Y', Color::Rgb(130, 180, 100)),
        Tile::Pebble => ('.', Color::Rgb(170, 160, 130)),
        Tile::Flower => ('*', Color::Rgb(210, 190, 180)),
        Tile::Grass => ('.', Color::Rgb(130, 175, 130)),
        Tile::MineEntrance | Tile::MineFrame => ('#', Color::Rgb(120, 80, 45)),
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
    };
    // water cells override the biome bg with deep blue
    let final_bg = if matches!(t, Tile::Water) {
        Color::Rgb(10, 25, 65)
    } else {
        bg
    };
    (g, Style::default().fg(fg).bg(final_bg).add_modifier(Modifier::BOLD))
}

fn render_pinned_task(frame: &mut Frame, q: &quest::QuestDef, progress: u32) {
    let area = frame.area();
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
) {
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" tasks (j/k navigate, p pin/unpin, q/esc close) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<ratatui::text::Line> = Vec::new();
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
            // borders
            for sy in box_top..=box_bot {
                if sy < area.y as i32 || sy >= (area.y + area.height) as i32 {
                    continue;
                }
                if box_x_left >= area.x as i32 {
                    buf[(box_x_left as u16, sy as u16)]
                        .set_char('|')
                        .set_style(Style::default().fg(Color::Yellow));
                }
                if box_x_right < (area.x + area.width) as i32 {
                    buf[(box_x_right as u16, sy as u16)]
                        .set_char('|')
                        .set_style(Style::default().fg(Color::Yellow));
                }
            }
            for sx in box_x_left..=box_x_right {
                if sx < area.x as i32 || sx >= (area.x + area.width) as i32 {
                    continue;
                }
                if box_top >= area.y as i32 {
                    buf[(sx as u16, box_top as u16)]
                        .set_char('-')
                        .set_style(Style::default().fg(Color::Yellow));
                }
                if box_bot < (area.y + area.height) as i32 {
                    buf[(sx as u16, box_bot as u16)]
                        .set_char('-')
                        .set_style(Style::default().fg(Color::Yellow));
                }
            }
            // 2-tall moving cell inside the box (range: box_top+1..=box_bot-1, with size 2)
            let inner_h = bar_h;
            let marker_top = box_top + 1 + ((1.0 - c.cast_pos) * (inner_h - 2) as f32).round() as i32;
            let marker_bot = marker_top + 1;
            let color = lerp_red_green(c.cast_pos);
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

fn lerp_red_green(frac: f32) -> Color {
    let r = ((1.0 - frac) * 230.0) as u8;
    let g = (frac * 220.0) as u8;
    Color::Rgb(r, g, 30)
}

fn render_rod_shop(
    frame: &mut Frame,
    cursor: u32,
    owned: u32,
    equipped: u32,
    valu: u64,
) {
    use crate::rod::rods;
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " rod shop - {} owned, equipped #{equipped} - j/k browse, enter to buy next, e to equip, q to leave ",
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

fn render_xp_popup(
    frame: &mut Frame,
    skill: &str,
    gained: u64,
    total_xp: u64,
    level: u32,
) {
    use crate::stats::level_to_xp;
    let area = frame.area();
    let w = 48u16.min(area.width);
    let h = 4u16.min(area.height);
    if w < 20 || h < 4 {
        return;
    }
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + 1;
    let rect = Rect { x, y, width: w, height: h };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" +{gained} {skill} xp "))
        .border_style(Style::default().fg(Color::LightGreen));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let cur_floor = level_to_xp(level);
    let next = level_to_xp(level + 1);
    let span = (next - cur_floor).max(1);
    let progress = total_xp.saturating_sub(cur_floor);
    let bar_w = inner.width.saturating_sub(2) as usize;
    let filled = ((progress as f32 / span as f32) * bar_w as f32) as usize;
    let bar: String = std::iter::repeat('=')
        .take(filled)
        .chain(std::iter::repeat('-').take(bar_w.saturating_sub(filled)))
        .collect();
    let lines = vec![
        ratatui::text::Line::from(format!("  Level {level}  ({progress}/{span} xp)")),
        ratatui::text::Line::from(format!(" [{bar}]")),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_location_popup(frame: &mut Frame, label: &str) {
    let area = frame.area();
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
    GrantUniqueFish,
    GrantUniqueIsh,
    GrantUniqueFsh,
    GrantUniqueFih,
    GrantUniqueFis,
    GrantUniqueFallen,
    MarkAllSpecies,
    ClearAllSpecies,
}

fn debug_entries() -> &'static [DebugEntry] {
    &[
        DebugEntry::DimCycle,
        DebugEntry::Valu,
        DebugEntry::LifetimeValu,
        DebugEntry::FishCaught,
        DebugEntry::FishEscaped,
        DebugEntry::FishSold,
        DebugEntry::Casts,
        DebugEntry::Steps,
        DebugEntry::NpcsTalked,
        DebugEntry::QuestsCompleted,
        DebugEntry::FishingXp,
        DebugEntry::WalkingXp,
        DebugEntry::NegotiationXp,
        DebugEntry::MiningXp,
        DebugEntry::WoodcuttingXp,
        DebugEntry::GrantUniqueFish,
        DebugEntry::GrantUniqueIsh,
        DebugEntry::GrantUniqueFsh,
        DebugEntry::GrantUniqueFih,
        DebugEntry::GrantUniqueFis,
        DebugEntry::GrantUniqueFallen,
        DebugEntry::MarkAllSpecies,
        DebugEntry::ClearAllSpecies,
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
                    self.world.dim = match self.world.dim {
                        crate::world::Dimension::Surface => crate::world::Dimension::Mines,
                        crate::world::Dimension::Mines => crate::world::Dimension::Atlantis,
                        crate::world::Dimension::Atlantis => crate::world::Dimension::Surface,
                    };
                }
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
                self.world.dim = match self.world.dim {
                    crate::world::Dimension::Surface => crate::world::Dimension::Mines,
                    crate::world::Dimension::Mines => crate::world::Dimension::Atlantis,
                    crate::world::Dimension::Atlantis => crate::world::Dimension::Surface,
                };
            }
            _ => {}
        }
    }
}

fn render_debug_console(
    frame: &mut Frame,
    cursor: usize,
    valu: u64,
    dim: crate::world::Dimension,
    stats: &Stats,
    skills: &Skills,
    _buffs: &crate::buffs::Buffs,
) {
    use ratatui::widgets::Paragraph;
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" developer console - h/l adjust, H/L big step, enter action, q/esc close ")
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let dim_label = match dim {
        crate::world::Dimension::Surface => "Surface",
        crate::world::Dimension::Mines => "Mines",
        crate::world::Dimension::Atlantis => "Atlantis",
    };
    let rows: Vec<(String, String)> = debug_entries()
        .iter()
        .map(|e| match e {
            DebugEntry::DimCycle => ("Dimension (h/l/enter cycles)".to_string(), dim_label.to_string()),
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
    frame.render_widget(Paragraph::new(lines), inner);
}

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
fn fishing_context(world: &World, x: i32, y: i32) -> (&'static str, String) {
    match world.dim {
        crate::world::Dimension::Surface => {
            (water_kind_at(world, x, y), biome_at(x, y, world.seed).label().to_string())
        }
        crate::world::Dimension::Mines => ("mineral_pool", "Mines".to_string()),
        crate::world::Dimension::Atlantis => ("atlantis", "Atlantis".to_string()),
    }
}

fn direction_for(code: KeyCode) -> Option<(i32, i32)> {
    match code {
        KeyCode::Char('h') | KeyCode::Left => Some((-1, 0)),
        KeyCode::Char('j') | KeyCode::Down => Some((0, 1)),
        KeyCode::Char('k') | KeyCode::Up => Some((0, -1)),
        KeyCode::Char('l') | KeyCode::Right => Some((1, 0)),
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
) {
    let area = frame.area();
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

    lines.push(ratatui::text::Line::from(""));
    lines.push(section("PROGRESS"));
    lines.push(row(
        "Fishdex",
        format!("{}/{} species", unique_caught, total_species),
    ));
    lines.push(row("Fish in basket", fish_in_basket.to_string()));
    lines.push(row("Items picked up", items_picked.to_string()));
    lines.push(row("Quests completed", quests_done.to_string()));

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
    lines.push(row(
        "Walk speed mult",
        format!("x{:.2}", 1.0 / buffs.walk_mult().max(0.01)),
    ));
    lines.push(row(
        "Luck bonus",
        format!("+{:.0}%", buffs.luck_bonus * 100.0),
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

fn render_settings(frame: &mut Frame) {
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" settings (q/esc to close) ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = vec![
        ratatui::text::Line::from(""),
        ratatui::text::Line::from("  No togglable settings yet."),
        ratatui::text::Line::from(""),
        ratatui::text::Line::from(ratatui::text::Span::styled(
            "  Game data is stored at:",
            Style::default().fg(Color::DarkGray),
        )),
        ratatui::text::Line::from(ratatui::text::Span::styled(
            "    ~/.local/share/fishcli/save.json",
            Style::default().fg(Color::DarkGray),
        )),
        ratatui::text::Line::from(ratatui::text::Span::styled(
            "    ~/.local/share/fishcli/notes.txt",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(body).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_help(frame: &mut Frame, topic: HelpTopic) {
    let area = frame.area();
    let (title, lines): (&str, Vec<(&str, &str)>) = match topic {
        HelpTopic::Controls => (
            " controls (q/esc to close) ",
            vec![
                ("h j k l / arrows", "move (and turn to face)"),
                ("f", "interact with what you're facing (door, npc, water)"),
                ("g", "pick up nearby flower / pebble"),
                ("x", "inspect the tile you're facing"),
                ("e", "open fishdex"),
                ("Esc", "switch from Insert -> Normal mode"),
                ("i / a", "switch from Normal -> Insert mode"),
                (":", "in Normal mode, open command line"),
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
) {
    let area = frame.area();
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
            let mut grouped: Vec<(&str, &str, usize)> = Vec::new();
            for f in fish_inv.iter().filter(|f| !f.unique) {
                if let Some((_, _, n)) = grouped.iter_mut().find(|(n, _, _)| *n == f.name.as_str()) {
                    *n += 1;
                } else {
                    grouped.push((f.name.as_str(), f.description.as_str(), 1));
                }
            }
            grouped
                .into_iter()
                .map(|(name, desc, n)| group_line(name, desc, n))
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
    let area = frame.area();
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

fn render_dialogue(frame: &mut Frame, npc: &Npc, line: usize) {
    let area = frame.area();
    let h = 7u16.min(area.height);
    let w = area.width;
    let box_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(h),
        width: w,
        height: h,
    };
    frame.render_widget(Clear, box_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", npc.name))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let total = npc.dialogue.len();
    let body = npc.dialogue.get(line).map(String::as_str).unwrap_or("");
    let footer = if line + 1 >= total {
        "(enter/space to leave)".to_string()
    } else {
        format!("({}/{} - enter/space to continue, q to leave)", line + 1, total)
    };
    let p = Paragraph::new(vec![
        ratatui::text::Line::from(body),
        ratatui::text::Line::from(""),
        ratatui::text::Line::from(footer),
    ])
    .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_name_prompt(frame: &mut Frame, buf: &str) {
    let area = frame.area();
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

fn render_placeholder(frame: &mut Frame, title: &str, body: &str) {
    let area = frame.area();
    let widget = Paragraph::new(body.to_owned()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title.to_owned())
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(widget, area);
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
