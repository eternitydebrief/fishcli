use crate::fish::FishDef;
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
const BITE_WINDOW_TICKS: u32 = 40; // 2 seconds at 20 fps

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
    pub fish: &'static FishDef,
    pub fishing_level: u32,
    pub rod_tier: u32,
    pub bar_h: usize,
    pub rect_h: f32,
    pub rect_y: f32,
    pub rect_vy: f32,
    pub fish_y: f32,
    pub fish_target_y: f32,
    pub fish_speed: f32,
    pub target_change_ticks: u32,
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
    pub fn new(
        fish: &'static FishDef,
        rng_seed: u32,
        fishing_level: u32,
        rod_tier: u32,
    ) -> Self {
        let bar_h = 22usize;
        let rect_h = fish.rect_h();
        let rect_y = (bar_h as f32 - rect_h) / 2.0;
        let mid = bar_h as f32 / 2.0;
        let rod_mult = 0.99f32.powi(rod_tier as i32);
        Self {
            fish,
            fishing_level,
            rod_tier,
            bar_h,
            rect_h,
            rect_y,
            rect_vy: 0.0,
            fish_y: mid,
            fish_target_y: mid,
            fish_speed: fish.fish_speed() * rod_mult,
            target_change_ticks: fish.target_change_ticks().max(1),
            progress: 50.0,
            finished: None,
            up_held: false,
            down_held: false,
            up_held_until: 0,
            down_held_until: 0,
            rng_state: if rng_seed == 0 { 0x9E37_79B9 } else { rng_seed },
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
        if self.tick_count % self.target_change_ticks == 0 {
            self.fish_target_y = self.next_target();
        }
        let dy = self.fish_target_y - self.fish_y;
        if dy.abs() > self.fish_speed {
            self.fish_y += self.fish_speed * dy.signum();
        } else {
            self.fish_y = self.fish_target_y;
        }
        let in_rect = self.fish_y >= self.rect_y && self.fish_y <= self.rect_y + self.rect_h;
        let fishing_mult = 1.0 + (self.fishing_level as f32) * 0.0025;
        if in_rect {
            self.progress += 0.8 * fishing_mult;
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
        let r = next_rand_f32(&mut self.rng_state);
        r * (self.bar_h as f32 - 1.0)
    }

    pub fn render(&self, frame: &mut Frame, anim_tick: u64) {
        let area = frame.area();
        let stars = "*".repeat(self.fish.difficulty as usize);
        let title = format!(" fishing - {} {} ", self.fish.name, stars);
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        self.render_reeling(frame, inner, anim_tick);
    }

    fn render_reeling(
        &self,
        frame: &mut Frame,
        inner: ratatui::layout::Rect,
        anim_tick: u64,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(5), Constraint::Min(15)])
            .split(inner);
        let (bar_idx, right_idx) = (0usize, 1usize);

        let _ = anim_tick;
        let bar = Paragraph::new(self.render_bar())
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" line ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
        frame.render_widget(bar, chunks[bar_idx]);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(chunks[right_idx]);

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" catch "))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(self.progress as u16);
        frame.render_widget(gauge, right[0]);

        let (msg, color): (String, Color) = match &self.finished {
            Some(FishingResult::Caught) => (
                format!("caught a {}!  {}", self.fish.name, self.fish.description),
                Color::Green,
            ),
            Some(FishingResult::Escaped) => (
                format!("the {} got away.", self.fish.name),
                Color::Red,
            ),
            None => (
                "hold up/down to pull line    esc/q: leave".into(),
                Color::White,
            ),
        };
        let status = Paragraph::new(msg).style(Style::default().fg(color)).block(
            Block::default().borders(Borders::ALL).title(" status "),
        );
        frame.render_widget(status, right[1]);
    }

    fn render_bar(&self) -> Vec<Line<'static>> {
        let rect_top = self.rect_y.floor() as i32;
        let rect_bot = (self.rect_y + self.rect_h).floor() as i32;
        let fish_pos = self.fish_y.round() as i32;
        let mut lines = Vec::with_capacity(self.bar_h + 2);
        lines.push(Line::from(Span::styled(
            "+-+",
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
                (true, false) => Span::styled("#", Style::default().fg(Color::Cyan)),
                (false, true) => Span::styled(
                    "f",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                _ => Span::raw(" "),
            };
            lines.push(Line::from(vec![
                Span::styled("|", Style::default().fg(Color::DarkGray)),
                cell,
                Span::styled("|", Style::default().fg(Color::DarkGray)),
            ]));
        }
        lines.push(Line::from(Span::styled(
            "+-+",
            Style::default().fg(Color::DarkGray),
        )));
        lines
    }
}

fn next_rand_f32(s: &mut u32) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    (x as f32) / (u32::MAX as f32)
}

fn lerp_red_green(frac: f32) -> Color {
    // frac=0 → red, frac=1 → green
    let r = ((1.0 - frac) * 230.0) as u8;
    let g = (frac * 220.0) as u8;
    Color::Rgb(r, g, 30)
}
