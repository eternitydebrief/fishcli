use crate::fish;
use crate::fishdex::Fishdex;
use crate::fishing::{Fishing, FishingResult};
use crate::fishlist::FISH;
use crate::narrator::Narrator;
use crate::player::Player;
use crate::world::{Tile, World};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub enum Scene {
    Overworld,
    RodShop,
    FishingSchool,
    Fishing(Fishing),
    Fishdex(Fishdex),
}

pub struct App {
    pub world: World,
    pub player: Player,
    pub scene: Scene,
    pub running: bool,
    pub anim_tick: u64,
    pub rng_state: u32,
    pub caught: Vec<bool>,
    pub narrator: Narrator,
}

impl App {
    pub fn new() -> Self {
        let mut narrator = Narrator::new();
        narrator.say("You arrive at the village.");
        narrator.say("Yellow D west = rod shop. Pink D east = fishing school. Dock is south.");
        Self {
            world: World::new(0xDEAD_BEEF),
            player: Player::spawn(),
            scene: Scene::Overworld,
            running: true,
            anim_tick: 0,
            rng_state: 0xC0FF_EE42,
            caught: vec![false; FISH.len()],
            narrator,
        }
    }

    pub fn tick(&mut self) {
        self.anim_tick = self.anim_tick.wrapping_add(1);
        if let Scene::Fishing(g) = &mut self.scene {
            g.tick();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match &mut self.scene {
            Scene::Fishing(g) => {
                let mut leave = false;
                match key.code {
                    KeyCode::Char('k') | KeyCode::Up => g.input_up(key.kind),
                    KeyCode::Char('j') | KeyCode::Down => g.input_down(key.kind),
                    KeyCode::Esc | KeyCode::Char('q') if key.kind == KeyEventKind::Press => {
                        leave = true;
                    }
                    _ => {}
                }
                if leave {
                    let fish_ref: &'static crate::fish::FishDef = g.fish;
                    let caught = matches!(g.finished, Some(FishingResult::Caught));
                    if caught {
                        if let Some(i) = FISH.iter().position(|f| std::ptr::eq(f, fish_ref)) {
                            self.caught[i] = true;
                        }
                        self.player.inventory.push(fish_ref);
                        self.narrator
                            .say(format!("You reel in a {}!", fish_ref.name));
                        self.narrator
                            .say(format!("Added to your basket ({} fish).", self.player.inventory.len()));
                    } else if matches!(g.finished, Some(FishingResult::Escaped)) {
                        self.narrator
                            .say(format!("The {} slips away.", fish_ref.name));
                    } else {
                        self.narrator.say("You leave the line slack and step away.");
                    }
                    self.scene = Scene::Overworld;
                }
            }
            Scene::Fishdex(d) => {
                if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
                    return;
                }
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => d.cursor_down(),
                    KeyCode::Char('k') | KeyCode::Up => d.cursor_up(),
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') => {
                        self.scene = Scene::Overworld;
                    }
                    _ => {}
                }
            }
            Scene::Overworld => {
                if key.kind == KeyEventKind::Press {
                    self.handle_overworld(key.code);
                }
            }
            Scene::RodShop | Scene::FishingSchool => {
                if key.kind == KeyEventKind::Press
                    && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                {
                    self.narrator.say("You step back outside.");
                    self.scene = Scene::Overworld;
                }
            }
        }
    }

    fn handle_overworld(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('e') => {
                self.narrator.say("You leaf through the fishdex.");
                self.scene = Scene::Fishdex(Fishdex::new());
            }
            KeyCode::Char('h') | KeyCode::Left => self.step(-1, 0),
            KeyCode::Char('j') | KeyCode::Down => self.step(0, 1),
            KeyCode::Char('k') | KeyCode::Up => self.step(0, -1),
            KeyCode::Char('l') | KeyCode::Right => self.step(1, 0),
            _ => {}
        }
    }

    fn step(&mut self, dx: i32, dy: i32) {
        let nx = self.player.x + dx;
        let ny = self.player.y + dy;
        match self.world.get(nx, ny) {
            Tile::DoorRod => {
                self.narrator.say("You step into the rod shop.");
                self.scene = Scene::RodShop;
            }
            Tile::DoorSchool => {
                self.narrator.say("You step into the fishing school.");
                self.scene = Scene::FishingSchool;
            }
            Tile::Dock => {
                let f = fish::pick_fish(&mut self.rng_state);
                self.narrator.say("You cast your line.");
                self.narrator
                    .say(format!("Something tugs the line — a {}!", f.name));
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
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(3)])
                    .split(area);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(
                        " fishcli  ({}, {}) ",
                        self.player.x, self.player.y
                    ))
                    .border_style(Style::default().fg(Color::Cyan));
                let inner = block.inner(chunks[0]);
                let lines = self.world.render_viewport(
                    (self.player.x, self.player.y),
                    inner.width as usize,
                    inner.height as usize,
                    anim_tick,
                );
                let map_widget = Paragraph::new(lines).block(block);
                frame.render_widget(map_widget, chunks[0]);
                let help = Paragraph::new(
                    "hjkl/arrows: move    e: fishdex    walk into door/dock to enter    q: quit",
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                frame.render_widget(help, chunks[1]);
            }
            Scene::RodShop => render_placeholder(
                frame,
                " rod shop ",
                "rod upgrades coming soon\n\nesc/q: leave",
            ),
            Scene::FishingSchool => render_placeholder(
                frame,
                " fishing school ",
                "techniques coming soon\n\nesc/q: leave",
            ),
            Scene::Fishing(g) => g.render(frame, anim_tick),
            Scene::Fishdex(d) => d.render(frame, &caught_snapshot),
        }
        let full = frame.area();

        let valu_str = format_valu(self.player.valu);
        let valu_w = (valu_str.len() as u16 + 4).max(14).min(full.width);
        let valu_h = 3u16.min(full.height);
        if valu_w >= 8 && valu_h >= 3 {
            let valu_area = Rect {
                x: full.x + full.width - valu_w,
                y: full.y + full.height - valu_h,
                width: valu_w,
                height: valu_h,
            };
            render_valu(frame, valu_area, &valu_str);
        }

        let log_w = 42u16.min(full.width.saturating_sub(valu_w));
        let log_h = 10u16.min(full.height);
        if log_w > 4 && log_h > 2 {
            let log_area = Rect {
                x: full.x,
                y: full.y + full.height - log_h,
                width: log_w,
                height: log_h,
            };
            self.narrator.render(frame, log_area);
        }
    }
}

fn render_valu(frame: &mut Frame, area: Rect, text: &str) {
    use ratatui::widgets::Clear;
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" valu ")
        .border_style(Style::default().fg(Color::Yellow));
    let p = Paragraph::new(text.to_string())
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .alignment(ratatui::layout::Alignment::Right)
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
