use anyhow::Result;
use crossterm::{
    event::{
        self, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
};
use std::io::stdout;
use std::time::{Duration, Instant};

mod app;
mod fish;
mod fishdex;
mod fishing;
mod fishlist;
mod narrator;
mod item;
mod notes;
mod npc;
mod player;
mod quest;
mod rod;
mod save;
mod stats;
mod world;

use app::App;

const TICK_RATE: Duration = Duration::from_millis(50);

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let enhanced = execute!(
        stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    )
    .is_ok();
    let result = run(&mut terminal);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
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
