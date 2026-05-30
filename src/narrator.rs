use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::collections::VecDeque;

const MAX: usize = 200;

pub struct Narrator {
    messages: VecDeque<String>,
}

impl Narrator {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::with_capacity(MAX),
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        if self.messages.len() == MAX {
            self.messages.pop_front();
        }
        self.messages.push_back(msg.into());
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" log ")
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 {
            return;
        }

        let max_lines = inner.height as usize;
        let recent: Vec<&String> = self.messages.iter().rev().take(max_lines).collect();
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_lines);
        for _ in recent.len()..max_lines {
            lines.push(Line::from(""));
        }
        for (i, m) in recent.iter().rev().enumerate() {
            let age = recent.len() - 1 - i;
            let style = match age {
                0 => Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
                1 | 2 => Style::default().fg(Color::Gray),
                _ => Style::default().fg(Color::DarkGray),
            };
            lines.push(Line::from(Span::styled(format!("> {m}"), style)));
        }

        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }
}
