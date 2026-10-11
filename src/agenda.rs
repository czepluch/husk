//! What a view shows and how: view membership, sort order, subtask
//! nesting, and the due and alarm texts. Every function takes `now`
//! explicitly, so the TUI, `husk list` and the tests agree on one answer.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local, NaiveTime, TimeDelta, Utc};

use crate::config::Config;
use crate::model::{Alarm, Anchor, Due, Priority, ProjectId, Task};
use crate::time;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Overdue,
    Today,
    Upcoming,
    All,
    Project(ProjectId),
}

/// How a due date relates to now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Overdue,
    Today,
    /// Within the next seven days.
    Soon,
    Later,
    None,
}

/// Places subtasks right after their parents, with their depth. A task
/// whose parent is not in the list, or is done, stays where the sort put
/// it at depth 0; a parent chain that never reaches a root (a cycle) falls
/// back to sort order, also at depth 0.
pub fn nest(sorted: Vec<&Task>) -> Vec<(&Task, usize)> {
    let parents: HashSet<&str> = sorted
        .iter()
        .filter(|t| !t.is_done())
        .map(|t| t.uid.as_str())
        .collect();
    let mut children: HashMap<&str, Vec<&Task>> = HashMap::new();
    let mut roots = Vec::new();
    for task in &sorted {
        match task
            .parent
            .as_deref()
            .filter(|p| parents.contains(p) && *p != task.uid)
        {
            Some(parent) => children.entry(parent).or_default().push(task),
            None => roots.push(*task),
        }
    }
    let mut out = Vec::with_capacity(sorted.len());
    let mut seen = HashSet::new();
    for root in roots {
        push_tree(root, 0, &children, &mut out, &mut seen);
    }
    for task in sorted {
        if !seen.contains(task.uid.as_str()) {
            out.push((task, 0));
        }
    }
    out
}

fn push_tree<'a>(
    task: &'a Task,
    depth: usize,
    children: &HashMap<&str, Vec<&'a Task>>,
    out: &mut Vec<(&'a Task, usize)>,
    seen: &mut HashSet<&'a str>,
) {
    if !seen.insert(task.uid.as_str()) {
        return;
    }
    out.push((task, depth));
    if let Some(kids) = children.get(task.uid.as_str()) {
        for kid in kids {
            push_tree(kid, depth + 1, children, out, seen);
        }
    }
}

pub fn bucket(due: Option<Due>, now: DateTime<Local>) -> Bucket {
    let today = now.date_naive();
    if crate::alarms::is_overdue(due, now.to_utc()) {
        return Bucket::Overdue;
    }
    let Some(due) = due else {
        return Bucket::None;
    };
    let date = time::due_date(due);
    if date == today {
        Bucket::Today
    } else if date <= today + TimeDelta::days(7) {
        Bucket::Soon
    } else {
        Bucket::Later
    }
}

/// Done tasks are hidden unless asked for. When shown, they qualify by when
/// they were finished: Today shows what was completed today, Upcoming the
/// last seven days, All and the projects everything, Overdue nothing.
pub fn in_view(task: &Task, view: &View, now: DateTime<Local>, show_done: bool) -> bool {
    if task.is_done() {
        if !show_done {
            return false;
        }
        return match view {
            View::All => true,
            View::Project(id) => &task.project == id,
            View::Overdue => false,
            View::Today | View::Upcoming => {
                let Some(at) = task.completed else {
                    return false;
                };
                let days = (now.date_naive() - time::local_date(at)).num_days();
                if *view == View::Today {
                    days == 0
                } else {
                    (0..=7).contains(&days)
                }
            }
        };
    }
    match view {
        View::All => true,
        View::Project(id) => &task.project == id,
        View::Overdue => bucket(task.due, now) == Bucket::Overdue,
        View::Today => matches!(bucket(task.due, now), Bucket::Overdue | Bucket::Today),
        View::Upcoming => matches!(bucket(task.due, now), Bucket::Today | Bucket::Soon),
    }
}

/// Overdue first, then by due time, then priority, then creation time.
/// Undated tasks follow the dated ones and done tasks come last.
pub fn sort_key(
    task: &Task,
    now: DateTime<Local>,
) -> (u8, Option<DateTime<Utc>>, Priority, Option<DateTime<Utc>>) {
    let rank = if task.is_done() {
        3
    } else {
        match bucket(task.due, now) {
            Bucket::Overdue => 0,
            Bucket::None => 2,
            _ => 1,
        }
    };
    (rank, task.due.map(instant), task.priority, task.created)
}

/// The instant a due date sorts at; all-day dates sort at local midnight.
pub fn instant(due: Due) -> DateTime<Utc> {
    match due {
        Due::DateTime(at) => at,
        Due::Date(date) => time::wall_to_utc(&Local, date.and_time(NaiveTime::MIN)),
    }
}

/// A short due column: the time for today, a weekday within a week either
/// way, the configured date format beyond that.
pub fn due_label(due: Option<Due>, now: DateTime<Local>, config: &Config) -> String {
    let Some(due) = due else {
        return "-".to_string();
    };
    let (date, time) = match due {
        Due::Date(date) => (date, None),
        Due::DateTime(at) => {
            let local = at.with_timezone(&Local);
            (
                local.date_naive(),
                Some(local.format(&config.time_format).to_string()),
            )
        }
    };
    let day = match (date - now.date_naive()).num_days() {
        0 => None,
        -6..=6 => Some(date.format("%a").to_string()),
        _ => Some(date.format(&config.date_format).to_string()),
    };
    match (day, time) {
        (None, Some(time)) => time,
        (None, None) => "today".to_string(),
        (Some(day), Some(time)) => format!("{day} {time}"),
        (Some(day), None) => day,
    }
}

/// The full due date for the detail view, with how far away it is.
pub fn due_detail(due: Due, now: DateTime<Local>, config: &Config) -> String {
    let (text, date) = match due {
        Due::Date(date) => (date.format(&config.date_format).to_string(), date),
        Due::DateTime(at) => {
            let local = at.with_timezone(&Local);
            (
                local.format(&config.date_time_format()).to_string(),
                local.date_naive(),
            )
        }
    };
    let relative = match (date - now.date_naive()).num_days() {
        0 if bucket(Some(due), now) == Bucket::Overdue => "earlier today".to_string(),
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        n if n > 0 => format!("in {n} days"),
        n => format!("{} days ago", -n),
    };
    format!("{text} ({relative})")
}

/// A timestamp in local time, for the detail view.
pub fn stamp(at: DateTime<Utc>, config: &Config) -> String {
    at.with_timezone(&Local)
        .format(&config.date_time_format())
        .to_string()
}

pub fn alarm_text(alarm: &Alarm, config: &Config) -> String {
    match alarm {
        Alarm::Absolute(at) => stamp(*at, config),
        Alarm::Relative { offset, anchor } => {
            let anchor = match anchor {
                Anchor::Due => "due",
                Anchor::Start => "start",
            };
            let seconds = offset.num_seconds();
            if seconds == 0 {
                return format!("at {anchor}");
            }
            let amount = human_duration(seconds.abs());
            if seconds < 0 {
                format!("{amount} before {anchor}")
            } else {
                format!("{amount} after {anchor}")
            }
        }
    }
}

fn human_duration(seconds: i64) -> String {
    let (n, unit) = if seconds % 86_400 == 0 {
        (seconds / 86_400, "d")
    } else if seconds % 3_600 == 0 {
        (seconds / 3_600, "h")
    } else if seconds % 60 == 0 {
        (seconds / 60, "m")
    } else {
        (seconds, "s")
    };
    format!("{n}{unit}")
}
