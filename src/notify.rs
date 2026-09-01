//! `husk notify`: fires a desktop notification for every alarm that came
//! due since the last run, once each, and remembers what it fired in a
//! small state file. Idempotent by construction: a second run right after
//! the first fires nothing, and runs missed while the laptop slept fire
//! everything that came due in between, once.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeDelta, Utc};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::alarms::{fire_times, is_overdue};
use crate::config::Config;
use crate::model::{Due, Project, Task, project_name};

/// Fired entries older than this are forgotten.
const KEEP_DAYS: i64 = 30;
/// How far back a run looks even if the previous run was more recent, so an
/// alarm that reaches the vdir after its time (a reminder set on the phone
/// minutes ahead, synced by the timer) still fires once.
const GRACE_MINUTES: i64 = 15;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct State {
    pub last_run: Option<DateTime<Utc>>,
    #[serde(default)]
    pub fired: Vec<Fired>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Fired {
    pub uid: String,
    pub at: DateTime<Utc>,
}

impl State {
    /// `~/.local/state/husk/notify.json`.
    pub fn default_path() -> Option<PathBuf> {
        BaseDirs::new()
            .and_then(|base| base.state_dir().map(Path::to_path_buf))
            .map(|dir| dir.join("husk").join("notify.json"))
    }

    /// A missing file is an empty state. A file that does not parse is
    /// moved aside and treated as empty too, so one bad write cannot keep
    /// every later run from working; at most one minute of alarms is lost.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        match serde_json::from_str(&text) {
            Ok(state) => Ok(state),
            Err(e) => {
                let aside = path.with_extension("json.corrupt");
                fs::rename(path, &aside)
                    .with_context(|| format!("move {} aside", path.display()))?;
                eprintln!(
                    "husk: {} did not parse ({e}); moved to {} and starting fresh",
                    path.display(),
                    aside.display()
                );
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))
    }
}

/// One notification to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub uid: String,
    pub at: DateTime<Utc>,
    pub title: String,
    pub body: String,
    pub urgent: bool,
    /// Nag notices are not remembered; they repeat on every `--nag` run.
    pub remember: bool,
}

/// What to fire now: every alarm of a pending task that came due after the
/// last run (or within the grace window, whichever is earlier) and up to
/// now, unless already fired. With `nag`, every overdue pending task as
/// well. Returns the notices and the state to save.
pub fn plan(
    tasks: &[Task],
    projects: &[Project],
    state: &State,
    now: DateTime<Utc>,
    config: &Config,
    nag: bool,
) -> (Vec<Notice>, State) {
    let since = window_start(state, now);
    let cutoff = now - TimeDelta::days(KEEP_DAYS);
    let mut fired: Vec<Fired> = state
        .fired
        .iter()
        .filter(|f| f.at > cutoff)
        .cloned()
        .collect();
    let mut notices = Vec::new();
    for task in tasks.iter().filter(|t| !t.is_done()) {
        let project = project_name(projects, &task.project);
        for at in fire_times(task, &config.default_alarm_leads) {
            let key = Fired {
                uid: task.uid.clone(),
                at,
            };
            if at <= since || at > now || fired.contains(&key) {
                continue;
            }
            notices.push(Notice {
                uid: task.uid.clone(),
                at,
                title: task.summary.clone(),
                body: format!("{} · {project}", due_text(task.due, config)),
                // Leads before the due time are gentle; at or after it, not.
                urgent: at_or_past_due(task.due, at),
                remember: true,
            });
            fired.push(key);
        }
        if nag && is_overdue(task.due, now) {
            notices.push(Notice {
                uid: task.uid.clone(),
                at: now,
                title: format!("Overdue: {}", task.summary),
                body: format!("{} · {project}", due_text(task.due, config)),
                urgent: true,
                remember: false,
            });
        }
    }
    notices.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.title.cmp(&b.title)));
    let state = State {
        last_run: Some(now),
        fired,
    };
    (notices, state)
}

