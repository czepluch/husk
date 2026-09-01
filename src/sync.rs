//! Runs the sync command in a background thread: after every write,
//! debounced so a burst of edits becomes one run, and on demand from `s`.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// A request has been made that no run has picked up yet.
    pending: Arc<AtomicBool>,
    enabled: bool,
}

impl Syncer {
    /// An empty command disables syncing; requests are then ignored.
    pub fn new(command: Vec<String>) -> Self {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(SyncState::default()));
        let pending = Arc::new(AtomicBool::new(false));
        let enabled = !command.is_empty();
        if enabled {
            let worker_state = Arc::clone(&state);
            let worker_pending = Arc::clone(&pending);
            thread::spawn(move || worker(&rx, &command, &worker_state, &worker_pending));
        }
        Self {
            tx,
            state,
            pending,
            enabled,
        }
    }

    pub fn request(&self) {
        if self.enabled {
            self.pending.store(true, Ordering::SeqCst);
            let _ = self.tx.send(());
        }
    }

    pub fn state(&self) -> SyncState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Whether a run is waiting to start or in progress.
    pub fn busy(&self) -> bool {
        self.pending.load(Ordering::SeqCst) || self.state().running
    }

    /// Waits for pending and running work to finish, so a write made just
    /// before quitting still reaches the server. Returns false on timeout.
    pub fn flush(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while self.busy() {
            if start.elapsed() > timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
        true
    }
}

fn worker(rx: &Receiver<()>, command: &[String], state: &Mutex<SyncState>, pending: &AtomicBool) {
    while rx.recv().is_ok() {
        thread::sleep(DEBOUNCE);
        while rx.try_recv().is_ok() {}
        let started = Instant::now();
        if let Ok(mut s) = state.lock() {
            s.running = true;
        }
        pending.store(false, Ordering::SeqCst);
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

/// `discover` and `metasync` with the sync command's program (a list made
/// on a phone needs both before `sync` sees it), then the sync command,
/// all with the terminal attached so discover can ask about creating
/// collections.
pub fn run_discover(command: &[String]) -> Result<(), String> {
    let (program, args) = command.split_first().ok_or("no sync command")?;
    for verb in ["discover", "metasync"] {
        let status = Command::new(program)
            .arg(verb)
            .status()
            .map_err(|e| format!("{program} {verb}: {e}"))?;
        if !status.success() {
            let hint = if Path::new(program)
                .file_name()
                .is_some_and(|n| n == "vdirsyncer")
            {
                ""
            } else {
                "; --discover runs vdirsyncer's discover and metasync verbs"
            };
            return Err(format!("{program} {verb} failed with {status}{hint}"));
        }
    }
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed with {status}"))
    }
}

/// Runs the sync command once, in the foreground.
pub fn run(command: &[String]) -> Result<(), String> {
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
