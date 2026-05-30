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
use std::collections::HashMap;
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
    RodShop,
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
    pub narrator: Narrator,
    /// quest id -> progress count
    pub quest_progress: HashMap<String, u32>,
    /// quest ids that have been completed and rewarded
    pub quest_done: Vec<String>,
    /// most recent biome the player stepped into
    pub current_biome: Option<Biome>,
    /// shown when biome changes, ticks down to 0
    pub biome_popup_ticks: u32,
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
            narrator,
            quest_progress: HashMap::new(),
            quest_done: Vec::new(),
            current_biome: None,
            biome_popup_ticks: 0,
            lifetime_valu: 0,
            session_start: std::time::Instant::now(),
            saved_play_secs: 0,
            pinned_quest: None,
            seen_cells: std::collections::HashSet::new(),
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
        self.world = World::new(data.world_seed);
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
        }
    }

    pub fn do_save(&mut self) -> bool {
        let data = self.current_save();
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

    pub fn tick(&mut self) {
        self.anim_tick = self.anim_tick.wrapping_add(1);
        if self.biome_popup_ticks > 0 {
            self.biome_popup_ticks -= 1;
        }

        let movement_allowed =
            matches!(self.mode, Mode::Insert) && matches!(self.scene, Scene::Overworld);
        if movement_allowed {
            if let Some(dir) = self.held_dir {
                if self.anim_tick > self.held_until_tick {
                    self.held_dir = None;
                } else {
                    let interval = if dir.1 == 0 {
                        MOVE_INTERVAL_H
                    } else {
                        MOVE_INTERVAL_V
                    };
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
            Scene::RodShop | Scene::FishingSchool => {
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
            "" => {}
            other => self.narrator.say(format!("Unknown command: :{other}")),
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
                    if let Some(i) = fishlist::fish().iter().position(|f| std::ptr::eq(f, fish_ref)) {
                        self.caught[i] = true;
                    }
                    self.player.inventory.push(fish_ref);
                    let name = fish_ref.name.clone();
                    self.narrator
                        .say(format!("You reel in a {}!", name));
                    self.narrator.say(format!(
                        "Added to your basket ({} fish).",
                        self.player.inventory.len()
                    ));
                    self.quest_progress("catch", &name);
                } else if escaped {
                    self.narrator
                        .say(format!("The {} slips away.", fish_ref.name));
                } else {
                    self.narrator.say("You leave the line slack and step away.");
                }
            }
            Scene::RodShop | Scene::FishingSchool => {
                self.narrator.say("You step back outside.");
            }
            _ => {}
        }
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
            KeyCode::Char('f') => self.interact_facing(),
            _ => {}
        }
    }

    fn inspect_surroundings(&mut self) {
        let (dx, dy) = self.player.facing;
        let tx = self.player.x + dx;
        let ty = self.player.y + dy;
        if let Some(npc) = npc::npc_at(tx, ty) {
            self.narrator.say(format!("{}: {}", npc.name, "An ordinary villager. Press f to talk."));
            return;
        }
        let t = self.world.get(tx, ty);
        self.narrator.say(t.describe());
    }

    fn step(&mut self, dx: i32, dy: i32) {
        self.player.facing = (dx, dy);
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        if npc::npc_at(nx, ny).is_some() {
            return; // blocked by NPC; press f to interact
        }
        let t = self.world.get(nx, ny);
        if t.walkable() {
            self.player.x = nx;
            self.player.y = ny;
            self.check_biome_change();
            self.mark_seen_around_player();
            let weight = if dy != 0 { 2 } else { 1 };
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
                return;
            }
        }
        self.narrator.say("Nothing to pick up here.");
    }

    fn interact_facing(&mut self) {
        let (dx, dy) = self.player.facing;
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        if let Some(npc) = npc::npc_at(nx, ny) {
            self.narrator.say(format!("You greet {}.", npc.name));
            let id = npc.id.clone();
            self.scene = Scene::Dialogue { npc, line: 0 };
            self.quest_progress("talk", &id);
            return;
        }
        match self.world.get(nx, ny) {
            Tile::DoorRod => {
                self.narrator.say("You step into the rod shop.");
                self.scene = Scene::RodShop;
            }
            Tile::DoorSchool => {
                self.narrator.say("You step into the fishing school.");
                self.scene = Scene::FishingSchool;
            }
            Tile::Dock | Tile::Water | Tile::Well => {
                let f = fish::pick_fish(&mut self.rng_state, fishlist::fish());
                let spot = match self.world.get(nx, ny) {
                    Tile::Dock => "off the dock",
                    Tile::Water => "into the water",
                    Tile::Well => "down the well",
                    _ => "out",
                };
                self.narrator.say(format!("You cast {spot}."));
                self.narrator
                    .say(format!("Something tugs the line - a {}!", f.name));
                self.scene = Scene::Fishing(Fishing::new(f, self.rng_state));
            }
            _ => self.narrator.say("Nothing to interact with."),
        }
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
        if self.current_biome != Some(b) {
            self.current_biome = Some(b);
            self.biome_popup_ticks = 90; // ~3s at 30fps
            self.narrator.say(format!("Entered: {}", b.label()));
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let anim_tick = self.anim_tick;
        let caught_snapshot = self.caught.clone();
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
            }
            Scene::RodShop => render_placeholder(
                frame,
                " rod shop ",
                "rod upgrades coming soon\n\nq or esc: leave",
            ),
            Scene::FishingSchool => render_placeholder(
                frame,
                " fishing school ",
                "techniques coming soon\n\nq or esc: leave",
            ),
            Scene::Fishing(g) => g.render(frame, anim_tick),
            Scene::Fishdex(d) => d.render(frame, &caught_snapshot),
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
        }

        if matches!(self.scene, Scene::NamePrompt(_)) {
            return;
        }

        let full = frame.area();
        let cmdline_h = 1u16;
        let effective_h = full.height.saturating_sub(cmdline_h);

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
            if let Some(b) = self.current_biome {
                render_biome_popup(frame, b);
            }
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

pub const MAP_CELL_W: i32 = 4;
pub const MAP_CELL_H: i32 = 2;

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

fn render_biome_popup(frame: &mut Frame, biome: Biome) {
    let area = frame.area();
    let label = biome.label();
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
    let rows: Vec<(&str, String)> = vec![
        ("Name", who.to_string()),
        ("Play time", play),
        ("Valu", format_valu(valu)),
        ("Lifetime valu earned", format_valu(lifetime_valu)),
        (
            "Fishdex",
            format!("{}/{} species", unique_caught, total_species),
        ),
        ("Fish in basket", fish_in_basket.to_string()),
        ("Items picked up", items_picked.to_string()),
        ("Quests completed", quests_done.to_string()),
    ];

    let lines: Vec<ratatui::text::Line> = rows
        .into_iter()
        .map(|(k, v)| {
            ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("  {:<24}", k),
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                ),
                ratatui::text::Span::raw(v),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
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
            for f in fish_inv {
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
