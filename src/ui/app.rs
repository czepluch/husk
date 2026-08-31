//! TUI state and key handling. What is shown (view membership, order, due
//! labels) is decided by functions that take `now` explicitly, so tests can
//! pin them without a terminal. Rendering lives in `views.rs`.

use chrono::{DateTime, Local, NaiveTime, TimeDelta, TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::model::{Alarm, Anchor, Due, Priority, Project, ProjectId, Task};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Today,
    Upcoming,
    All,
    Project(ProjectId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Views,
    Tasks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Detail,
    Filter,
    Help,
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

pub struct App {
    pub config: Config,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub pane: Pane,
    pub view_index: usize,
    /// The selected task's UID. The list is re-sorted as time passes, so the
    /// selection follows the task rather than a position.
    selected: Option<String>,
    /// Where the cursor was, used when the selected task leaves the list.
    cursor: usize,
    pub mode: Mode,
    /// The mode `?` was pressed in, restored when the help closes.
    pub help_from: Mode,
    pub filter: String,
    /// Whether completed and cancelled tasks are shown in the current view.
    pub show_done: bool,
    /// Lines scrolled off the top of the detail view.
    pub detail_scroll: u16,
    pub now: DateTime<Local>,
    pub quit: bool,
}

impl App {
    pub fn new(
        config: Config,
        projects: Vec<Project>,
        tasks: Vec<Task>,
        now: DateTime<Local>,
    ) -> Self {
        Self {
            config,
            projects,
            tasks,
            pane: Pane::Tasks,
            view_index: 0,
            selected: None,
            cursor: 0,
            mode: Mode::Normal,
            help_from: Mode::Normal,
            filter: String::new(),
            show_done: false,
            detail_scroll: 0,
            now,
            quit: false,
        }
    }

    /// The left pane: the three smart views, then one view per project.
    pub fn views(&self) -> Vec<View> {
        [View::Today, View::Upcoming, View::All]
            .into_iter()
            .chain(self.projects.iter().map(|p| View::Project(p.id.clone())))
            .collect()
    }

    pub fn view(&self) -> View {
        self.views()
            .get(self.view_index)
            .cloned()
            .unwrap_or(View::All)
    }

    pub fn view_name(&self, view: &View) -> String {
        match view {
            View::Today => "Today".to_string(),
            View::Upcoming => "Upcoming".to_string(),
            View::All => "All".to_string(),
            View::Project(id) => self.project_name(id),
        }
    }

    pub fn project_name(&self, id: &ProjectId) -> String {
        self.projects
            .iter()
            .find(|p| &p.id == id)
            .map_or_else(|| id.as_str().to_string(), |p| p.name.clone())
    }

    /// Pending tasks in a view, ignoring the filter.
    pub fn count(&self, view: &View) -> usize {
        self.tasks
            .iter()
            .filter(|t| in_view(t, view, self.now, false))
            .count()
    }

    /// The tasks of the current view that match the filter, in display order.
    pub fn visible_tasks(&self) -> Vec<&Task> {
        let view = self.view();
        let mut tasks: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| in_view(t, &view, self.now, self.show_done))
            .filter(|t| self.matches_filter(t))
            .collect();
        tasks.sort_by_cached_key(|t| sort_key(t, self.now));
        tasks
    }

    /// The position of the selected task in the visible list.
    pub fn task_index(&self) -> usize {
        self.index_in(&self.visible_tasks())
    }

    fn index_in(&self, visible: &[&Task]) -> usize {
        self.selected
            .as_ref()
            .and_then(|uid| visible.iter().position(|t| &t.uid == uid))
            .unwrap_or_else(|| step(self.cursor, 0, visible.len()))
    }

    pub fn selected_task(&self) -> Option<&Task> {
        let visible = self.visible_tasks();
        visible.get(self.index_in(&visible)).copied()
    }

    /// Visible pending tasks that are due today or overdue.
    pub fn due_count(&self) -> usize {
        self.visible_tasks()
            .iter()
            .filter(|t| !t.is_done())
            .filter(|t| matches!(bucket(t.due, self.now), Bucket::Overdue | Bucket::Today))
            .count()
    }

    fn matches_filter(&self, task: &Task) -> bool {
        let needle = self.filter.trim().trim_start_matches('#').to_lowercase();
        if needle.is_empty() {
            return true;
        }
        task.summary.to_lowercase().contains(&needle)
            || task.tags.iter().any(|t| t.to_lowercase().contains(&needle))
            || self
                .project_name(&task.project)
                .to_lowercase()
                .contains(&needle)
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::Filter => self.filter_key(key),
            Mode::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?' | 'q')
                ) {
                    self.mode = self.help_from;
                }
            }
            Mode::Detail => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down => self.move_task(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_task(-1),
                KeyCode::Char('J') | KeyCode::PageDown => {
                    self.detail_scroll = self.detail_scroll.saturating_add(5);
                }
                KeyCode::Char('K') | KeyCode::PageUp => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(5);
                }
                KeyCode::Char('?') => self.open_help(),
                _ => {}
            },
            Mode::Normal => self.normal_key(key.code),
        }
        self.clamp();
    }

    fn normal_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('c') => self.show_done = !self.show_done,
            KeyCode::Tab | KeyCode::BackTab => {
                self.pane = match self.pane {
                    Pane::Views => Pane::Tasks,
                    Pane::Tasks => Pane::Views,
                }
            }
            KeyCode::Enter => match self.pane {
                Pane::Views => self.pane = Pane::Tasks,
                Pane::Tasks => {
                    if self.selected_task().is_some() {
                        self.mode = Mode::Detail;
                        self.detail_scroll = 0;
                    }
                }
            },
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_cursor(isize::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.move_cursor(isize::MAX / 2),
            KeyCode::Esc => self.filter.clear(),
            _ => {}
        }
    }

    fn open_help(&mut self) {
        self.help_from = self.mode;
        self.mode = Mode::Help;
    }

    fn filter_key(&mut self, key: KeyEvent) {
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(c) if plain => self.filter.push(c),
            _ => {}
        }
        self.reset_cursor();
    }

    fn move_cursor(&mut self, delta: isize) {
        match self.pane {
            Pane::Views => {
                self.view_index = step(self.view_index, delta, self.views().len());
                self.reset_cursor();
            }
            Pane::Tasks => self.move_task(delta),
        }
    }

    fn move_task(&mut self, delta: isize) {
        let (index, uid) = {
            let visible = self.visible_tasks();
            let index = step(self.index_in(&visible), delta, visible.len());
            (index, visible.get(index).map(|t| t.uid.clone()))
        };
        self.select(index, uid);
    }

    fn select(&mut self, index: usize, uid: Option<String>) {
        if uid != self.selected {
            self.detail_scroll = 0;
        }
        self.cursor = index;
        self.selected = uid;
    }

    fn reset_cursor(&mut self) {
        self.cursor = 0;
        self.selected = None;
        self.detail_scroll = 0;
    }

    /// Keeps the cursor inside the lists and anchors the selection on a task.
    fn clamp(&mut self) {
        self.view_index = step(self.view_index, 0, self.views().len());
        let (index, uid) = {
            let visible = self.visible_tasks();
            let index = self.index_in(&visible);
            (index, visible.get(index).map(|t| t.uid.clone()))
        };
        self.select(index, uid);
    }
}

