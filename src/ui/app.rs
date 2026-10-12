//! TUI state and key handling. What a view shows, and in which order, is
//! decided in `agenda.rs`; rendering lives in `views.rs`. Every write goes
//! through the `Store`, is followed by a reload, and asks the syncer to run.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::agenda::{Bucket, View, bucket, due_label, in_view, nest, sort_key};
use crate::alarms;
use crate::cli;
use crate::config::Config;
use crate::ical::{codec, tz, vtodo};
use crate::model::{Alarm, Due, NewTask, Priority, Project, ProjectId, Task};
use crate::quickadd;
use crate::recur;
use crate::store::Store;
use crate::sync::{SyncState, Syncer};
use crate::time::Zone;

const UNDO_DEPTH: usize = 20;

/// The smart views ahead of the projects in the left pane.
pub const SMART_VIEWS: [View; 5] = [
    View::Overdue,
    View::Today,
    View::Upcoming,
    View::Urgent,
    View::All,
];
/// How long a bar message stays without a key press.
const MESSAGE_SECONDS: i64 = 5;

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
    /// A one-line text prompt; which one is in `App::input`.
    Input,
    /// Waiting for y/n on a delete.
    Confirm,
    /// Choosing a project to move the task to.
    Pick,
    /// The add or edit form.
    Form,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Due,
    Tags,
    Project,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub kind: InputKind,
    pub buffer: String,
    /// Cursor position in the buffer, counted in characters.
    pub cursor: usize,
    /// What the buffer started as; an unchanged prompt changes nothing.
    pub initial: String,
    /// The task being edited.
    pub uid: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    Title,
    Due,
    Priority,
    Tags,
    Project,
    Notes,
}

pub const FORM_FIELDS: [FormField; 6] = [
    FormField::Title,
    FormField::Due,
    FormField::Priority,
    FormField::Tags,
    FormField::Project,
    FormField::Notes,
];

/// The add or edit form. The text fields share one cursor, moved to the
/// end of a field's text when it gains focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    /// The task being edited; none when adding.
    pub uid: Option<String>,
    pub title: String,
    /// What the title started as; an untouched title is never fed to the
    /// quick-add grammar, so `@` or `+` words in it survive a save.
    initial_title: String,
    /// The task as the form opened it; the save is refused when the task
    /// changed underneath in the meantime (a sync, an editor).
    original: Option<Box<Task>>,
    pub due: String,
    /// What the due field started as; unchanged text leaves the task's
    /// due date alone, so a re-parse cannot shave off odd seconds.
    initial_due: String,
    pub priority: Priority,
    pub tags: String,
    /// Index into the app's projects.
    pub project: usize,
    pub notes: String,
    pub field: FormField,
    pub cursor: usize,
}

impl Form {
    /// Moves focus by `delta` through the fields, wrapping past either
    /// end, and puts the cursor at the end of the newly focused text.
    fn focus(&mut self, delta: isize) {
        let at = FORM_FIELDS
            .iter()
            .position(|f| *f == self.field)
            .unwrap_or(0);
        self.field = FORM_FIELDS[step_wrap(at, delta, FORM_FIELDS.len())];
        self.cursor = self.text().map_or(0, |t| t.chars().count());
    }

    /// The focused field's text, when it is a text field.
    pub fn text(&self) -> Option<&str> {
        match self.field {
            FormField::Title => Some(&self.title),
            FormField::Due => Some(&self.due),
            FormField::Tags => Some(&self.tags),
            _ => None,
        }
    }
}

/// Text the run loop should open in `$EDITOR`, and what to do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorRequest {
    pub text: String,
    pub target: EditorTarget,
}

/// What the text edited in `$EDITOR` becomes when it comes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTarget {
    /// This task's notes.
    Notes(String),
    /// This task's whole `.ics` file.
    Raw(String),
    /// The notes field of the open form.
    FormNotes,
}

enum Undo {
    /// The task as it was before a change or a delete.
    Restore(Box<Task>),
    /// A task created here.
    Created(String),
}

