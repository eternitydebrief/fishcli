//! Boss fishing minigame.
//!
//! Two fish at once, two vertical bars. Player controls each bar
//! independently:
//!   * Left  bar: `F` pulls the rectangle up, `V` pulls it down.
//!   * Right bar: `J` pulls the rectangle up, `N` pulls it down.
//!
//! Each fish drifts up/down inside its bar. When the rectangle overlaps
//! the fish, that bar's catch meter fills; otherwise it bleeds. Win when
//! *both* meters reach 1.0. Player gets 5 attempts; losing all attempts
//! ends the boss fight as a defeat.
//!
//! A 3-5s pre-fight overlay shows the controls before the first attempt
//! so the player has time to plant their fingers.

use crate::fish::FishDef;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
};

pub enum BossResult {
    Won,
    Lost,
}

pub struct BossBar {
    /// Vertical position of the player rectangle's top (0..=bar_h-rect_h).
    pub rect_y: f32,
    pub rect_h: f32,
    pub fish_y: f32,
    fish_target_y: f32,
    fish_speed: f32,
    target_change_ticks: u32,
    pub progress: f32,
    rng_state: u32,
}

impl BossBar {
    fn new(bar_h: f32, rect_h: f32, rng_seed: u32, fish_speed: f32) -> Self {
        Self {
            rect_y: bar_h * 0.4,
            rect_h,
            fish_y: bar_h * 0.5,
            fish_target_y: bar_h * 0.5,
            fish_speed,
            target_change_ticks: 30,
            // Start at 30% so the player has a buffer to find the fish
            // without insta-failing all 5 attempts on the opening frame.
            progress: 0.30,
            rng_state: rng_seed | 1,
        }
    }

    fn tick(&mut self, bar_h: f32) {
        // Fish picks a new target every ~1.5s.
        if self.target_change_ticks == 0 {
            let r = crate::fish::next_rand_f32(&mut self.rng_state);
            self.fish_target_y = r * (bar_h - 1.0);
            self.target_change_ticks = 25 + (crate::fish::next_rand_f32(&mut self.rng_state) * 25.0) as u32;
        } else {
            self.target_change_ticks -= 1;
        }
        let dy = self.fish_target_y - self.fish_y;
        let step = dy.signum() * self.fish_speed.min(dy.abs());
        self.fish_y = (self.fish_y + step).clamp(0.0, bar_h - 1.0);
        // Catch meter: in-rect = up, else slow drain.
        let overlap = self.fish_y >= self.rect_y && self.fish_y <= self.rect_y + self.rect_h;
        if overlap {
            self.progress = (self.progress + 0.006).min(1.0);
        } else {
            self.progress = (self.progress - 0.0025).max(0.0);
        }
    }

    fn pull_up(&mut self) {
        self.rect_y = (self.rect_y - 0.6).max(0.0);
    }
    fn pull_down(&mut self, bar_h: f32) {
        self.rect_y = (self.rect_y + 0.6).min(bar_h - self.rect_h);
    }
}

pub struct Boss {
    pub fish_a: &'static FishDef,
    pub fish_b: &'static FishDef,
    pub bar_h: f32,
    pub left: BossBar,
    pub right: BossBar,
    pub attempts_left: u32,
    /// Intro overlay timer in ticks. While >0, the player input is locked
    /// and a control reminder is shown.
    pub intro_ticks: u32,
    /// Cooldown ticks before another attempt can be deducted. Guards
    /// against a single dropped bar costing multiple attempts in one
    /// second.
    attempt_cooldown: u32,
    pub finished: Option<BossResult>,
}

impl Boss {
    pub fn new(
        fish_a: &'static FishDef,
        fish_b: &'static FishDef,
        rng_seed: u32,
        fishing_level: u32,
    ) -> Self {
        let bar_h = 22.0;
        // Big fish are *fast*; +1% per difficulty point.
        let base = 0.15 + 0.02 * (fish_a.difficulty.max(fish_b.difficulty) as f32);
        let lvl = (fishing_level as f32 * 0.005).min(0.4);
        let speed = (base - lvl).max(0.08);
        Self {
            fish_a,
            fish_b,
            bar_h,
            left: BossBar::new(bar_h, 4.5, rng_seed ^ 0xDEAD_F00D, speed),
            right: BossBar::new(bar_h, 4.5, rng_seed ^ 0xBEEF_CAFE, speed),
            attempts_left: 5,
            intro_ticks: 80, // 4 seconds at 20fps
            attempt_cooldown: 0,
            finished: None,
        }
    }