fn step(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = isize::try_from(len - 1).unwrap_or(isize::MAX);
    let current = isize::try_from(index).unwrap_or(isize::MAX);
    usize::try_from(current.saturating_add(delta).clamp(0, max)).unwrap_or(0)
}

pub fn bucket(due: Option<Due>, now: DateTime<Local>) -> Bucket {
    let today = now.date_naive();
    let date = match due {
        None => return Bucket::None,
        Some(Due::Date(date)) => date,
        Some(Due::DateTime(at)) => {
            if at < now.to_utc() {
                return Bucket::Overdue;
            }
            at.with_timezone(&Local).date_naive()
        }
    };
    if date < today {
        Bucket::Overdue
    } else if date == today {
        Bucket::Today
    } else if date <= today + TimeDelta::days(7) {
        Bucket::Soon
    } else {
        Bucket::Later
    }
}

/// Done tasks are hidden unless asked for. When shown, they qualify by when
/// they were finished: Today shows what was completed today, Upcoming the
/// last seven days, All and the projects everything.
pub fn in_view(task: &Task, view: &View, now: DateTime<Local>, show_done: bool) -> bool {
    if task.is_done() {
        if !show_done {
            return false;
        }
        return match view {
            View::All => true,
            View::Project(id) => &task.project == id,
            View::Today | View::Upcoming => {
                let Some(at) = task.completed else {
                    return false;
                };
                let days = (now.date_naive() - at.with_timezone(&Local).date_naive()).num_days();
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
        View::Today => matches!(bucket(task.due, now), Bucket::Overdue | Bucket::Today),
        View::Upcoming => matches!(
            bucket(task.due, now),
            Bucket::Overdue | Bucket::Today | Bucket::Soon
        ),
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
        Due::Date(date) => {
            let midnight = date.and_time(NaiveTime::MIN);
            Local
                .from_local_datetime(&midnight)
                .earliest()
                .map_or_else(|| midnight.and_utc(), |local| local.to_utc())
        }
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
            let format = format!("{} {}", config.date_format, config.time_format);
            (local.format(&format).to_string(), local.date_naive())
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
    let format = format!("{} {}", config.date_format, config.time_format);
    at.with_timezone(&Local).format(&format).to_string()
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
