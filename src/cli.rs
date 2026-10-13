//! The scripting side: `husk add` and `husk list`. Same store, same
//! grammar and same view rules as the TUI, so a Waybar module or a
//! keybind sees exactly what the TUI shows.

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::Serialize;

use crate::agenda::{Bucket, View, bucket, due_label, in_view, nest, sort_key};
use crate::alarms;
use crate::caldav;
use crate::config::Config;
use crate::model::{Due, NewTask, Priority, Project, ProjectId, Task, find_project, project_name};
use crate::quickadd;
use crate::store::Store;

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
    pub priority: Priority,
    pub tags: Vec<String>,
    pub recurring: bool,
    /// UID of the parent task, when it has one.
    pub parent: Option<String>,
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
    nest(picked)
        .into_iter()
        .map(|(task, _)| Row {
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
            priority: task.priority,
            tags: task.tags.clone(),
            recurring: task.is_recurring(),
            parent: task.parent.clone(),
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
        let marker = row.priority.marker();
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

/// Creates a list on the server, then its vdir directory under the same
/// id, so the next discover pairs them and metasync has nothing to argue
/// about. Needs `[caldav]` in the config.
pub fn add_project(
    store: &dyn Store,
    config: &Config,
    name: &str,
    color: Option<&str>,
) -> Result<Project> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("a list needs a name");
    }
    let caldav = config.caldav.as_ref().context(
        "creating a list needs [caldav] url, username and password_command in the config",
    )?;
    if let Some(color) = color
        && !is_color(color)
    {
        anyhow::bail!("color must be #RRGGBB, not {color:?}");
    }
    if find_project(&store.projects()?, name).is_some() {
        anyhow::bail!("a list named {name:?} already exists");
    }
    // Upper case like the ids Apple Reminders makes.
    let id = ProjectId::new(uuid::Uuid::new_v4().to_string().to_uppercase());
    caldav::create_list(caldav, id.as_str(), name, color)?;
    store.create_project(&id, name, color)
}

/// What `remove_project` does with the tasks of a list that holds some.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Leftover {
    /// Refuse to remove the list.
    Keep,
    /// Move them to the named list first.
    MoveTo(String),
    Delete,
}

/// Deletes a list on the server (a list already gone there counts), then
/// its vdir directory. Tasks in it are moved or deleted first as `leftover`
/// says; by default a list with tasks is refused.
pub fn remove_project(
    store: &dyn Store,
    config: &Config,
    name: &str,
    leftover: &Leftover,
) -> Result<Project> {
    let projects = store.projects()?;
    let project = find_project(&projects, name.trim())
        .with_context(|| format!("no list named {name:?}"))?
        .clone();
    let caldav = config.caldav.as_ref().context(
        "removing a list needs [caldav] url, username and password_command in the config",
    )?;
    let tasks = store.tasks(Some(&project.id))?;
    let target = match leftover {
        Leftover::Keep if !tasks.is_empty() => anyhow::bail!(
            "{} holds {} tasks: move them with --move-to <list> or drop them with --delete-tasks",
            project.name,
            tasks.len()
        ),
        Leftover::MoveTo(target) => {
            let target = find_project(&projects, target)
                .with_context(|| format!("no list named {target:?}"))?;
            if target.id == project.id {
                anyhow::bail!("cannot move the tasks of {} into itself", project.name);
            }
            Some(target.id.clone())
        }
        Leftover::Keep | Leftover::Delete => None,
    };
    // The server goes first, so a refused request changes nothing locally.
    caldav::delete_list(caldav, project.id.as_str())?;
    for task in &tasks {
        match &target {
            Some(target) => store.move_to(&task.uid, target)?,
            None => store.delete(&task.uid)?,
        }
    }
    store.delete_project(&project.id)?;
    Ok(project)
}

fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
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
    store.create(
        &project,
        NewTask {
            summary: parsed.summary,
            description: None,
            due: parsed.due,
            priority: parsed.priority,
            tags: parsed.tags,
            alarms: alarms::for_new(parsed.due, &config.default_alarm_leads),
        },
    )
}
