use anyhow::Result;
use crossterm::event::{self, Event};
use std::time::{Duration, Instant};

mod app;
mod fishing;
mod map;
mod player;

use app::App;

const TICK_RATE: Duration = Duration::from_millis(33);

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let mut app = App::new();
    let mut last_tick = Instant::now();
    while app.running {
        terminal.draw(|frame| app.render(frame))?;
        let timeout = TICK_RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }
        if last_tick.elapsed() >= TICK_RATE {
            app.tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}
