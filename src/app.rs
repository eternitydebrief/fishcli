use crate::fishing::Fishing;
use crate::map::{Map, Tile};
use crate::player::Player;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub enum Scene {
    Overworld,
    RodShop,
    FishingSchool,
    Fishing(Fishing),
}

pub struct App {
    pub map: Map,
    pub player: Player,
    pub scene: Scene,
    pub running: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            map: Map::starter(),
            player: Player::spawn(),
            scene: Scene::Overworld,
            running: true,
        }
    }

    pub fn tick(&mut self) {
        if let Scene::Fishing(g) = &mut self.scene {
            g.tick();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match &mut self.scene {
            Scene::Overworld => self.handle_overworld(key.code),
            Scene::Fishing(g) => {
                let mut leave = false;
                match key.code {
                    KeyCode::Char('k') | KeyCode::Up => g.push_up(),
                    KeyCode::Char('j') | KeyCode::Down => g.push_down(),
                    KeyCode::Esc | KeyCode::Char('q') => leave = true,
                    _ => {}
                }
                if leave {
                    self.scene = Scene::Overworld;
                }
            }
            Scene::RodShop | Scene::FishingSchool => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.scene = Scene::Overworld;
                }
            }
        }
    }

    fn handle_overworld(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('h') | KeyCode::Left => self.step(-1, 0),
            KeyCode::Char('j') | KeyCode::Down => self.step(0, 1),
            KeyCode::Char('k') | KeyCode::Up => self.step(0, -1),
            KeyCode::Char('l') | KeyCode::Right => self.step(1, 0),
            _ => {}
        }
    }

    fn step(&mut self, dx: i32, dy: i32) {
        let nx = self.player.x as i32 + dx;
        let ny = self.player.y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= self.map.width as i32 || ny >= self.map.height as i32 {
            return;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        match self.map.get(nx, ny) {
            Tile::DoorRod => self.scene = Scene::RodShop,
            Tile::DoorSchool => self.scene = Scene::FishingSchool,
            Tile::Dock => self.scene = Scene::Fishing(Fishing::new()),
            t if t.walkable() => {
                self.player.x = nx;
                self.player.y = ny;
            }
            _ => {}
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        match &self.scene {
            Scene::Overworld => self.render_overworld(frame),
            Scene::RodShop => self.render_placeholder(
                frame,
                " rod shop ",
                "rod upgrades coming soon\n\nesc/q: leave",
            ),
            Scene::FishingSchool => self.render_placeholder(
                frame,
                " fishing school ",
                "techniques coming soon\n\nesc/q: leave",
            ),
            Scene::Fishing(g) => g.render(frame),
        }
    }

    fn render_overworld(&self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);

        let lines = self.map.render_lines(Some((self.player.x, self.player.y)));
        let map_widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" fishcli ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(map_widget, chunks[0]);

        let help = Paragraph::new(
            "hjkl/arrows: move    walk into a door or dock to enter    q: quit",
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(help, chunks[1]);
    }

    fn render_placeholder(&self, frame: &mut Frame, title: &str, body: &str) {
        let area = frame.area();
        let widget = Paragraph::new(body.to_owned()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_owned())
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(widget, area);
    }
}
