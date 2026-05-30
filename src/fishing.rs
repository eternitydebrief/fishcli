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

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(" fishing ")
            .border_style(Style::default().fg(Color::Cyan));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(9), Constraint::Min(20)])
            .split(inner);

        let bar = Paragraph::new(self.render_bar()).alignment(Alignment::Center);
        frame.render_widget(bar, chunks[0]);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(chunks[1]);

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" catch "))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(self.progress as u16);
        frame.render_widget(gauge, right[0]);

        let (msg, color) = match &self.finished {
            Some(FishingResult::Caught) => ("caught it!  esc/q to leave", Color::Green),
            Some(FishingResult::Escaped) => ("got away.  esc/q to leave", Color::Red),
            None => ("k/up: pull up    j/down: pull down", Color::White),
        };
        let status = Paragraph::new(msg).style(Style::default().fg(color)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" status "),
        );
        frame.render_widget(status, right[1]);
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
