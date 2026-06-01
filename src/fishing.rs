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
    // ---- skill tree derived effects ----
    /// Quickcatch T1: catch-progress multiplier
    qc_speed_mult: f32,
    /// Quickcatch T2: bonus multiplier when cast_strength was near-max
    qc_perfect_mult: f32,
    /// Rod of Legends T1: 0..1.0 robotic factor (1 = perfectly robotic)
    rl_inertia_reduce: f32,
    /// Rod of Legends T1: coyote frames before gravity resumes
    rl_coyote_frames: u32,
    /// Rod of Legends T2: max boost duration (frames). 0 = ability locked.
    rl_boost_max_frames: u32,
    rl_boost_available: bool,
    rl_boost_left: u32,
    /// active rect_h bonus added when boost is on
    rl_rect_h_bonus: f32,
    coyote_left: u32,
    /// Tamer T2: slow fraction (0..0.4). 0 = ability locked.
    tm_slow_strength: f32,
    tm_slow_available: bool,
    tm_slow_left: u32,
    // ---- T3 effects ----
    /// Quickcatch T3 multiplier (1.0 = neutral)
    qc_effortless_mult: f32,
    /// Rod of Legends T3 phantom pull strength
    rl_phantom_pull: f32,
    /// Tamer T3 grace frames remaining (fish stays put)
    tm_grace_left: u32,
    /// Display-only summary lines for the equipped bait + tackle. The
    /// caller fills these in before transitioning to the scene; the
    /// minigame just renders them in a corner panel.
    pub gear_bait_label: String,
    pub gear_tackle_label: String,
}

impl Fishing {
    /// Construct a Fishing scene with skill-tree effects + cast strength
    /// (used by Quickcatch T2's "perfect throw" bonus).
    /// `extra_speed_pct` is an additive catch-speed bonus from sources
    /// outside the skill tree — weather, dim presence, etc. 0.0 = none.
    pub fn new_with_skills(
        fish: &'static FishDef,
        rng_seed: u32,
        fishing_level: u32,
        rod_tier: u32,
        cast_strength: f32,
        tree: &crate::skill_tree::SkillTree,
        extra_speed_pct: f32,
    ) -> Self {
        let bar_h = 20usize;
        let rect_h = (fish.rect_h() + tree.rect_h_bonus()).min(bar_h as f32 - 1.0);
        // Player rectangle starts at the very bottom of the bar — the
        // player has to actively reel up to track whatever the fish
        // does. Catch progress starts at 0% so no fish is a freebie.
        let rect_y = bar_h as f32 - rect_h;
        let mid = bar_h as f32 / 2.0;
        let rod_mult = 0.99f32.powi(rod_tier as i32);
        let calm = tree.tamer_calm_mult();
        let mut s = Self {
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
            target_change_ticks: ((fish.target_change_ticks() as f32) * calm) as u32,
            progress: 0.0,
            finished: None,
            up_held: false,
            down_held: false,
            up_held_until: 0,
            down_held_until: 0,
            rng_state: if rng_seed == 0 { 0x9E37_79B9 } else { rng_seed },
            tick_count: 0,
            qc_speed_mult: tree.fishing_speed_mult() * (1.0 + extra_speed_pct),
            qc_perfect_mult: if cast_strength >= 0.9 {
                tree.perfect_throw_mult()
            } else {
                1.0
            },
            rl_inertia_reduce: tree.inertia_reduce(),
            rl_coyote_frames: tree.coyote_frames(),
            rl_boost_max_frames: tree.legends_boost_frames(),
            rl_boost_available: tree.legends_boost_frames() > 0,
            rl_boost_left: 0,
            rl_rect_h_bonus: 0.0,
            coyote_left: 0,
            tm_slow_strength: tree.tamer_slow_strength(),
            tm_slow_available: tree.tamer_slow_strength() > 0.0,
            tm_slow_left: 0,
            qc_effortless_mult: tree.effortless_mult(),
            rl_phantom_pull: tree.phantom_pull(),
            tm_grace_left: tree.telepathic_grace_frames(),
            gear_bait_label: String::new(),
            gear_tackle_label: String::new(),
        };
        if s.target_change_ticks == 0 {
            s.target_change_ticks = 1;
        }
        s
    }

    /// Active rectangle boost — Rod of Legends T2. Adds +1 to the rectangle
    /// height for the configured duration. One use per fishing scene.
    pub fn input_legends_boost(&mut self) {
        if !self.rl_boost_available || self.rl_boost_max_frames == 0 {
            return;
        }
        self.rl_boost_available = false;
        self.rl_boost_left = self.rl_boost_max_frames;
        self.rl_rect_h_bonus = 1.0;
    }

