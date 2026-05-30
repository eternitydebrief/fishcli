use crossterm::event::KeyEventKind;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

pub enum FishingResult {
    Caught,
    Escaped,
}

const SCENE_W: usize = 25;
const SCENE_H: usize = 18;

fn scene_water(x: usize, y: usize, tick: u64) -> (char, Style) {
    let phase = (x as u64 + (y as u64) * 3 + tick / 4) % 12;
    let glyph = match phase {
        0 | 1 => '~',
        2 | 3 => '=',
        4 => '-',
        5..=8 => '~',
        9 => '-',
        _ => '~',
    };
    let color = match phase {
        0..=2 => Color::Blue,
        3..=5 => Color::LightBlue,
        6..=8 => Color::Cyan,
        _ => Color::Blue,
    };
    (glyph, Style::default().fg(color))
}

pub struct Fishing {
    pub bar_h: usize,
    pub rect_h: f32,
    pub rect_y: f32,
    pub rect_vy: f32,
    pub fish_y: f32,
    pub fish_target_y: f32,
    pub fish_speed: f32,
    pub progress: f32,
    pub finished: Option<FishingResult>,
    up_held: bool,
    down_held: bool,
    up_held_until: u32,
    down_held_until: u32,
    rng_state: u32,
    tick_count: u32,
}

impl Fishing {
    pub fn new() -> Self {
        Self {
            bar_h: 22,
            rect_h: 5.0,
            rect_y: 8.5,
            rect_vy: 0.0,
            fish_y: 11.0,
            fish_target_y: 11.0,
            fish_speed: 0.35,
            progress: 50.0,
            finished: None,
            up_held: false,
            down_held: false,
            up_held_until: 0,
            down_held_until: 0,
            rng_state: 0x9E37_79B9,
            tick_count: 0,
        }
    }

    pub fn input_up(&mut self, kind: KeyEventKind) {
        match kind {
            KeyEventKind::Press => {
                self.rect_vy -= 0.35;
                self.up_held = true;
                self.up_held_until = self.tick_count + 18;
            }
            KeyEventKind::Repeat => {
                self.up_held = true;
                self.up_held_until = self.tick_count + 8;
            }
            KeyEventKind::Release => {
                self.up_held = false;
            }
        }
    }

    pub fn input_down(&mut self, kind: KeyEventKind) {
        match kind {
            KeyEventKind::Press => {
                self.rect_vy += 0.35;
                self.down_held = true;
                self.down_held_until = self.tick_count + 18;
            }
            KeyEventKind::Repeat => {
                self.down_held = true;
                self.down_held_until = self.tick_count + 8;
            }
            KeyEventKind::Release => {
                self.down_held = false;
            }
        }
    }

    pub fn tick(&mut self) {
        if self.finished.is_some() {
            return;
        }
        self.tick_count += 1;

        if self.up_held && self.tick_count > self.up_held_until {
            self.up_held = false;
        }
        if self.down_held && self.tick_count > self.down_held_until {
            self.down_held = false;
        }
        if self.up_held {
            self.rect_vy -= 0.45;
        }
        if self.down_held {
            self.rect_vy += 0.45;
        }

        let gravity = 0.22;
        let damping = 0.85;
        self.rect_vy += gravity;
        self.rect_vy *= damping;
        self.rect_y += self.rect_vy;

        let max_top = (self.bar_h as f32) - self.rect_h;
        if self.rect_y < 0.0 {
            self.rect_y = 0.0;
            self.rect_vy = 0.0;
        }
        if self.rect_y > max_top {
            self.rect_y = max_top;
            self.rect_vy = 0.0;
        }

        if self.tick_count % 35 == 0 {
            self.fish_target_y = self.next_target();
        }
        let dy = self.fish_target_y - self.fish_y;
        if dy.abs() > self.fish_speed {
            self.fish_y += self.fish_speed * dy.signum();
        } else {
            self.fish_y = self.fish_target_y;
        }

        let in_rect = self.fish_y >= self.rect_y && self.fish_y <= self.rect_y + self.rect_h;
        if in_rect {
            self.progress += 0.8;
        } else {
            self.progress -= 0.4;
        }
        if self.progress >= 100.0 {
            self.progress = 100.0;
            self.finished = Some(FishingResult::Caught);
        } else if self.progress <= 0.0 {
            self.progress = 0.0;
            self.finished = Some(FishingResult::Escaped);
        }
    }

    fn next_target(&mut self) -> f32 {
        let mut s = self.rng_state;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.rng_state = s;
        (s as f32 / u32::MAX as f32) * (self.bar_h as f32 - 1.0)
    }

