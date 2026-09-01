//! `husk notify`: fires a desktop notification for every alarm that came
//! due since the last run, once each, and remembers what it fired in a
//! small state file. Idempotent by construction: a second run right after
//! the first fires nothing, and runs missed while the laptop slept fire
//! everything that came due in between, once.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeDelta, Utc};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::alarms::{fire_times, is_overdue};
use crate::config::Config;
use crate::model::{Due, Project, Task};

/// Fired entries older than this are forgotten.
const KEEP_DAYS: i64 = 30;

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

    /// A missing file is an empty state; a broken one is an error.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
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
/// last run and up to now, unless already fired. With `nag`, every overdue
/// pending task as well. Returns the notices and the state to save.
pub fn plan(
    tasks: &[Task],
    projects: &[Project],
    state: &State,
    now: DateTime<Utc>,
    config: &Config,
    nag: bool,
) -> (Vec<Notice>, State) {
    // On the very first run nothing has "come due since", so nothing fires.
    let since = state.last_run.unwrap_or(now);
    let cutoff = now - TimeDelta::days(KEEP_DAYS);
    let mut fired: Vec<Fired> = state
        .fired
        .iter()
        .filter(|f| f.at > cutoff)
        .cloned()
        .collect();
    let mut notices = Vec::new();
    for task in tasks.iter().filter(|t| !t.is_done()) {
        let project = projects
            .iter()
            .find(|p| p.id == task.project)
            .map_or_else(|| task.project.as_str().to_string(), |p| p.name.clone());
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

/// Shows one notice through the desktop notification daemon.
pub fn send(notice: &Notice) -> Result<()> {
    use notify_rust::{Notification, Urgency};
    Notification::new()
        .appname("husk")
        .summary(&notice.title)
        .body(&notice.body)
        .urgency(if notice.urgent {
            Urgency::Critical
        } else {
            Urgency::Normal
        })
        .show()
        .with_context(|| format!("notify {:?}", notice.title))?;
    Ok(())
}

/// Plans, sends and saves. A notice whose delivery fails is not marked as
/// fired, so the next run tries it again. Returns how many were shown.
pub fn run(
    tasks: &[Task],
    projects: &[Project],
    path: &Path,
    now: DateTime<Utc>,
    config: &Config,
    nag: bool,
) -> Result<usize> {
    let state = State::load(path)?;
    let (notices, mut next) = plan(tasks, projects, &state, now, config, nag);
    let mut sent = 0;
    let mut failures = Vec::new();
    for notice in &notices {
        match send(notice) {
            Ok(()) => sent += 1,
            Err(e) => {
                if notice.remember {
                    next.fired
                        .retain(|f| !(f.uid == notice.uid && f.at == notice.at));
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