    /// Active fish slow — Tamer T2. Reduces fish_speed for 5 seconds (100
    /// frames at 20fps). One use per fishing scene.
    pub fn input_tamer_slow(&mut self) {
        if !self.tm_slow_available || self.tm_slow_strength <= 0.0 {
            return;
        }
        self.tm_slow_available = false;
        self.tm_slow_left = 100;
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

    /// Hard downward yank. Baseline strength is 0.30 (weak); the Rod of
    /// Legends T1 skill tree ramps it toward 1.20 (= same as yank up) at
    /// max rank. Caller passes the computed strength.
    pub fn input_yank_down(&mut self, kind: KeyEventKind, strength: f32) {
        if matches!(kind, KeyEventKind::Press) {
            self.rect_vy += strength;
            self.down_held = true;
            self.down_held_until = self.tick_count + 6;
        }
    }

    /// Hard upward yank — mirror of yank_down for symmetry.
    pub fn input_yank_up(&mut self, kind: KeyEventKind) {
        if matches!(kind, KeyEventKind::Press) {
            self.rect_vy -= 1.20;
            self.up_held = true;
            self.up_held_until = self.tick_count + 6;
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
        // Rod of Legends T2 boost: rectangle height bonus tapers off
        if self.rl_boost_left > 0 {
            self.rl_boost_left -= 1;
            if self.rl_boost_left == 0 {
                self.rl_rect_h_bonus = 0.0;
            }
        }
        // Tamer T2 slow: tick down
        if self.tm_slow_left > 0 {
            self.tm_slow_left -= 1;
        }
        if self.up_held {
            self.rect_vy -= 0.45;
            self.coyote_left = self.rl_coyote_frames;
        }
        if self.down_held {
            self.rect_vy += 0.45;
            self.coyote_left = self.rl_coyote_frames;
        }
        // Phantom Rod (RoL T3): when neither key is held, the rectangle is
        // pulled toward the fish position.
        if !self.up_held && !self.down_held && self.rl_phantom_pull > 0.0 {
            let rect_center = self.rect_y + (self.rect_h + self.rl_rect_h_bonus) * 0.5;
            let pull_dy = self.fish_y - rect_center;
            self.rect_vy += pull_dy.signum() * self.rl_phantom_pull * pull_dy.abs().min(3.0);
        }
        // Rod of Legends T1: blend gravity/damping toward "robotic" as
        // inertia_reduce → 1.0. Maxed-out → near-zero gravity AND high
        // damping, with coyote_frames preventing immediate drop after release.
        let robotic = self.rl_inertia_reduce.clamp(0.0, 1.0);
        let base_gravity = 0.22;
        let base_damping = 0.85;
        let gravity = base_gravity * (1.0 - robotic * 0.92);
        let damping = base_damping * (1.0 - robotic * 0.50) + robotic * 0.10;
        // coyote hover: while coyote_left > 0 and no input, suppress gravity
        if self.coyote_left > 0 && !self.up_held && !self.down_held {
            self.coyote_left -= 1;
        } else {
            self.rect_vy += gravity;
        }
        self.rect_vy *= damping;
        self.rect_y += self.rect_vy;
        let effective_rect_h = self.rect_h + self.rl_rect_h_bonus;
        let max_top = (self.bar_h as f32) - effective_rect_h;
        if self.rect_y < 0.0 {
            self.rect_y = 0.0;
            self.rect_vy = 0.0;
        }
        if self.rect_y > max_top {
            self.rect_y = max_top;
            self.rect_vy = 0.0;
        }
        // Telepathic Lure (Tamer T3): fish stays put for grace frames.
        if self.tm_grace_left > 0 {
            self.tm_grace_left -= 1;
        } else if self.tick_count % self.target_change_ticks == 0 {
            self.fish_target_y = self.next_target();
        }
        let effective_fish_speed = if self.tm_slow_left > 0 {
            self.fish_speed * (1.0 - self.tm_slow_strength)
        } else {
            self.fish_speed
        };
        let dy = self.fish_target_y - self.fish_y;
        if dy.abs() > effective_fish_speed {
            self.fish_y += effective_fish_speed * dy.signum();
        } else {
            self.fish_y = self.fish_target_y;
        }
        let in_rect = self.fish_y >= self.rect_y && self.fish_y <= self.rect_y + effective_rect_h;
        let fishing_mult = 1.0 + (self.fishing_level as f32) * 0.0025;
        let speed_mult = self.qc_speed_mult * self.qc_perfect_mult * self.qc_effortless_mult;
        if in_rect {
            self.progress += 0.8 * fishing_mult * speed_mult;
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
        let caught = matches!(self.finished, Some(FishingResult::Caught));
        let title = if caught {
            let stars = "*".repeat(self.fish.difficulty as usize);
            format!(" fishing - {} {} ", self.fish.name, stars)
        } else {
            " fishing - ??? ".to_string()
        };
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
                "it slipped away. you'll never know what.".into(),
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
        let caught = matches!(self.finished, Some(FishingResult::Caught));
        let (fish_name, fish_stars, fish_desc) = if caught {
            (
                self.fish.name.clone(),
                "*".repeat(self.fish.difficulty as usize),
                self.fish.description.clone(),
            )
        } else {
            ("???".to_string(), "?".to_string(), String::new())
        };
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
                    fish_name,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  difficulty: ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    fish_stars,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  bait: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    self.gear_bait_label.clone(),
                    Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  tackle: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    self.gear_tackle_label.clone(),
                    Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", fish_desc),
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

