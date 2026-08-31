//! The terminal loop: draw, wait for a key, hand it to `App`, and step out
//! to `$EDITOR` when the app asks for it.

pub mod app;
pub mod views;

use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
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
        if let Some((uid, text)) = app.take_editor_request() {
            ratatui::restore();
            let edited = edit_with_editor(&text);
            *terminal = ratatui::init();
            terminal.clear()?;
            match edited {
                Ok(new_text) => app.apply_notes(&uid, &new_text),
                Err(e) => app.message = Some(format!("{e:#}")),
            }
        }
        app.now = Local::now();
        app.poll_sync();
    }
    Ok(())
}

/// Opens `$VISUAL` or `$EDITOR` on a temp file holding `text` and returns
/// what was saved. The terminal must be restored before calling this.
pub fn edit_with_editor(text: &str) -> Result<String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|e| !e.trim().is_empty());
    let Some(editor) = editor else {
        bail!("set $EDITOR (or $VISUAL) to edit notes");
    };
    edit_with(&editor, text)
}

/// Runs one editor command on a temp file holding `text`.
pub fn edit_with(editor: &str, text: &str) -> Result<String> {
    let path = std::env::temp_dir().join(format!("husk-notes-{}.md", std::process::id()));
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    // Through the shell so an editor setting with arguments ("code --wait") works.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("husk")
        .arg(&path)
        .status()
        .with_context(|| format!("run {editor}"))?;
    let result = if status.success() {
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
    } else {
        Err(anyhow::anyhow!(
            "{editor} exited with {status}; notes unchanged"
        ))
    };
    let _ = std::fs::remove_file(&path);
    result
}
