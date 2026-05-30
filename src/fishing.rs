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
        let bar_h = 20usize;
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

    pub fn render(&self, frame: &mut Frame, area: ratatui::layout::Rect, anim_tick: u64) {
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
        let show_scene = inner.width >= 38;
        let constraints: Vec<Constraint> = if show_scene {
            vec![
                Constraint::Length(22),
                Constraint::Length(9),
                Constraint::Min(15),
            ]
        } else {
            vec![Constraint::Length(9), Constraint::Min(15)]
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner);
        let (bar_idx, right_idx) = if show_scene {
            (1usize, 2usize)
        } else {
            (0usize, 1usize)
        };
        if show_scene {
            let panel = Paragraph::new(self.render_rod_panel(anim_tick)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" rod ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(panel, chunks[0]);
        }

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

        // bottom-right info pane: rod, fishing level, fish info
        let rod_name = crate::rod::get(self.rod_tier)
            .map(|r| r.name.as_str())
            .unwrap_or("?");
        let stars = "*".repeat(self.fish.difficulty as usize);
        let info_lines = vec![
            Line::from(vec![
                Span::styled(
                    "  rod: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("#{} {}", self.rod_tier, rod_name),
                    Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  fishing lv: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{}", self.fishing_level),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  fish: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    self.fish.name.clone(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  difficulty: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    stars,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", self.fish.description),
                Style::default().fg(Color::Gray),
            )),
        ];
        let info = Paragraph::new(info_lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" details "));
        frame.render_widget(info, right[2]);
    }

    fn render_rod_panel(&self, anim_tick: u64) -> Vec<Line<'static>> {
        // tension = how aligned the rect is with the fish; full tension makes the rod bend hard.
        let in_rect = self.fish_y >= self.rect_y
            && self.fish_y <= self.rect_y + self.rect_h;
        let tension = if in_rect { 1.0_f32 } else { 0.0_f32 };
        // small shimmer on the line based on tick so it never sits still
        let wobble = ((anim_tick / 2) % 3) as i32 - 1;

        // 16-row ascii panel. player at top-left, rod curves down-right, line drops, fish thrashes.
        // tension 0 = rod nearly straight, tension 1 = rod severely bent (sucked toward the fish).
        let rows = 16usize;
        let cols = 20usize;
        let mut grid: Vec<Vec<(char, Color)>> =
            vec![vec![(' ', Color::Reset); cols]; rows];

        // player
        grid[1][1] = ('@', Color::White);
        grid[2][2] = ('|', Color::Rgb(140, 90, 50));

        // rod curve: parametric, more curve with tension
        let rod_len = 10;
        for i in 0..rod_len {
            let t = i as f32 / (rod_len - 1) as f32;
            let x = 2.0 + t * 9.0;
            // tension pulls the tip down so the rod bends
            let y = 2.0 + t * 1.0 + tension * (t * t) * 4.5;
            let r = y.round() as usize;
            let c = x.round() as usize;
            if r < rows && c < cols {
                let glyph = if i == 0 {
                    '|'
                } else if i < rod_len - 2 {
                    if tension > 0.5 { '\\' } else { '-' }
                } else {
                    '*'
                };
                let color = if i == rod_len - 1 {
                    Color::Yellow
                } else {
                    Color::Rgb(190, 140, 80)
                };
                grid[r][c] = (glyph, color);
            }
        }

        // line: from rod tip down to fish
        let tip_x = (2.0_f32 + 9.0_f32).round() as usize;
        let tip_y = (2.0_f32 + 1.0_f32 + tension * 4.5_f32).round() as usize;
        let fish_y_row = (rows - 2) as i32;
        let fish_x_col = (tip_x as i32 + wobble).max(2).min(cols as i32 - 3) as usize;
        let line_top = (tip_y + 1) as i32;
        for ly in line_top..fish_y_row {
            let r = ly as usize;
            if r >= rows {
                break;
            }
            let lx = fish_x_col as i32 + ((ly as i32 - line_top) % 3 - 1) / 2;
            let c = lx.max(0).min(cols as i32 - 1) as usize;
            grid[r][c] = ('|', Color::Gray);
        }

        // fish thrash
        let fish_color = if in_rect {
            Color::LightGreen
        } else {
            Color::LightRed
        };
        let thrash = ((anim_tick / 3) % 4) as usize;
        let fish_glyph = ['<', 'o', '>', 'o'][thrash];
        let fy = fish_y_row as usize;
        if fy < rows {
            grid[fy][fish_x_col] = (fish_glyph, fish_color);
            if fish_x_col > 0 {
                grid[fy][fish_x_col - 1] = (
                    if thrash % 2 == 0 { '~' } else { '=' },
                    Color::Cyan,
                );
            }
            if fish_x_col + 1 < cols {
                grid[fy][fish_x_col + 1] = (
                    if thrash % 2 == 0 { '=' } else { '~' },
                    Color::Cyan,
                );
            }
        }

        // water ripple line at the bottom
        if rows > 0 {
            for x in 0..cols {
                let phase = (x as u64 + anim_tick / 4) % 6;
                let ch = match phase {
                    0..=1 => '~',
                    2 => '-',
                    _ => '.',
                };
                grid[rows - 1][x] = (ch, Color::Rgb(60, 100, 150));
            }
        }

        grid.into_iter()
            .map(|row| {
                let spans: Vec<Span<'static>> = row
                    .into_iter()
                    .map(|(g, c)| {
                        Span::styled(
                            g.to_string(),
                            Style::default().fg(c).add_modifier(Modifier::BOLD),
                        )
                    })
                    .collect();
                Line::from(spans)
            })
            .collect()
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
