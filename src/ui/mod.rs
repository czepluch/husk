//! The terminal loop: draw, wait for a key, hand it to `App`.

pub mod app;
pub mod views;

use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::theme::Theme;
use app::App;

pub fn run(mut app: App, theme: &Theme) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, theme);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, theme: &Theme) -> Result<()> {
    while !app.quit {
        terminal.draw(|frame| views::draw(frame, app, theme))?;
        if event::poll(Duration::from_millis(500))?
            && let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
        {
            app.handle_key(key);
        }
        app.now = Local::now();
    }
    Ok(())
}
