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
}

impl Fishdex {
    pub fn new() -> Self {
        let mut s = ListState::default();
        s.select(Some(0));
        Self { state: s }
    }

    pub fn cursor_down(&mut self) {
        let len = fish().len();
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some((i + 1).min(len - 1)));
    }

    pub fn cursor_up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some(i.saturating_sub(1)));
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        caught: &[bool],
        caught_at: &[Option<(String, String)>],
        caught_context: &[Option<(String, String, String)>],
    ) {
        let area = frame.area();
        let total = fish().len();
        let caught_count = caught.iter().filter(|c| **c).count();
        let title = format!(" fishdex ({}/{}) - j/k or arrows to browse, esc/q to leave ", caught_count, total);
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

        let items: Vec<ListItem<'static>> = fish()
            .iter()
            .enumerate()
            .map(|(i, f)| {
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

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" species "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, chunks[0], &mut self.state);

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
