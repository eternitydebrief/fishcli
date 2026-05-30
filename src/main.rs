use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use std::time::Duration;

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" fishcli ")
                .border_style(Style::default().fg(Color::Cyan));
            let body = Paragraph::new(vec![
                Line::from(""),
                Line::from("  @"),
                Line::from(""),
                Line::from("  press q to quit"),
            ])
            .block(block);
            frame.render_widget(body, frame.area());
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }
    }
}
