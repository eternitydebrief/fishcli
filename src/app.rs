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

const MOVE_INTERVAL: u64 = 4;

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
        narrator.say("hjkl/arrows: move    x: inspect    e: fishdex    esc: normal");
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
            held_dir: None,
            held_until_tick: 0,
            last_step_tick: 0,
        }
    }

    fn quest_progress(&mut self, kind: &str, target: &str) {
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
                self.quest_done.push(q.id.clone());
                self.narrator.say(format!(
                    "Quest complete: {} (+{}$V)",
                    q.title, q.reward.valu
                ));
            } else {
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
            play_time_secs: 0,
            lifetime_valu_earned: 0,
            quest_progress: self
                .quest_progress
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            quest_done: self.quest_done.clone(),
            items: self.player.items.clone(),
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
                } else if self.anim_tick.saturating_sub(self.last_step_tick) >= MOVE_INTERVAL {
                    self.step(dir.0, dir.1);
                    self.last_step_tick = self.anim_tick;
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
                self.step(dir.0, dir.1);
                self.held_dir = Some(dir);
                self.held_until_tick = self.anim_tick + 5;
                self.last_step_tick = self.anim_tick;
            }
            KeyEventKind::Repeat => {
                self.held_dir = Some(dir);
                self.held_until_tick = self.anim_tick + 5;
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
            "q!" => {
                self.running = false;
            }
            "s" => self.narrator.say("Stats screen - coming in a later commit."),
            "q-list" | "quests" => self.list_quests(),
            "m" => self.narrator.say("Settings - coming in a later commit."),
            "e" => {
                self.narrator.say("You leaf through the fishdex.");
                self.scene = Scene::Fishdex(Fishdex::new());
            }
            "n" | "notes" => {
                self.narrator.say("You open your notebook.");
                self.scene = Scene::Notes(NotesBuf::from_text(&notes::load()));
            }
            "i" | "inv" | "inventory" => {
                self.scene = Scene::Inventory { tab: 0 };
            }
            "h" | "help" => self
                .narrator
                .say("commands: :w  :wq  :q  :q!  :s  :m  :e  :h"),
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
            self.narrator.say("All quests complete!");
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
            _ => {}
        }
    }

    fn inspect_surroundings(&mut self) {
        let here = self.world.get(self.player.x, self.player.y);
        let mut described = false;
        for (dx, dy, label) in [
            (0, 0, "Here"),
            (0, -1, "North"),
            (0, 1, "South"),
            (-1, 0, "West"),
            (1, 0, "East"),
        ] {
            let t = self.world.get(self.player.x + dx, self.player.y + dy);
            if (dx, dy) == (0, 0) {
                // skip if the player stands on plain grass / sand alone
                if matches!(here, Tile::Grass) {
                    continue;
                }
            } else if matches!(t, Tile::Grass) {
                continue;
            }
            self.narrator
                .say(format!("{label}: {}", t.describe()));
            described = true;
        }
        if !described {
            self.narrator.say("Nothing noteworthy nearby.");
        }
    }

    fn step(&mut self, dx: i32, dy: i32) {
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
            t if t.walkable() => {
                self.player.x = nx;
                self.player.y = ny;
                self.check_biome_change();
            }
            _ => {}
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
    }
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
        Category::Fish => fish_inv
            .iter()
            .map(|f| {
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        f.name.clone(),
                        Style::default().fg(Color::LightYellow),
                    ),
                    ratatui::text::Span::raw("  - "),
                    ratatui::text::Span::raw(f.description.clone()),
                ])
            })
            .collect(),
        other => items
            .iter()
            .filter(|it| it.category == other)
            .map(|it| {
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        it.name.clone(),
                        Style::default().fg(Color::LightYellow),
                    ),
                    ratatui::text::Span::raw("  - "),
                    ratatui::text::Span::raw(it.description.clone()),
                ])
            })
            .collect(),
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