pub struct App {
    pub config: Config,
    store: Box<dyn Store>,
    syncer: Syncer,
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
    /// The mode an action was started from, restored when it ends.
    pub return_to: Mode,
    pub filter: String,
    /// Cursor position in the filter, counted in characters.
    pub filter_cursor: usize,
    /// Whether completed and cancelled tasks are shown in the current view.
    pub show_done: bool,
    /// Lines scrolled off the top of the detail view.
    pub detail_scroll: u16,
    pub input: Option<Input>,
    /// The add or edit form while one is open.
    pub form: Option<Form>,
    /// UID and summary of the task a delete is waiting on.
    pub confirm: Option<(String, String)>,
    /// Cursor in the project picker, and the task it will move.
    pub pick_index: usize,
    pick_uid: Option<String>,
    /// Shown in the bar until the next key or for a few seconds.
    pub message: Option<String>,
    message_at: DateTime<Local>,
    undo: Vec<Undo>,
    editor_request: Option<EditorRequest>,
    seen_runs: u64,
    /// The store's change stamp at the last reload.
    stamp: u64,
    pub now: DateTime<Local>,
    pub quit: bool,
}

impl App {
    /// Loads projects and tasks from the store and starts the syncer.
    pub fn new(config: Config, store: Box<dyn Store>, now: DateTime<Local>) -> Result<Self> {
        let projects = store.projects()?;
        let tasks = store.tasks(None)?;
        let stamp = store.stamp().unwrap_or(0);
        let syncer = Syncer::new(config.sync_command.clone());
        // Pick up what the phones did since the last run.
        syncer.request();
        Ok(Self {
            config,
            store,
            syncer,
            projects,
            tasks,
            pane: Pane::Tasks,
            view_index: 1,
            selected: None,
            cursor: 0,
            mode: Mode::Normal,
            help_from: Mode::Normal,
            return_to: Mode::Normal,
            filter: String::new(),
            filter_cursor: 0,
            show_done: false,
            detail_scroll: 0,
            input: None,
            form: None,
            confirm: None,
            pick_index: 0,
            pick_uid: None,
            message: None,
            message_at: now,
            undo: Vec::new(),
            editor_request: None,
            seen_runs: 0,
            stamp,
            now,
            quit: false,
        })
    }

