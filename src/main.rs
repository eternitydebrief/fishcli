use anyhow::Result;
use crossterm::event::{self, Event};
use std::time::Duration;

mod app;
mod fishing;
mod map;
mod player;

use app::App;

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let mut app = App::new();
    while app.running {
        terminal.draw(|frame| app.render(frame))?;
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }
        app.tick();
    }
    Ok(())
}