    pub fn tick(&mut self) {
        if self.finished.is_some() {
            return;
        }
        if self.intro_ticks > 0 {
            self.intro_ticks -= 1;
            return;
        }
        self.left.tick(self.bar_h);
        self.right.tick(self.bar_h);
        if self.left.progress >= 1.0 && self.right.progress >= 1.0 {
            self.finished = Some(BossResult::Won);
            return;
        }
        // Lose an attempt when either meter empties out. The cooldown
        // guarantees one missed bar can't bleed multiple attempts in a
        // single second — the player has to actually fail again after a
        // 2-second grace before another attempt comes off the stack.
        if self.attempt_cooldown > 0 {
            self.attempt_cooldown -= 1;
        }
        if self.attempt_cooldown == 0
            && (self.left.progress <= 0.0 || self.right.progress <= 0.0)
        {
            self.attempts_left = self.attempts_left.saturating_sub(1);
            if self.attempts_left == 0 {
                self.finished = Some(BossResult::Lost);
            } else {
                self.left.progress = 0.4;
                self.right.progress = 0.4;
                self.attempt_cooldown = 40; // 2 s before another attempt can drop
            }
        }
    }

    pub fn input_left_up(&mut self)  { if self.intro_ticks == 0 { self.left.pull_up(); } }
    pub fn input_left_down(&mut self) { if self.intro_ticks == 0 { self.left.pull_down(self.bar_h); } }
    pub fn input_right_up(&mut self) { if self.intro_ticks == 0 { self.right.pull_up(); } }
    pub fn input_right_down(&mut self) { if self.intro_ticks == 0 { self.right.pull_down(self.bar_h); } }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " BOSS - {} & {}  (attempts left: {}) ",
                self.fish_a.name, self.fish_b.name, self.attempts_left
            ))
            .border_style(Style::default().fg(Color::Red));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.intro_ticks > 0 {
            // Pre-fight overlay: control reminder. Always centered in the
            // inner area, regardless of bar layout below.
            let secs = (self.intro_ticks as f32 / 20.0).ceil() as u32;
            let lines = vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(format!("  Boss fight in {}...", secs)),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from("  LEFT  bar: F = pull up,  V = pull down"),
                ratatui::text::Line::from("  RIGHT bar: J = pull up,  N = pull down"),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from("  Keep both fish inside their boxes until both bars fill."),
                ratatui::text::Line::from("  Five attempts before they get away."),
            ];
            frame.render_widget(
                Paragraph::new(lines).alignment(Alignment::Left),
                inner,
            );
            return;
        }

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);
        self.render_bar(frame, cols[0], &self.left, self.fish_a, true);
        self.render_bar(frame, cols[1], &self.right, self.fish_b, false);
    }

    fn render_bar(&self, frame: &mut Frame, area: Rect, bar: &BossBar, fish: &FishDef, is_left: bool) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(2)])
            .split(area);
        let bar_area = rows[0];
        let meter_area = rows[1];
        // Render the vertical column as text.
        let h = bar_area.height as usize;
        let scale = (self.bar_h as usize).max(1) as f32 / h.max(1) as f32;
        let mut lines: Vec<ratatui::text::Line> = Vec::with_capacity(h);
        for row in 0..h {
            let y = row as f32 * scale;
            let in_rect = y >= bar.rect_y && y <= bar.rect_y + bar.rect_h;
            let is_fish = (y - bar.fish_y).abs() <= scale * 0.6;
            let (ch, style) = if is_fish {
                ('o', Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))
            } else if in_rect {
                ('|', Style::default().fg(Color::Cyan))
            } else {
                ('.', Style::default().fg(Color::DarkGray))
            };
            let label = format!("    {ch}    ");
            lines.push(ratatui::text::Line::from(
                ratatui::text::Span::styled(label, style),
            ));
        }
        let side = if is_left { "L" } else { "R" };
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {side}: {} ", fish.name))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(para, bar_area);
        let pct = (bar.progress * 100.0) as u16;
        let g = Gauge::default()
            .gauge_style(Style::default().fg(Color::LightGreen).bg(Color::Rgb(20, 20, 20)))
            .label(format!("{}%", pct))
            .percent(pct.min(100));
        frame.render_widget(g, meter_area);
    }
}
