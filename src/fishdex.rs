use crate::fishlist::fish;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub struct Fishdex {
    pub state: ListState,
    pub filter: String,
    /// True while typing into the filter box (/-prompt mode).
    pub editing_filter: bool,
}

impl Fishdex {
    pub fn new() -> Self {
        let mut s = ListState::default();
        s.select(Some(0));
        Self { state: s, filter: String::new(), editing_filter: false }
    }

    /// Indices into the global fish list that match the current filter.
    /// Empty filter = every entry, in catalog order.
    pub fn visible(&self, caught: &[bool]) -> Vec<usize> {
        let q = self.filter.to_ascii_lowercase();
        fish()
            .iter()
            .enumerate()
            .filter(|(i, f)| {
                if q.is_empty() {
                    return true;
                }
                let known = caught.get(*i).copied().unwrap_or(false);
                if !known {
                    return false;
                }
                if f.name.to_ascii_lowercase().contains(&q) {
                    return true;
                }
                if f.biomes.iter().any(|b| b.to_ascii_lowercase().contains(&q)) {
                    return true;
                }
                if f.waters.iter().any(|w| w.to_ascii_lowercase().contains(&q)) {
                    return true;
                }
                if f.pool.iter().any(|p| p.to_ascii_lowercase().contains(&q)) {
                    return true;
                }
                false
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn cursor_down(&mut self, caught: &[bool]) {
        let vis = self.visible(caught);
        if vis.is_empty() {
            return;
        }
        let cur = self.state.selected().unwrap_or(0);
        let pos = vis.iter().position(|&i| i == cur).unwrap_or(0);
        let next = (pos + 1).min(vis.len() - 1);
        self.state.select(Some(vis[next]));
    }

    pub fn cursor_up(&mut self, caught: &[bool]) {
        let vis = self.visible(caught);
        if vis.is_empty() {
            return;
        }
        let cur = self.state.selected().unwrap_or(0);
        let pos = vis.iter().position(|&i| i == cur).unwrap_or(0);
        let prev = pos.saturating_sub(1);
        self.state.select(Some(vis[prev]));
    }

    pub fn start_filter(&mut self) {
        self.editing_filter = true;
    }

    pub fn push_filter(&mut self, c: char) {
        if self.filter.chars().count() < 40 {
            self.filter.push(c);
        }
    }

    pub fn pop_filter(&mut self) {
        self.filter.pop();
    }

    /// Commit the current filter; cursor snaps to first visible match.
    pub fn apply_filter(&mut self, caught: &[bool]) {
        self.editing_filter = false;
        if let Some(&first) = self.visible(caught).first() {
            self.state.select(Some(first));
        }
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.editing_filter = false;
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        caught: &[bool],
        caught_at: &[Option<(String, String)>],
        caught_context: &[Option<(String, String, String)>],
        milestone_blurb: &str,
    ) {
        let area = frame.area();
        let total = fish().len();
        let caught_count = caught.iter().filter(|c| **c).count();
        let visible_idx = self.visible(caught);
        let title = if self.editing_filter {
            format!(" fishdex / {}_  (Enter apply, Esc clear) ", self.filter)
        } else if !self.filter.is_empty() {
            format!(
                " fishdex ({}/{}) filter: {} ({} match) ",
                caught_count,
                total,
                self.filter,
                visible_idx.len()
            )
        } else {
            format!(
                " fishdex ({}/{}){milestone_blurb} - j/k browse, / filter, esc/q leave ",
                caught_count, total
            )
        };
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let list_w = inner.width.max(20) / 2;
        let list_w = list_w.clamp(16, 36);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(list_w), Constraint::Min(8)])
            .split(inner);

        let items: Vec<ListItem<'static>> = visible_idx
            .iter()
            .map(|&i| {
                let f = &fish()[i];
                let known = caught.get(i).copied().unwrap_or(false);
                if known {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            f.name.to_string(),
                            Style::default().fg(Color::LightYellow),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            "*".repeat(f.difficulty as usize),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]))
                } else {
                    ListItem::new(Line::from(Span::styled(
                        "???".to_string(),
                        Style::default().fg(Color::DarkGray),
                    )))
                }
            })
            .collect();

        // The list widget needs a local cursor counting from 0 against the
        // filtered indices, not the global fish index that self.state holds.
        let global_sel = self.state.selected().unwrap_or(0);
        let local_pos = visible_idx.iter().position(|&i| i == global_sel);
        let mut local_state = ListState::default();
        local_state.select(local_pos);
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" species "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, chunks[0], &mut local_state);

        let sel = self.state.selected().unwrap_or(0);
        let detail_lines: Vec<Line> = if let Some(f) = fish().get(sel) {
            let known = caught.get(sel).copied().unwrap_or(false);
            if known {
                let mut lines = vec![
                    Line::from(Span::styled(
                        f.name.to_string(),
                        Style::default()
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("difficulty: "),
                        Span::styled(
                            "*".repeat(f.difficulty as usize),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]),
                    Line::from(""),
                ];
                if let Some(Some((biome, water))) = caught_at.get(sel) {
                    lines.push(Line::from(vec![
                        Span::raw("location:  "),
                        Span::styled(
                            format!("{biome} / {water}"),
                            Style::default().fg(Color::LightGreen),
                        ),
                    ]));
                }
                if let Some(Some((tod, w, season))) = caught_context.get(sel) {
                    lines.push(Line::from(vec![
                        Span::raw("time:      "),
                        Span::styled(
                            tod.clone(),
                            Style::default().fg(Color::LightCyan),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::raw("weather:   "),
                        Span::styled(
                            w.clone(),
                            Style::default().fg(Color::LightBlue),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::raw("season:    "),
                        Span::styled(
                            season.clone(),
                            Style::default().fg(Color::LightYellow),
                        ),
                    ]));
                }
                // declared preferences from JSON
                if !f.biomes.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("prefers:   "),
                        Span::styled(
                            f.biomes.join(", "),
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                }
                if !f.waters.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("water:     "),
                        Span::styled(
                            f.waters.join(", "),
                            Style::default().fg(Color::Blue),
                        ),
                    ]));
                }
                if !f.pool.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("pool:      "),
                        Span::styled(
                            f.pool.join(", "),
                            Style::default().fg(Color::Magenta),
                        ),
                    ]));
                }
                // Sale price intentionally hidden — sell at the fishmonger
                // to learn what something is worth.
                if let Some(eff) = &f.effect {
                    lines.push(Line::from(vec![
                        Span::raw("effect:    "),
                        Span::styled(
                            eff.clone(),
                            Style::default().fg(Color::LightMagenta),
                        ),
                    ]));
                }
                if f.unique {
                    lines.push(Line::from(Span::styled(
                        "UNIQUE - misc tab, cannot be sold or discarded",
                        Style::default().fg(Color::LightYellow),
                    )));
                }
                if f.joke {
                    lines.push(Line::from(Span::styled(
                        "JOKE - not a real fish",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(f.description.as_str()));
                lines
            } else {
                vec![
                    Line::from(Span::styled(
                        "???",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "catch this fish to reveal its entry",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
        } else {
            vec![Line::from("no fish selected")]
        };

        let detail = Paragraph::new(detail_lines)
            .block(Block::default().borders(Borders::ALL).title(" detail "));
        frame.render_widget(detail, chunks[1]);
    }
}
