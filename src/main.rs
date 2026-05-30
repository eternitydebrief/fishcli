use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::time::Duration;

mod map;
mod player;

use map::Map;
use player::Player;

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let map = Map::starter();
    let mut player = Player::spawn();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(area);

            let lines = map.render_lines(Some((player.x, player.y)));
            let map_widget = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" fishcli ")
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(map_widget, chunks[0]);

            let help = Paragraph::new("hjkl/arrows: move    q: quit").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(help, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('h') | KeyCode::Left => player.try_move(&map, -1, 0),
                    KeyCode::Char('j') | KeyCode::Down => player.try_move(&map, 0, 1),
                    KeyCode::Char('k') | KeyCode::Up => player.try_move(&map, 0, -1),
                    KeyCode::Char('l') | KeyCode::Right => player.try_move(&map, 1, 0),
                    _ => {}
                }
            }
        }
    }
}
