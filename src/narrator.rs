use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
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
        // newest messages last; pad with blanks at the top so newest sits at bottom.
        // styled as a brightness gradient: newest brightest, older fade out.
        let n = self.messages.len();
        let start = n.saturating_sub(max_lines);
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(max_lines);
        let visible: Vec<&String> = self.messages.iter().skip(start).collect();
        for _ in visible.len()..max_lines {
            lines.push(Line::from(""));
        }
        let last = visible.len().saturating_sub(1);
        for (i, m) in visible.iter().enumerate() {
            let age = last - i;
            let gray = match age {
                0 => 0xb0,
                1 => 0x90,
                2 => 0x70,
                3 => 0x50,
                4 => 0x40,
                _ => 0x20,
            };
            let style = Style::default().fg(Color::Rgb(gray, gray, gray));
            lines.push(Line::from(Span::styled(format!("> {m}"), style)));
        }
        // no wrap - one message per row, so newest never gets pushed off-screen
        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }
}
