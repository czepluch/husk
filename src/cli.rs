//! The scripting side: `husk add` and `husk list`. Same store, same
//! grammar and same view rules as the TUI, so a Waybar module or a
//! keybind sees exactly what the TUI shows.

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::Serialize;

use crate::alarms::default_alarms;
use crate::config::Config;
use crate::model::{Due, NewTask, Priority, Project, ProjectId, Task, find_project, project_name};
use crate::quickadd;
use crate::store::Store;
use crate::ui::app::{Bucket, View, bucket, due_label, in_view, sort_key};

/// One task as `husk list` reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub uid: String,
    pub summary: String,
    pub project: String,
    pub project_id: String,
    /// RFC 3339 for timed tasks, `YYYY-MM-DD` for all-day ones.
    pub due: Option<String>,
    pub due_label: String,
    pub overdue: bool,
    pub priority: &'static str,
    pub tags: Vec<String>,
    pub recurring: bool,
}

/// The tasks of a view, optionally one project, in the TUI's order.
pub fn rows(
    tasks: &[Task],
    projects: &[Project],
    view: &View,
    project: Option<&ProjectId>,
    now: DateTime<Local>,
    config: &Config,
) -> Vec<Row> {
    let mut picked: Vec<&Task> = tasks
        .iter()
        .filter(|t| in_view(t, view, now, false))
        .filter(|t| project.is_none_or(|p| &t.project == p))
        .collect();
    picked.sort_by_cached_key(|t| sort_key(t, now));
    picked
        .into_iter()
        .map(|task| Row {
            uid: task.uid.clone(),
            summary: task.summary.clone(),
            project: project_name(projects, &task.project),
            project_id: task.project.as_str().to_string(),
            due: task.due.map(|due| match due {
                Due::Date(date) => date.format("%Y-%m-%d").to_string(),
                Due::DateTime(at) => at.with_timezone(&Local).to_rfc3339(),
            }),
            due_label: due_label(task.due, now, config),
            overdue: bucket(task.due, now) == Bucket::Overdue,
            priority: match task.priority {
                Priority::High => "high",
                Priority::Medium => "medium",
                Priority::Low => "low",
                Priority::None => "none",
            },
            tags: task.tags.clone(),
            recurring: task.is_recurring(),
        })
        .collect()
}

/// One line per task: overdue mark, due label, priority marker, title,
/// tags, project.
pub fn text(rows: &[Row]) -> String {
    let width = rows.iter().map(|r| r.due_label.len()).max().unwrap_or(1);
    let mut out = String::new();
    for row in rows {
        let flag = if row.overdue { "◂" } else { " " };
        let marker = match row.priority {
            "high" => "!!!",
            "medium" => "!!",
            "low" => "!",
            _ => "",
        };
        let tags: String = row.tags.iter().map(|t| format!(" #{t}")).collect();
        out.push_str(&format!(
            "{flag} {:<width$}  {marker:<3} {}{tags}  [{}]\n",
            row.due_label, row.summary, row.project
        ));
    }
    out
}

pub fn json(rows: &[Row]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

/// Creates a task from quick-add text. The project comes from `@project`,
/// else `default_project`; there is no current view to fall back on.
pub fn add(store: &dyn Store, config: &Config, text: &str, now: DateTime<Local>) -> Result<Task> {
    let parsed = quickadd::parse(text, now);
    if parsed.summary.is_empty() {
        anyhow::bail!("a task needs a title");
    }
    let projects = store.projects()?;
    let name = parsed
        .project
        .as_deref()
        .or(config.default_project.as_deref())
        .context("no project: use @project or set default_project in the config")?;
    let project = find_project(&projects, name)
        .with_context(|| format!("no project named {name:?}"))?
        .id
        .clone();
    let alarms = match parsed.due {
        Some(Due::DateTime(_)) => default_alarms(&config.default_alarm_leads),
        _ => Vec::new(),
    };
    store.create(
        &project,
        NewTask {
            summary: parsed.summary,
            description: None,
            due: parsed.due,
            priority: parsed.priority,
            tags: parsed.tags,
            alarms,
        },
    )
}
