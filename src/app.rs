use crate::fish;
use crate::fishdex::Fishdex;
use crate::fishing::{Fishing, FishingResult};
use crate::fishlist;
use crate::narrator::Narrator;
use crate::npc::{self, Npc};
use crate::player::Player;
use crate::save::{self, SaveData};
use crate::world::{Tile, World, WorldView};
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
            held_dir: None,
            held_until_tick: 0,
            last_step_tick: 0,
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
            "m" => self.narrator.say("Settings - coming in a later commit."),
            "e" => {
                self.narrator.say("You leaf through the fishdex.");
                self.scene = Scene::Fishdex(Fishdex::new());
            }
            "h" | "help" => self
                .narrator
                .say("commands: :w  :wq  :q  :q!  :s  :m  :e  :h"),
            "" => {}
            other => self.narrator.say(format!("Unknown command: :{other}")),
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
                    self.narrator
                        .say(format!("You reel in a {}!", fish_ref.name));
                    self.narrator.say(format!(
                        "Added to your basket ({} fish).",
                        self.player.inventory.len()
                    ));
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
            self.scene = Scene::Dialogue { npc, line: 0 };
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
            }
            _ => {}
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
