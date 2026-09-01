//! The terminal loop: draw, wait for a key, hand it to `App`, and step out
//! to `$EDITOR` when the app asks for it.

pub mod app;
pub mod views;

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use notify::Watcher;

use anyhow::{Context, Result, bail};
use chrono::Local;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::theme::Theme;
use app::App;

/// Runs the TUI. `reload` re-reads the theme; it is called whenever a file
/// under `watch` changes, so a theme can be edited while husk runs.
pub fn run(
    mut app: App,
    theme: Theme,
    reload: impl Fn() -> Result<Theme>,
    watch: Option<&Path>,
) -> Result<()> {
    let (tx, changes) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |_: notify::Result<notify::Event>| {
        let _ = tx.send(());
    })
    .ok();
    if let (Some(watcher), Some(dir)) = (watcher.as_mut(), watch.filter(|d| d.is_dir())) {
        let _ = watcher.watch(dir, notify::RecursiveMode::Recursive);
    }
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, theme, &reload, &changes);
    ratatui::restore();
    if app.sync_busy() {
        eprintln!("husk: waiting for the sync to finish");
        if !app.flush_sync(Duration::from_secs(30)) {
            eprintln!("husk: sync still running; it continues in the background");
        }
    }
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mut theme: Theme,
    reload: &impl Fn() -> Result<Theme>,
    changes: &mpsc::Receiver<()>,
) -> Result<()> {
    while !app.quit {
        if changes.try_recv().is_ok() {
            while changes.try_recv().is_ok() {}
            match reload() {
                Ok(fresh) => {
                    theme = fresh;
                    app.message = Some("Theme reloaded".to_string());
                }
                Err(e) => app.message = Some(format!("theme: {e:#}")),
            }
        }
        terminal.draw(|frame| views::draw(frame, app, &theme))?;
        if event::poll(Duration::from_millis(500))?
            && let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
        {
            app.handle_key(key);
        }
        if let Some(request) = app.take_editor_request() {
            ratatui::restore();
            let edited = edit_with_editor(&request.text);
            *terminal = ratatui::init();
            terminal.clear()?;
            match (edited, request.raw) {
                (Ok(text), true) => app.apply_raw(&request.uid, &text),
                (Ok(text), false) => app.apply_notes(&request.uid, &text),
                (Err(e), _) => app.message = Some(format!("{e:#}")),
            }
        }
        app.now = Local::now();
        app.poll();
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
    write_private(&path, text).with_context(|| format!("write {}", path.display()))?;
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

/// Writes a file readable by the owner only; notes are private.
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}