    pub fn render(&self, frame: &mut Frame, anim_tick: u64) {
        let area = frame.area();
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(" fishing ")
            .border_style(Style::default().fg(Color::Cyan));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SCENE_W as u16 + 2),
                Constraint::Length(5),
                Constraint::Min(20),
            ])
            .split(inner);

        let scene = Paragraph::new(self.render_scene(anim_tick)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" dock ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(scene, chunks[0]);

        let bar = Paragraph::new(self.render_bar())
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" line ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
        frame.render_widget(bar, chunks[1]);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(chunks[2]);

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" catch "))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(self.progress as u16);
        frame.render_widget(gauge, right[0]);

        let (msg, color) = match &self.finished {
            Some(FishingResult::Caught) => ("caught it!  esc/q to leave", Color::Green),
            Some(FishingResult::Escaped) => ("got away.  esc/q to leave", Color::Red),
            None => ("hold up/down to pull line    esc/q: leave", Color::White),
        };
        let status = Paragraph::new(msg).style(Style::default().fg(color)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" status "),
        );
        frame.render_widget(status, right[1]);
    }

    fn render_scene(&self, anim_tick: u64) -> Vec<Line<'static>> {
        const PLAYER_POS: (usize, usize) = (16, 4);
        const ROD_TIP: (usize, usize) = (10, 10);
        const WATER_TOP: usize = 11;
        const WATER_BOT: usize = SCENE_H - 1;

        let max_fish = (self.bar_h as f32 - 1.0).max(1.0);
        let water_rows = (WATER_BOT - WATER_TOP) as f32;
        let fish_row = WATER_TOP
            + ((self.fish_y / max_fish) * water_rows).round() as usize;
        let fish_row = fish_row.clamp(WATER_TOP + 1, WATER_BOT);

        (0..SCENE_H)
            .map(|y| {
                let spans = (0..SCENE_W)
                    .map(|x| {
                        let (g, s) = self.scene_cell(x, y, PLAYER_POS, ROD_TIP, fish_row, anim_tick);
                        Span::styled(g.to_string(), s)
                    })
                    .collect::<Vec<_>>();
                Line::from(spans)
            })
            .collect()
    }

    fn scene_cell(
        &self,
        x: usize,
        y: usize,
        player: (usize, usize),
        rod_tip: (usize, usize),
        fish_row: usize,
        anim_tick: u64,
    ) -> (char, Style) {
        let line_col = rod_tip.0;
        let water_top = 11usize;

        if (x, y) == player {
            return (
                '@',
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        }

        if y > player.1 && y <= rod_tip.1 {
            let dy = (y - player.1) as i32;
            let total_dy = (rod_tip.1 - player.1) as i32;
            let dx = ((player.0 as i32 - rod_tip.0 as i32) * dy + total_dy / 2) / total_dy;
            let rod_x = player.0 as i32 - dx;
            if rod_x >= 0 && x == rod_x as usize {
                let glyph = if y == rod_tip.1 { '\\' } else { '\\' };
                return (glyph, Style::default().fg(Color::Yellow));
            }
        }

        if y == rod_tip.1 {
            if x == line_col {
                return (
                    'o',
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                );
            }
            return ('=', Style::default().fg(Color::LightYellow));
        }

        if x == line_col && y > rod_tip.1 && y < fish_row {
            return ('|', Style::default().fg(Color::Gray));
        }
        if x == line_col && y == fish_row {
            let color = match &self.finished {
                Some(FishingResult::Caught) => Color::Green,
                Some(FishingResult::Escaped) => Color::DarkGray,
                None => Color::Red,
            };
            return (
                'f',
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            );
        }

        if y >= water_top {
            return scene_water(x, y, anim_tick);
        }

        (' ', Style::default())
    }

    fn render_bar(&self) -> Vec<Line<'static>> {
        let rect_top = self.rect_y.floor() as i32;
        let rect_bot = (self.rect_y + self.rect_h).floor() as i32;
        let fish_pos = self.fish_y.round() as i32;
        let mut lines = Vec::with_capacity(self.bar_h + 2);
        lines.push(Line::from(Span::styled(
            "┌─┐",
            Style::default().fg(Color::DarkGray),
        )));
        for y in 0..self.bar_h as i32 {
            let in_rect = y >= rect_top && y < rect_bot;
            let on_fish = y == fish_pos;
            let cell = match (in_rect, on_fish) {
                (true, true) => Span::styled(
                    "f",
                    Style::default()
                        .fg(Color::Green)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                (true, false) => Span::styled("█", Style::default().fg(Color::Cyan)),
                (false, true) => Span::styled(
                    "f",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                _ => Span::raw(" "),
            };
            lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(Color::DarkGray)),
                cell,
                Span::styled("│", Style::default().fg(Color::DarkGray)),
            ]));
        }
        lines.push(Line::from(Span::styled(
            "└─┘",
            Style::default().fg(Color::DarkGray),
        )));
        lines
    }
}
