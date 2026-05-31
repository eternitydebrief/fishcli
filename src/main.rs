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
mod buffs;
mod fish;
mod gametime;
mod weather;
mod fishdex;
mod fishing;
mod fishlist;
mod narrator;
mod item;
mod mining;
mod notes;
mod npc;
mod player;
mod quest;
mod rod;
mod save;
mod skill_tree;
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
    let mut dirty = true; // render at least once on startup
    while app.running {
        // Render only when something changed or the tick fired. Without
        // this we drew every event-loop iteration — i.e. once per key
        // event PLUS once per tick, so holding a movement key drove the
        // CPU much harder than 20fps required.
        if dirty {
            terminal.draw(|frame| app.render(frame))?;
            dirty = false;
        }

        let timeout = TICK_RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
                dirty = true;
            }
        }
        if last_tick.elapsed() >= TICK_RATE {
            app.tick();
            last_tick = Instant::now();
            dirty = true;
        }
    }
    Ok(())
}