    /// The left pane: the smart views, then one view per project.
    pub fn views(&self) -> Vec<View> {
        SMART_VIEWS
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
            View::Overdue => "Overdue".to_string(),
            View::Today => "Today".to_string(),
            View::Upcoming => "Upcoming".to_string(),
            View::Urgent => "Urgent".to_string(),
            View::All => "All".to_string(),
            View::Project(id) => self.project_name(id),
        }
    }

    pub fn project_name(&self, id: &ProjectId) -> String {
        crate::model::project_name(&self.projects, id)
    }

    /// A project by directory name or display name, case-insensitively.
    pub fn find_project(&self, name: &str) -> Option<ProjectId> {
        crate::model::find_project(&self.projects, name).map(|p| p.id.clone())
    }

    /// Pending tasks in a view, ignoring the filter.
    pub fn count(&self, view: &View) -> usize {
        self.tasks
            .iter()
            .filter(|t| in_view(t, view, self.now, false))
            .count()
    }

    /// The tasks of the current view that match the filter, in display
    /// order: sorted, then each subtask placed right after its parent when
    /// the parent is in the list too.
    pub fn visible_tasks(&self) -> Vec<&Task> {
        self.visible_with_depth()
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    /// `visible_tasks` with each task's nesting depth, 0 for a top-level row.
    pub fn visible_with_depth(&self) -> Vec<(&Task, usize)> {
        let view = self.view();
        let mut tasks: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| in_view(t, &view, self.now, self.show_done))
            .filter(|t| self.matches_filter(t))
            .collect();
        tasks.sort_by_cached_key(|t| sort_key(t, self.now));
        nest(tasks)
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

    pub fn sync_state(&self) -> SyncState {
        self.syncer.state()
    }

    pub fn sync_enabled(&self) -> bool {
        !self.config.sync_command.is_empty()
    }

    /// Reloads when the store changed underneath (a vdirsyncer run husk did
    /// not start, a hand edit) or when a sync run this app asked for ended.
    pub fn poll(&mut self) {
        if self.message.is_some() && (self.now - self.message_at).num_seconds() >= MESSAGE_SECONDS {
            self.message = None;
        }
        let state = self.syncer.state();
        let finished = state.runs != self.seen_runs;
        let changed = self.store.stamp().is_ok_and(|stamp| stamp != self.stamp);
        if finished || changed {
            self.seen_runs = state.runs;
            self.reload();
        }
        if finished
            && let Some(error) = state.last_error
            && self.message.is_none()
        {
            self.notify(error);
        }
    }

    pub fn sync_busy(&self) -> bool {
        self.syncer.busy()
    }

    /// Waits for a pending or running sync; false when it did not finish in time.
    pub fn flush_sync(&self, timeout: std::time::Duration) -> bool {
        self.syncer.flush(timeout)
    }

    /// The editor session the run loop should perform, if any.
    pub fn take_editor_request(&mut self) -> Option<EditorRequest> {
        self.editor_request.take()
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
        self.message = None;
        match self.mode {
            Mode::Filter => self.filter_key(key),
            Mode::Input => self.input_key(key),
            Mode::Form => self.form_key(key),
            Mode::Confirm => self.confirm_key(key.code),
            Mode::Pick => self.pick_key(key.code),
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
                code => self.action_key(code),
            },
            Mode::Normal => self.normal_key(key.code),
        }
        self.clamp();
    }

    fn normal_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('/') => {
                self.filter_cursor = self.filter.chars().count();
                self.mode = Mode::Filter;
            }
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
            code => self.action_key(code),
        }
    }

    /// Keys that act on the selected task, shared by the list and the detail.
    fn action_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('a') => self.open_form(None),
            KeyCode::Char('s') => {
                if self.sync_enabled() {
                    self.syncer.request();
                    self.notify("Sync requested".to_string());
                } else {
                    self.notify("Sync is off: sync_command is empty".to_string());
                }
            }
            KeyCode::Char('P') => {
                self.start_input(InputKind::Project, String::new(), String::new())
            }
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char('d') => self.toggle_done(),
            KeyCode::Char('x') => {
                if let Some(task) = self.selected_task() {
                    self.confirm = Some((task.uid.clone(), task.summary.clone()));
                    self.return_to = self.mode;
                    self.mode = Mode::Confirm;
                }
            }
            KeyCode::Char('e') => {
                let uid = self.selected_task().map(|t| t.uid.clone());
                if let Some(uid) = uid {
                    self.open_form(Some(&uid));
                }
            }
            KeyCode::Char('t') => {
                if let Some(task) = self.selected_task() {
                    let (uid, text) = (task.uid.clone(), due_input(task.due, &self.config));
                    self.start_input(InputKind::Due, text, uid);
                }
            }
            KeyCode::Char('T') => {
                if let Some(task) = self.selected_task() {
                    let (uid, text) = (task.uid.clone(), task.tags.join(", "));
                    self.start_input(InputKind::Tags, text, uid);
                }
            }
            KeyCode::Char('p') => self.cycle_priority(),
            KeyCode::Char('m') => {
                let target = self
                    .selected_task()
                    .map(|t| (t.uid.clone(), t.project.clone()));
                if let Some((uid, project)) = target {
                    self.pick_index = self
                        .projects
                        .iter()
                        .position(|p| p.id == project)
                        .unwrap_or(0);
                    self.pick_uid = Some(uid);
                    self.return_to = self.mode;
                    self.mode = Mode::Pick;
                }
            }
            KeyCode::Char('n') => {
                if let Some(task) = self.selected_task() {
                    self.editor_request = Some(EditorRequest {
                        text: task.description.clone().unwrap_or_default(),
                        target: EditorTarget::Notes(task.uid.clone()),
                    });
                }
            }
            KeyCode::Char('o') => {
                if let Some(task) = self.selected_task() {
                    self.editor_request = Some(EditorRequest {
                        text: codec::serialize(task.raw()),
                        target: EditorTarget::Raw(task.uid.clone()),
                    });
                }
            }
            _ => {}
        }
    }

    fn open_help(&mut self) {
        self.help_from = self.mode;
        self.mode = Mode::Help;
    }

    fn start_input(&mut self, kind: InputKind, buffer: String, uid: String) {
        self.input = Some(Input {
            kind,
            initial: buffer.clone(),
            cursor: buffer.chars().count(),
            buffer,
            uid,
        });
        self.return_to = self.mode;
        self.mode = Mode::Input;
    }

    fn filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.filter_cursor = 0;
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.mode = Mode::Normal,
            _ => edit_key(&mut self.filter, &mut self.filter_cursor, key),
        }
        self.reset_cursor();
    }

    fn input_key(&mut self, key: KeyEvent) {
        let Some(mut input) = self.input.take() else {
            self.mode = Mode::Normal;
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = self.return_to,
            KeyCode::Enter => match self.submit(&input) {
                Ok(text) => {
                    self.mode = self.return_to;
                    self.report(Ok(text));
                }
                // The prompt stays open with its text so the mistake can be fixed.
                Err(e) => {
                    self.notify(format!("{e:#}"));
                    self.input = Some(input);
                }
            },
            _ => {
                edit_key(&mut input.buffer, &mut input.cursor, key);
                self.input = Some(input);
            }
        }
    }

    fn confirm_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                if let Some((uid, _)) = self.confirm.take() {
                    let result = self.delete(&uid);
                    self.report(result);
                }
                // The deleted task's detail would show its neighbour.
                self.mode = Mode::Normal;
            }
            KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {
                self.confirm = None;
                self.mode = self.return_to;
            }
            _ => {}
        }
    }

    fn pick_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.pick_index = step_wrap(self.pick_index, 1, self.projects.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.pick_index = step_wrap(self.pick_index, -1, self.projects.len());
            }
            KeyCode::Enter => {
                self.mode = self.return_to;
                if let (Some(uid), Some(project)) =
                    (self.pick_uid.take(), self.projects.get(self.pick_index))
                {
                    let project = project.id.clone();
                    let result = self.move_task_to(&uid, &project);
                    self.report(result);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.pick_uid = None;
                self.mode = self.return_to;
            }
            _ => {}
        }
    }

    fn submit(&mut self, input: &Input) -> Result<String> {
        if input.buffer == input.initial {
            return Ok(String::new());
        }
        let uid = &input.uid;
        match input.kind {
            InputKind::Due => {
                let text = input.buffer.trim();
                let due = if text.is_empty() {
                    None
                } else {
                    Some(read_due(text, self.now)?)
                };
                let leads = self.config.default_alarm_leads.clone();
                self.change(uid, "Due set", move |task| {
                    alarms::set_due(task, due, &leads);
                    Ok(())
                })
            }
            InputKind::Tags => {
                let tags = split_tags(&input.buffer);
                self.change(uid, "Tags set", |task| {
                    task.tags = tags;
                    Ok(())
                })
            }
            InputKind::Project => self.add_project(&input.buffer),
        }
    }

    /// Creates the list on the server and in the vdir, shows it, and lets
    /// the next sync run discover so vdirsyncer pairs the two sides.
    fn add_project(&mut self, name: &str) -> Result<String> {
        let project = cli::add_project(self.store.as_ref(), &self.config, name, None)?;
        self.reload();
        if let Some(i) = self
            .views()
            .iter()
            .position(|v| *v == View::Project(project.id.clone()))
        {
            self.view_index = i;
            self.reset_cursor();
        }
        self.syncer.request_discover();
        // Back to the list, never into a detail of the empty new project.
        self.return_to = Mode::Normal;
        Ok(format!("Created {}", project.name))
    }

    /// Opens the form empty, or prefilled from a task.
    fn open_form(&mut self, uid: Option<&str>) {
        let form = match uid.and_then(|uid| self.tasks.iter().find(|t| t.uid == uid)) {
            Some(task) => {
                let due = due_input(task.due, &self.config);
                Form {
                    uid: Some(task.uid.clone()),
                    title: task.summary.clone(),
                    initial_title: task.summary.clone(),
                    original: Some(Box::new(task.clone())),
                    initial_due: due.clone(),
                    due,
                    priority: task.priority,
                    tags: task.tags.join(", "),
                    project: self
                        .projects
                        .iter()
                        .position(|p| p.id == task.project)
                        .unwrap_or(0),
                    notes: task.description.clone().unwrap_or_default(),
                    field: FormField::Title,
                    cursor: task.summary.chars().count(),
                }
            }
            None => Form {
                uid: None,
                title: String::new(),
                initial_title: String::new(),
                original: None,
                due: String::new(),
                initial_due: String::new(),
                priority: Priority::None,
                tags: String::new(),
                project: self.form_default_project(),
                notes: String::new(),
                field: FormField::Title,
                cursor: 0,
            },
        };
        self.form = Some(form);
        self.return_to = self.mode;
        self.mode = Mode::Form;
    }

    /// Where a new task lands before `@project` or the selector says
    /// otherwise: the current project view, else `default_project`, else
    /// the first project.
    fn form_default_project(&self) -> usize {
        let id = match self.view() {
            View::Project(id) => Some(id),
            _ => self
                .config
                .default_project
                .as_deref()
                .and_then(|name| self.find_project(name)),
        };
        id.and_then(|id| self.projects.iter().position(|p| p.id == id))
            .unwrap_or(0)
    }

    fn form_key(&mut self, key: KeyEvent) {
        let Some(mut form) = self.form.take() else {
            self.mode = Mode::Normal;
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.mode = self.return_to;
                return;
            }
            KeyCode::Enter => match self.form_submit(&form) {
                Ok(text) => {
                    self.mode = self.return_to;
                    self.report(Ok(text));
                    return;
                }
                // The form stays open so the mistake can be fixed.
                Err(e) => self.notify(format!("{e:#}")),
            },
            KeyCode::Tab | KeyCode::Down => form.focus(1),
            KeyCode::BackTab | KeyCode::Up => form.focus(-1),
            _ => match form.field {
                FormField::Title => edit_key(&mut form.title, &mut form.cursor, key),
                FormField::Due => edit_key(&mut form.due, &mut form.cursor, key),
                FormField::Tags => edit_key(&mut form.tags, &mut form.cursor, key),
                FormField::Priority => match key.code {
                    KeyCode::Left => form.priority = priority_back(form.priority),
                    KeyCode::Right | KeyCode::Char(' ') => {
                        form.priority = priority_next(form.priority);
                    }
                    _ => {}
                },
                FormField::Project => match key.code {
                    KeyCode::Left => {
                        form.project = step_wrap(form.project, -1, self.projects.len());
                    }
                    KeyCode::Right | KeyCode::Char(' ') => {
                        form.project = step_wrap(form.project, 1, self.projects.len());
                    }
                    _ => {}
                },
                FormField::Notes => {
                    if key.code == KeyCode::Char('e') {
                        self.editor_request = Some(EditorRequest {
                            text: form.notes.clone(),
                            target: EditorTarget::FormNotes,
                        });
                    }
                }
            },
        }
        self.form = Some(form);
    }

    /// Saves the form: creates a task, or applies every field to the one
    /// being edited. Grammar tokens typed in the title (`due:`, `pri:`,
    /// `+tag`, `@project`) win over the corresponding fields.
    fn form_submit(&mut self, form: &Form) -> Result<String> {
        let parsed = if form.title == form.initial_title {
            quickadd::QuickAdd {
                summary: form.title.trim().to_string(),
                ..quickadd::QuickAdd::default()
            }
        } else {
            quickadd::parse(&form.title, self.now)
        };
        if parsed.summary.is_empty() {
            return Err(anyhow!("a task needs a title"));
        }
        let due_text = form.due.trim();
        let due = match (parsed.due, due_text) {
            (Some(due), _) => Some(due),
            (None, "") => None,
            (None, text) => Some(read_due(text, self.now)?),
        };
        let due_touched = parsed.due.is_some() || due_text != form.initial_due.trim();
        let priority = if parsed.priority == Priority::None {
            form.priority
        } else {
            parsed.priority
        };
        let mut tags = split_tags(&form.tags);
        for tag in parsed.tags {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        let project = match &parsed.project {
            Some(name) => self
                .find_project(name)
                .with_context(|| format!("no project named {name:?}"))?,
            None => self
                .projects
                .get(form.project)
                .map(|p| p.id.clone())
                .context("no projects")?,
        };
        let notes = form.notes.trim_end().to_string();
        let leads = self.config.default_alarm_leads.clone();
        match &form.uid {
            None => {
                let new = NewTask {
                    summary: parsed.summary.clone(),
                    description: (!notes.is_empty()).then_some(notes),
                    due,
                    priority,
                    tags,
                    alarms: alarms::for_new(due, &leads),
                };
                let task = self.store.create(&project, new)?;
                self.push_undo(Undo::Created(task.uid.clone()));
                self.after_write(Some(&task.uid));
                Ok(format!("Added: {} (u undo)", parsed.summary))
            }
            Some(uid) => {
                let uid = uid.clone();
                let current = self
                    .tasks
                    .iter()
                    .find(|t| t.uid == uid)
                    .cloned()
                    .with_context(|| format!("task {uid} is no longer listed"))?;
                if form.original.as_ref().is_some_and(|o| o.raw != current.raw) {
                    anyhow::bail!("task changed since the form opened; press Esc and reopen");
                }
                let before = current.clone();
                let mut task = current;
                task.summary = parsed.summary;
                task.priority = priority;
                task.tags = tags;
                task.description = (!notes.is_empty()).then_some(notes);
                if due_touched && task.due != due {
                    alarms::set_due(&mut task, due, &leads);
                }
                if let Err(e) = self.store.save(&mut task) {
                    self.reload();
                    return Err(e);
                }
                let changed = task.raw != before.raw;
                let moved = task.project != project;
                if moved && let Err(e) = self.store.move_to(&uid, &project) {
                    // The field edits reached the disk even though the
                    // move failed, so undo must cover them.
                    if changed {
                        self.push_undo(Undo::Restore(Box::new(before)));
                        self.after_write(Some(&uid));
                    }
                    return Err(e);
                }
                if !changed && !moved {
                    return Ok(String::new());
                }
                let summary = task.summary.clone();
                self.push_undo(Undo::Restore(Box::new(before)));
                self.after_write(Some(&uid));
                Ok(format!("Saved: {summary} (u undo)"))
            }
        }
    }

    /// Notes back from `$EDITOR` into the open form.
    pub fn set_form_notes(&mut self, text: &str) {
        match &mut self.form {
            Some(form) => form.notes = text.trim_end().to_string(),
            None => self.notify("The form closed; the edited notes were dropped".to_string()),
        }
    }
    fn toggle_done(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };
        let (uid, done) = (task.uid.clone(), task.is_done());
        if !done && let Some(rule) = task.rrule.clone() {
            self.complete_occurrence(&uid, &rule);
            return;
        }
        let now = self.now.to_utc();
        let label = if done { "Reopened" } else { "Done" };
        let result = self.change(&uid, label, move |task| {
            if done {
                task.reopen();
            } else {
                task.complete(now);
            }
            Ok(())
        });
        self.report(result);
    }

    /// `d` on a recurring task: the due date moves to the rule's next
    /// occurrence and the task stays pending, with absolute alarms keeping
    /// their wall-clock time and a counted rule losing one count.
    /// Completing the last occurrence completes the task itself. Wall
    /// times live in the due's own `TZID` zone when it names one, else
    /// the local zone. A rule the computation does not cover is left to
    /// the phone apps.
    fn complete_occurrence(&mut self, uid: &str, rule: &str) {
        let Some(task) = self.tasks.iter().find(|t| t.uid == uid) else {
            return;
        };
        let Some(due) = task.due else {
            self.notify("No due date to advance; complete it on the phone".to_string());
            return;
        };
        let zone = Zone::from_tz(tz::due_zone(task));
        let from = match due {
            Due::Date(date) => date,
            Due::DateTime(at) => zone.wall(at).date(),
        };
        match recur::advance(rule, from) {
            recur::Advance::Unsupported => {
                self.notify("This rule is completed on the phone".to_string());
            }
            recur::Advance::Ended => {
                let now = self.now.to_utc();
                let result = self.change(uid, "Done for good", move |task| {
                    task.complete(now);
                    Ok(())
                });
                self.report(result);
            }
            recur::Advance::Next { date, rrule } => {
                let days = date - from;
                let next = match due {
                    Due::Date(_) => Due::Date(date),
                    Due::DateTime(at) => Due::DateTime(zone.add_days(at, days)),
                };
                let label = format!(
                    "Done, next {}",
                    due_label(Some(next), self.now, &self.config)
                );
                let result = self.change(uid, label.trim_end(), move |task| {
                    task.due = Some(next);
                    if let Some(rrule) = rrule {
                        task.rrule = Some(rrule);
                    }
                    for alarm in &mut task.alarms {
                        if let Alarm::Absolute(at) = alarm {
                            *at = zone.add_days(*at, days);
                        }
                    }
                    Ok(())
                });
                self.report(result);
            }
        }
    }

    fn cycle_priority(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };
        let (uid, current) = (task.uid.clone(), task.priority);
        let next = priority_next(current);
        let result = self.change(&uid, "Priority set", move |task| {
            task.priority = next;
            Ok(())
        });
        self.report(result);
    }

    /// Notes as saved from `$EDITOR`; trailing blank lines are dropped.
    pub fn apply_notes(&mut self, uid: &str, text: &str) {
        let notes = text.trim_end().to_string();
        let result = self.change(uid, "Notes saved", move |task| {
            task.description = if notes.is_empty() { None } else { Some(notes) };
            Ok(())
        });
        self.report(result);
    }

    /// The raw `.ics` as saved from `$EDITOR`: parsed, checked to hold this
    /// one task, then written through the store with a sequence bump like
    /// any other edit, and refused like any other edit when the file changed
    /// on disk in the meantime.
    pub fn apply_raw(&mut self, uid: &str, text: &str) {
        let result = (|| -> Result<String> {
            let before = self
                .tasks
                .iter()
                .find(|t| t.uid == uid)
                .cloned()
                .with_context(|| format!("task {uid} is no longer listed"))?;
            let current = self.store.get(uid)?;
            if current.raw().root != before.raw().root {
                self.reload();
                anyhow::bail!("task {uid} changed on disk since it was read; reload and retry");
            }
            let doc = codec::parse(text)?;
            if doc.root.children().filter(|c| c.is("VTODO")).count() != 1 {
                anyhow::bail!("the file must hold exactly one VTODO");
            }
            let edited = vtodo::from_document(doc, before.project.clone())?;
            if edited.uid != uid {
                anyhow::bail!("the UID must stay {uid}");
            }
            // Only line endings differing is not an edit.
            if edited.raw().root == before.raw().root {
                return Ok(String::new());
            }
            let restored = self.store.restore(&edited)?;
            self.push_undo(Undo::Restore(Box::new(before)));
            self.after_write(Some(uid));
            Ok(format!("File saved: {} (u undo)", restored.summary))
        })();
        self.report(result);
    }

    /// Applies a change to the task as the user saw it and saves. The store
    /// refuses when the file changed underneath in the meantime; the list is
    /// then reloaded so the next attempt starts from what is there. A save
    /// that changes nothing leaves no undo entry and triggers no sync.
    fn change(
        &mut self,
        uid: &str,
        label: &str,
        apply: impl FnOnce(&mut Task) -> Result<()>,
    ) -> Result<String> {
        let mut task = self
            .tasks
            .iter()
            .find(|t| t.uid == uid)
            .cloned()
            .with_context(|| format!("task {uid} is no longer listed"))?;
        let before = task.clone();
        apply(&mut task)?;
        if let Err(e) = self.store.save(&mut task) {
            self.reload();
            return Err(e);
        }
        if task.raw == before.raw {
            return Ok(String::new());
        }
        self.push_undo(Undo::Restore(Box::new(before)));
        self.after_write(Some(uid));
        Ok(format!("{label}: {} (u undo)", task.summary))
    }

    fn delete(&mut self, uid: &str) -> Result<String> {
        let task = self.store.get(uid)?;
        self.store.delete(uid)?;
        let summary = task.summary.clone();
        self.push_undo(Undo::Restore(Box::new(task)));
        self.after_write(None);
        Ok(format!("Deleted: {summary} (u undo)"))
    }

    fn move_task_to(&mut self, uid: &str, project: &ProjectId) -> Result<String> {
        let task = self.store.get(uid)?;
        if &task.project == project {
            return Ok(String::new());
        }
        self.store.move_to(uid, project)?;
        self.push_undo(Undo::Restore(Box::new(task)));
        self.after_write(Some(uid));
        Ok(format!("Moved to {} (u undo)", self.project_name(project)))
    }

    fn undo(&mut self) {
        let Some(entry) = self.undo.pop() else {
            self.notify("Nothing to undo".to_string());
            return;
        };
        let result = match entry {
            Undo::Restore(task) => {
                let uid = task.uid.clone();
                // A moved task goes back the way it came (write, then delete)
                // before its old content is written over it.
                let moved_away = self
                    .store
                    .get(&uid)
                    .is_ok_and(|current| current.project != task.project);
                let outcome = if moved_away {
                    self.store
                        .move_to(&uid, &task.project)
                        .and_then(|()| self.store.restore(&task))
                } else {
                    self.store.restore(&task)
                };
                outcome.map(|restored| {
                    self.after_write(Some(&uid));
                    format!("Restored: {}", restored.summary)
                })
            }
            Undo::Created(uid) => self.store.delete(&uid).map(|()| {
                self.after_write(None);
                "Removed the added task".to_string()
            }),
        };
        self.report(result);
    }

    fn push_undo(&mut self, entry: Undo) {
        self.undo.push(entry);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    fn after_write(&mut self, select: Option<&str>) {
        self.reload();
        if let Some(uid) = select {
            self.selected = Some(uid.to_string());
        }
        self.syncer.request();
    }

    pub fn reload(&mut self) {
        self.stamp = self.store.stamp().unwrap_or(self.stamp);
        match self.store.tasks(None) {
            Ok(tasks) => self.tasks = tasks,
            Err(e) => self.message = Some(format!("{e:#}")),
        }
        if let Ok(projects) = self.store.projects() {
            self.projects = projects;
        }
    }

    fn report(&mut self, result: Result<String>) {
        match result {
            Ok(text) if text.is_empty() => self.message = None,
            Ok(text) => self.notify(text),
            Err(e) => self.notify(format!("{e:#}")),
        }
    }

    /// Shows a message in the bar until the next key or for a few seconds.
    pub fn notify(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
        self.message_at = self.now;
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

/// Applies a line-editing key to `text` at `cursor`, a position counted in
/// characters: the arrows move (Ctrl by a word), Home and End jump,
/// Backspace and Delete remove around the cursor, and a plain character is
/// inserted at it.
fn edit_key(text: &mut String, cursor: &mut usize, key: KeyEvent) {
    let chars = text.chars().count();
    *cursor = (*cursor).min(chars);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Left if ctrl => *cursor = word_left(text, *cursor),
        KeyCode::Right if ctrl => *cursor = word_right(text, *cursor),
        KeyCode::Left => *cursor = cursor.saturating_sub(1),
        KeyCode::Right => *cursor = (*cursor + 1).min(chars),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = chars,
        KeyCode::Backspace => {
            if *cursor > 0 {
                *cursor -= 1;
                remove_at(text, *cursor);
            }
        }
        KeyCode::Delete => remove_at(text, *cursor),
        KeyCode::Char(c) if plain(key) => {
            text.insert(byte_of(text, *cursor), c);
            *cursor += 1;
        }
        _ => {}
    }
}

/// The byte index of the character at `cursor`, or the end of `text`.
pub fn byte_of(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .nth(cursor)
        .map_or(text.len(), |(at, _)| at)
}

fn remove_at(text: &mut String, cursor: usize) {
    if let Some((at, _)) = text.char_indices().nth(cursor) {
        text.remove(at);
    }
}

/// The start of the word before the cursor.
fn word_left(text: &str, cursor: usize) -> usize {
    let before: Vec<char> = text.chars().take(cursor).collect();
    let space = before
        .iter()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();
    let word = before[..before.len() - space]
        .iter()
        .rev()
        .take_while(|c| !c.is_whitespace())
        .count();
    cursor - space - word
}

/// The start of the word after the cursor.
fn word_right(text: &str, cursor: usize) -> usize {
    let after: Vec<char> = text.chars().skip(cursor).collect();
    let word = after.iter().take_while(|c| !c.is_whitespace()).count();
    let space = after[word..]
        .iter()
        .take_while(|c| c.is_whitespace())
        .count();
    cursor + word + space
}

fn priority_next(priority: Priority) -> Priority {
    match priority {
        Priority::None => Priority::High,
        Priority::High => Priority::Medium,
        Priority::Medium => Priority::Low,
        Priority::Low => Priority::None,
    }
}

fn priority_back(priority: Priority) -> Priority {
    match priority {
        Priority::None => Priority::Low,
        Priority::Low => Priority::Medium,
        Priority::Medium => Priority::High,
        Priority::High => Priority::None,
    }
}

/// A due date from the prompt or the form's due field.
fn read_due(text: &str, now: DateTime<Local>) -> Result<Due> {
    quickadd::parse_due_text(text, now)
        .with_context(|| format!("could not read a date from {text:?}"))
}

/// Comma-separated input into clean unique tags; a leading `#` is noise.
fn split_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in text.split(',') {
        let tag = tag.trim().trim_start_matches('#').trim().to_string();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

fn plain(key: KeyEvent) -> bool {
    !key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

/// `step`, but past either end it comes out on the other side. For short
/// selectors; long lists clamp.
fn step_wrap(index: usize, delta: isize, len: usize) -> usize {
    let Ok(len) = isize::try_from(len) else {
        return index;
    };
    if len == 0 {
        return 0;
    }
    let current = isize::try_from(index).unwrap_or(0).min(len - 1);
    usize::try_from(current.saturating_add(delta).rem_euclid(len)).unwrap_or(0)
}

fn step(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = isize::try_from(len - 1).unwrap_or(isize::MAX);
    let current = isize::try_from(index).unwrap_or(isize::MAX);
    usize::try_from(current.saturating_add(delta).clamp(0, max)).unwrap_or(0)
}

/// The text the due prompt starts with: what the task has now, in the
/// forms the prompt reads back.
pub fn due_input(due: Option<Due>, _config: &Config) -> String {
    match due {
        None => String::new(),
        Some(Due::Date(date)) => date.format("%Y-%m-%d").to_string(),
        Some(Due::DateTime(at)) => at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    }
}
