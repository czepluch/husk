//! Runs the sync command in a background thread: after every write,
//! debounced so a burst of edits becomes one run, and on demand from `s`.

use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};

/// Requests arriving within this window are merged into one run.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// At most one run per this long.
const MIN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncState {
    pub running: bool,
    /// Completed runs, successful or not; the UI reloads when this changes.
    pub runs: u64,
    pub last_ok: Option<DateTime<Local>>,
    pub last_error: Option<String>,
}

pub struct Syncer {
    tx: Sender<()>,
    state: Arc<Mutex<SyncState>>,
}

impl Syncer {
    /// An empty command disables syncing; requests are then ignored.
    pub fn new(command: Vec<String>) -> Self {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(SyncState::default()));
        if !command.is_empty() {
            let worker_state = Arc::clone(&state);
            thread::spawn(move || worker(&rx, &command, &worker_state));
        }
        Self { tx, state }
    }

    pub fn request(&self) {
        let _ = self.tx.send(());
    }

    pub fn state(&self) -> SyncState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn worker(rx: &Receiver<()>, command: &[String], state: &Mutex<SyncState>) {
    while rx.recv().is_ok() {
        thread::sleep(DEBOUNCE);
        while rx.try_recv().is_ok() {}
        let started = Instant::now();
        if let Ok(mut s) = state.lock() {
            s.running = true;
        }
        let outcome = run(command);
        if let Ok(mut s) = state.lock() {
            s.running = false;
            s.runs += 1;
            match outcome {
                Ok(()) => {
                    s.last_ok = Some(Local::now());
                    s.last_error = None;
                }
                Err(message) => s.last_error = Some(message),
            }
        }
        if let Some(rest) = MIN_INTERVAL.checked_sub(started.elapsed()) {
            thread::sleep(rest);
        }
    }
}

fn run(command: &[String]) -> Result<(), String> {
    let (program, args) = command.split_first().ok_or("no sync command")?;
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_string();
    Err(format!("{program} failed: {reason}"))
}