/// The last run or the start of the grace window, whichever is earlier;
/// on the very first run, just the grace window.
pub fn window_start(state: &State, now: DateTime<Utc>) -> DateTime<Utc> {
    let grace = now - TimeDelta::minutes(GRACE_MINUTES);
    state.last_run.map_or(grace, |last| last.min(grace))
}

fn at_or_past_due(due: Option<Due>, at: DateTime<Utc>) -> bool {
    match due {
        None => false,
        Some(Due::DateTime(when)) => at >= when,
        Some(Due::Date(date)) => date <= at.with_timezone(&Local).date_naive(),
    }
}

fn due_text(due: Option<Due>, config: &Config) -> String {
    match due {
        None => "No due date".to_string(),
        Some(Due::Date(date)) => format!("Due {}", date.format(&config.date_format)),
        Some(Due::DateTime(at)) => format!(
            "Due {}",
            at.with_timezone(&Local)
                .format(&format!("{} {}", config.date_format, config.time_format))
        ),
    }
}

/// Shows one notice through the desktop notification daemon. The body is
/// escaped because daemons like mako read it as Pango markup.
pub fn send(notice: &Notice) -> Result<()> {
    use notify_rust::{Notification, Urgency};
    let body = notice
        .body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Notification::new()
        .appname("husk")
        .summary(&notice.title)
        .body(&body)
        .urgency(if notice.urgent {
            Urgency::Critical
        } else {
            Urgency::Normal
        })
        .show()
        .with_context(|| format!("notify {:?}", notice.title))?;
    Ok(())
}

/// Plans, sends and saves. Returns how many were shown.
/// A dry run prints what would fire and changes nothing.
pub fn run(
    tasks: &[Task],
    projects: &[Project],
    path: &Path,
    now: DateTime<Utc>,
    config: &Config,
    nag: bool,
    dry_run: bool,
) -> Result<usize> {
    run_with(tasks, projects, path, now, config, nag, dry_run, send)
}

/// `run` with the sender injected. When a remembered notice cannot be
/// delivered it is not marked fired and `last_run` stays where it was, so
/// the next run tries again; the fired set keeps the delivered ones from
/// repeating. Two runs at once are serialized by a lock next to the file.
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    tasks: &[Task],
    projects: &[Project],
    path: &Path,
    now: DateTime<Utc>,
    config: &Config,
    nag: bool,
    dry_run: bool,
    mut sender: impl FnMut(&Notice) -> Result<()>,
) -> Result<usize> {
    let _lock = if dry_run { None } else { Some(lock(path)?) };
    let state = State::load(path)?;
    let (notices, mut next) = plan(tasks, projects, &state, now, config, nag);
    if dry_run {
        let format = |at: DateTime<Utc>| at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
        println!(
            "Alarms after {} up to {} (last run: {})",
            format(window_start(&state, now)),
            format(now),
            state
                .last_run
                .map_or("never".to_string(), |at| format(at).to_string())
        );
        for notice in &notices {
            println!("{}  {}  {}", format(notice.at), notice.title, notice.body);
        }
        if notices.is_empty() {
            println!("nothing to notify");
        }
        return Ok(notices.len());
    }
    let mut sent = 0;
    let mut failures = Vec::new();
    for notice in &notices {
        match sender(notice) {
            Ok(()) => sent += 1,
            Err(e) => {
                if notice.remember {
                    next.fired
                        .retain(|f| !(f.uid == notice.uid && f.at == notice.at));
                    next.last_run = state.last_run;
                }
                failures.push(format!("{e:#}"));
            }
        }
    }
    next.save(path)?;
    if let Some(first) = failures.first() {
        anyhow::bail!(
            "{} of {} notifications failed: {first}",
            failures.len(),
            notices.len()
        );
    }
    Ok(sent)
}

fn lock(path: &Path) -> Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let lock_path = path.with_extension("lock");
    let file =
        File::create(&lock_path).with_context(|| format!("create {}", lock_path.display()))?;
    file.lock()
        .with_context(|| format!("lock {}", lock_path.display()))?;
    Ok(file)
}
