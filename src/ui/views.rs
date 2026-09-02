//! Rendering of the panes, the detail view, the bottom bar and the help
//! overlay. Colors come from `Theme` slots only.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{
    App, Bucket, FORM_FIELDS, FormField, InputKind, Mode, Pane, SMART_VIEWS, View, alarm_text,
    bucket, byte_of, due_detail, due_label, stamp,
};
use crate::model::{Priority, Status, Task};
use crate::recur;
use crate::theme::Theme;

const VIEWS_WIDTH: u16 = 24;
/// A list row shows at most this many lines of a long summary.
const MAX_ROWS: usize = 3;
const ELLIPSIS: &str = "…";

pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let area = frame.area();
    frame.render_widget(Block::new().style(theme.base), area);
    let [main, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Length(VIEWS_WIDTH), Constraint::Min(20)]).areas(main);
    draw_views(frame, app, theme, left);
    if showing_detail(app) {
        draw_detail(frame, app, theme, right);
    } else {
        draw_tasks(frame, app, theme, right);
    }
    draw_bar(frame, app, theme, bar);
    match app.mode {
        Mode::Help => draw_help(frame, theme, area),
        Mode::Pick => draw_picker(frame, app, theme, area),
        Mode::Form => draw_form(frame, app, theme, area),
        _ => {}
    }
}

/// The detail stays on screen while a prompt, confirmation, picker or the
/// help that was opened from it is showing.
fn showing_detail(app: &App) -> bool {
    match app.mode {
        Mode::Detail => true,
        Mode::Help => app.help_from == Mode::Detail,
        Mode::Input | Mode::Confirm | Mode::Pick | Mode::Form => app.return_to == Mode::Detail,
        Mode::Normal | Mode::Filter => false,
    }
}

fn block<'a>(theme: &Theme, title: impl Into<Line<'a>>, active: bool) -> Block<'a> {
    let title = title.into().style(theme.title);
    match theme.border_type {
        Some(border_type) => Block::bordered()
            .border_type(border_type)
            .border_style(if active {
                theme.border_active
            } else {
                theme.border
            })
            .title(title),
        None => Block::new().title(title),
    }
}

/// Cuts text to a display width, ending in an ellipsis when it was longer.
fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push_str(ELLIPSIS);
    out
}

fn draw_views(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let inner_width = usize::from(area.width.saturating_sub(2));
    let name_width = inner_width.saturating_sub(6);
    let mut items = Vec::new();
    let mut selected = 0;
    for (i, view) in app.views().iter().enumerate() {
        if i == SMART_VIEWS.len() {
            items.push(ListItem::new(Line::styled(
                "─".repeat(inner_width),
                theme.border,
            )));
        }
        let count = app.count(view);
        let style = match view {
            View::Project(id) => app
                .projects
                .iter()
                .find(|p| &p.id == id)
                .and_then(|p| p.color.as_deref())
                .and_then(|color| theme.hex_style(color))
                .unwrap_or(theme.project),
            View::Overdue if count > 0 => theme.overdue,
            _ => Style::default(),
        };
        let name = fit(&app.view_name(view), name_width);
        let pad = " ".repeat(name_width.saturating_sub(name.width()));
        let row = format!(" {name}{pad}{count:>4} ");
        items.push(ListItem::new(Line::styled(row, style)));
        if i == app.view_index {
            selected = items.len() - 1;
        }
    }
    let list = List::new(items)
        .block(block(theme, " Projects ", app.pane == Pane::Views))
        .highlight_style(theme.selected);
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_tasks(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let tasks = app.visible_tasks();
    let name = app.view_name(&app.view());
    let mut title = format!(" {name}");
    if !app.filter.is_empty() {
        title.push_str(&format!(" /{}", app.filter));
    }
    if app.show_done {
        title.push_str(" +done");
    }
    title.push(' ');
    let mut block = block(theme, title, app.pane == Pane::Tasks);
    if area.width >= 40 {
        let due = Line::styled(format!(" {} due ", app.due_count()), theme.muted).right_aligned();
        block = block.title_top(due);
    }
    if tasks.is_empty() {
        let empty = Paragraph::new(Line::styled("No tasks", theme.muted))
            .centered()
            .block(block);
        frame.render_widget(empty, area);
        return;
    }
    // The due column is as wide as the widest label on screen, so a long
    // date format never pushes the summary around.
    let label_width = tasks
        .iter()
        .map(|t| due_label(t.due, app.now, &app.config).width())
        .max()
        .unwrap_or(5)
        .clamp(5, 20);
    let items: Vec<ListItem> = app
        .visible_with_depth()
        .into_iter()
        .map(|(task, depth)| {
            ListItem::new(task_item(task, app, theme, area.width, label_width, depth))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selected);
    let mut state = ListState::default().with_selected(Some(app.task_index()));
    frame.render_stateful_widget(list, area, &mut state);
}

fn due_style(task: &Task, app: &App, theme: &Theme) -> Style {
    match bucket(task.due, app.now) {
        Bucket::Overdue => theme.overdue,
        Bucket::Today => theme.due_today,
        Bucket::Soon => theme.due_soon,
        Bucket::Later | Bucket::None => theme.muted,
    }
}

fn priority_style(priority: Priority, theme: &Theme) -> Style {
    match priority {
        Priority::High => theme.pri_high,
        Priority::Medium => theme.pri_medium,
        Priority::Low => theme.pri_low,
        Priority::None => Style::default(),
    }
}

fn priority_marker(priority: Priority) -> &'static str {
    match priority {
        Priority::High => "!!!",
        Priority::Medium => "!!",
        Priority::Low => "!",
        Priority::None => "",
    }
}

/// One list item: flag, due label and priority columns, then the summary,
/// tags and recurring symbol wrapped by word to the pane width. Continuation
/// lines sit under the summary column; at most `MAX_ROWS` lines are shown.
fn task_item<'a>(
    task: &'a Task,
    app: &App,
    theme: &Theme,
    pane_width: u16,
    label_width: usize,
    depth: usize,
) -> Text<'a> {
    let done = task.is_done();
    let dim = |style: Style| if done { theme.done } else { style };
    let due = dim(due_style(task, app, theme));
    let flag = if done {
        theme.symbols.done.clone()
    } else if bucket(task.due, app.now) == Bucket::Overdue {
        theme.symbols.overdue.clone()
    } else {
        " ".to_string()
    };
    let label = due_label(task.due, app.now, &app.config);
    let marker = priority_marker(task.priority);
    let prefix = vec![
        Span::styled(format!(" {flag} "), due),
        Span::styled(format!("{label:<label_width$} "), due),
        Span::styled(format!("{marker:<3} "), dim(theme.muted)),
    ];
    let prefix_width = label_width + 8;

    let text = dim(priority_style(task.priority, theme));
    let mut words: Vec<Span<'a>> = Vec::new();
    if depth > 0 {
        words.push(Span::styled(
            format!("{}{}", "  ".repeat(depth - 1), theme.symbols.subtask),
            dim(theme.muted),
        ));
    }
    words.extend(
        task.summary
            .split_whitespace()
            .map(|word| Span::styled(word, text)),
    );
    words.extend(
        task.tags
            .iter()
            .map(|tag| Span::styled(format!("#{tag}"), dim(theme.tag))),
    );
    if let Some(rule) = &task.rrule {
        words.push(Span::styled(
            theme.symbols.recurring.clone(),
            dim(theme.recurring),
        ));
        words.extend(
            recur::describe(rule)
                .split_whitespace()
                .map(|word| Span::styled(word.to_string(), dim(theme.recurring))),
        );
    }

    let width = usize::from(pane_width.saturating_sub(2))
        .saturating_sub(prefix_width)
        .max(8);
    let mut rows = wrap_words(words, width);
    if rows.len() > MAX_ROWS {
        rows.truncate(MAX_ROWS);
        let last = &mut rows[MAX_ROWS - 1];
        while row_width(last) + 2 > width && last.len() > 1 {
            last.pop();
        }
        last.push(Span::styled(ELLIPSIS, dim(theme.muted)));
    }
    let mut lines = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        let mut spans = if i == 0 {
            prefix.clone()
        } else {
            vec![Span::raw(" ".repeat(prefix_width))]
        };
        for (j, word) in row.into_iter().enumerate() {
            if j > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(word);
        }
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(prefix));
    }
    Text::from(lines)
}

fn row_width(row: &[Span]) -> usize {
    row.iter().map(Span::width).sum::<usize>() + row.len().saturating_sub(1)
}

/// Greedy word wrap on display width. A word wider than a line is broken
/// by width, so a long URL or a run of CJK still shows in full.
fn wrap_words<'a>(words: Vec<Span<'a>>, width: usize) -> Vec<Vec<Span<'a>>> {
    let mut rows: Vec<Vec<Span<'a>>> = Vec::new();
    let mut current: Vec<Span<'a>> = Vec::new();
    let mut used = 0;
    for word in words.into_iter().flat_map(|w| break_word(w, width)) {
        let w = word.width();
        if !current.is_empty() && used + 1 + w > width {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        used = if current.is_empty() { w } else { used + 1 + w };
        current.push(word);
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

fn break_word<'a>(word: Span<'a>, width: usize) -> Vec<Span<'a>> {
    if word.width() <= width {
        return vec![word];
    }
    let style = word.style;
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut used = 0;
    for c in word.content.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width && !piece.is_empty() {
            pieces.push(Span::styled(std::mem::take(&mut piece), style));
            used = 0;
        }
        piece.push(c);
        used += w;
    }
    if !piece.is_empty() {
        pieces.push(Span::styled(piece, style));
    }
    pieces
}

fn draw_detail(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = block(theme, " Task ", true);
    let Some(task) = app.selected_task() else {
        frame.render_widget(Paragraph::new("").block(block), area);
        return;
    };
    let config = &app.config;
    let label = |text: &str| Span::styled(format!(" {text:<10}"), theme.muted);
    let status = match task.status {
        Status::NeedsAction => "Needs action",
        Status::InProcess => "In process",
        Status::Completed => "Completed",
        Status::Cancelled => "Cancelled",
    };
    let priority = match task.priority {
        Priority::High => "High",
        Priority::Medium => "Medium",
        Priority::Low => "Low",
        Priority::None => "None",
    };
    let mut lines = vec![
        Line::from(vec![
            label("Summary"),
            Span::styled(task.summary.clone(), theme.title),
        ]),
        Line::from(vec![
            label("Project"),
            Span::styled(app.project_name(&task.project), theme.project),
        ]),
        Line::from(vec![
            label("Due"),
            match task.due {
                Some(due) => Span::styled(
                    due_detail(due, app.now, config),
                    due_style(task, app, theme),
                ),
                None => Span::styled("-", theme.muted),
            },
        ]),
    ];
    if let Some(start) = task.start {
        lines.push(Line::from(vec![
            label("Start"),
            Span::raw(due_detail(start, app.now, config)),
        ]));
    }
    lines.push(Line::from(vec![
        label("Priority"),
        Span::styled(priority, priority_style(task.priority, theme)),
    ]));
    lines.push(Line::from(vec![label("Status"), Span::raw(status)]));
    if let Some(at) = task.completed {
        lines.push(Line::from(vec![
            label("Completed"),
            Span::styled(stamp(at, config), theme.done),
        ]));
    }
    if !task.tags.is_empty() {
        let tags: Vec<String> = task.tags.iter().map(|t| format!("#{t}")).collect();
        lines.push(Line::from(vec![
            label("Tags"),
            Span::styled(tags.join(" "), theme.tag),
        ]));
    }
    if let Some(rrule) = &task.rrule {
        lines.push(Line::from(vec![
            label("Repeats"),
            Span::styled(
                format!("{} {}", theme.symbols.recurring, recur::describe(rrule)),
                theme.recurring,
            ),
            Span::styled(format!("  {rrule}"), theme.muted),
        ]));
    }
    if !task.alarms.is_empty() {
        let alarms: Vec<String> = task
            .alarms
            .iter()
            .map(|a| format!("{} {}", theme.symbols.alarm, alarm_text(a, config)))
            .collect();
        lines.push(Line::from(vec![
            label("Alarms"),
            Span::raw(alarms.join("   ")),
        ]));
    }
    if let Some(parent) = &task.parent {
        lines.push(Line::from(vec![label("Parent"), Span::raw(parent.clone())]));
    }
    if let Some(created) = task.created {
        lines.push(Line::from(vec![
            label("Created"),
            Span::styled(stamp(created, config), theme.muted),
        ]));
    }
    lines.push(Line::from(vec![
        label("UID"),
        Span::styled(task.uid.clone(), theme.muted),
    ]));
    if let Some(notes) = &task.description {
        lines.push(Line::raw(""));
        lines.push(Line::styled(" Notes", theme.muted));
        for line in notes.lines() {
            lines.push(Line::raw(format!(" {line}")));
        }
    }
    let detail = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0))
        .block(block);
    frame.render_widget(detail, area);
}

fn draw_bar(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    // While a prompt is open, the reason it was refused sits at the right
    // end of the bar, where the sync status otherwise goes.
    let status = match (&app.mode, &app.message) {
        (Mode::Input | Mode::Form, Some(message)) => Some((message.clone(), true)),
        _ => sync_status(app),
    };
    let status_width =
        u16::try_from(status.as_ref().map_or(0, |(t, _)| t.width() + 2)).unwrap_or(0);
    let [left, right] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(status_width)]).areas(area);
    let line = match app.mode {
        Mode::Filter => {
            let (before, after) = app.filter.split_at(byte_of(&app.filter, app.filter_cursor));
            Line::from(vec![
                Span::styled(" /", theme.help_key),
                Span::styled(before.to_string(), theme.base),
                Span::styled("▌", theme.accent),
                Span::styled(after.to_string(), theme.base),
            ])
        }
        Mode::Input => prompt_line(app, theme),
        Mode::Confirm => Line::from(vec![
            Span::styled(
                format!(
                    " Delete \"{}\"? ",
                    app.confirm
                        .as_ref()
                        .map_or("", |(_, summary)| summary.as_str())
                ),
                theme.base,
            ),
            Span::styled("y", theme.help_key),
            Span::raw("/"),
            Span::styled("n", theme.help_key),
        ]),
        Mode::Pick => hints(
            &[("j/k", "choose"), ("Enter", "move"), ("Esc", "cancel")],
            theme,
        ),
        Mode::Form => hints(
            &[
                ("Tab", "next field"),
                ("Enter", "save (on notes: edit)"),
                ("Esc", "cancel"),
            ],
            theme,
        ),
        _ if app.message.is_some() => Line::from(vec![
            Span::raw(" "),
            Span::styled(app.message.clone().unwrap_or_default(), theme.accent),
        ]),
        Mode::Detail => hints(
            &[
                ("Esc", "back"),
                ("j/k", "next/prev"),
                ("J/K", "scroll"),
                ("d", "done"),
                ("e", "edit"),
                ("n", "notes"),
                ("o", "raw"),
                ("?", "help"),
            ],
            theme,
        ),
        Mode::Normal | Mode::Help => hints(
            &[
                ("a", "add"),
                ("d", "done"),
                ("x", "del"),
                ("u", "undo"),
                ("e", "edit"),
                ("t", "due"),
                ("p", "pri"),
                ("m", "move"),
                ("/", "filter"),
                ("s", "sync"),
                ("?", "help"),
            ],
            theme,
        ),
    };
    frame.render_widget(Paragraph::new(line).style(theme.status_bar), left);
    if let Some((text, failed)) = status {
        let style = if failed {
            theme.overdue
        } else {
            theme.status_bar
        };
        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {text} "), style)).right_aligned(),
            right,
        );
    }
}

/// The sync summary for the bar's right end, and whether it is an error.
fn sync_status(app: &App) -> Option<(String, bool)> {
    if !app.sync_enabled() {
        return None;
    }
    let state = app.sync_state();
    if state.running {
        return Some(("syncing…".to_string(), false));
    }
    if let Some(error) = state.last_error {
        return Some((error, true));
    }
    state.last_ok.map(|at| {
        (
            format!("synced {}", at.format(&app.config.time_format)),
            false,
        )
    })
}

fn prompt_line<'a>(app: &App, theme: &Theme) -> Line<'a> {
    let Some(input) = &app.input else {
        return Line::raw("");
    };
    let (prompt, hint) = match input.kind {
        InputKind::Due => (
            "due",
            "  today | fri | +2d | 2026-09-03, optional HH:MM; empty clears",
        ),
        InputKind::Tags => ("tags", "  comma separated"),
    };
    // The hint is a placeholder: shown while the line is empty, gone once
    // typing starts, so nothing shifts under the cursor.
    let placeholder = if input.buffer.is_empty() { hint } else { "" };
    let (before, after) = input.buffer.split_at(byte_of(&input.buffer, input.cursor));
    Line::from(vec![
        Span::styled(format!(" {prompt}> "), theme.help_key),
        Span::styled(before.to_string(), theme.base),
        Span::styled("▌", theme.accent),
        Span::styled(after.to_string(), theme.base),
        Span::styled(placeholder.to_string(), theme.muted),
    ])
}

fn draw_picker(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let width = 40.min(area.width.saturating_sub(2));
    let height = u16::try_from(app.projects.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|p| ListItem::new(Line::styled(format!(" {} ", p.name), theme.project)))
        .collect();
    frame.render_widget(Clear, popup);
    let list = List::new(items)
        .style(theme.base)
        .block(block(theme, " Move to ", true))
        .highlight_style(theme.selected);
    let mut state = ListState::default().with_selected(Some(app.pick_index));
    frame.render_stateful_widget(list, popup, &mut state);
}

fn draw_form(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let Some(form) = &app.form else {
        return;
    };
    let width = 64.min(area.width.saturating_sub(2));
    let height = u16::try_from(FORM_FIELDS.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let project = app
        .projects
        .get(form.project)
        .map_or(String::new(), |p| p.name.clone());
    let lines: Vec<Line> = FORM_FIELDS
        .iter()
        .map(|field| {
            let focused = *field == form.field;
            let label = match field {
                FormField::Title => "title",
                FormField::Due => "due",
                FormField::Priority => "priority",
                FormField::Tags => "tags",
                FormField::Project => "project",
                FormField::Notes => "notes",
            };
            let marker = if focused { " > " } else { "   " };
            let mut spans = vec![Span::styled(
                format!("{marker}{label:<9}"),
                if focused { theme.title } else { theme.help_key },
            )];
            match field {
                FormField::Title | FormField::Due | FormField::Tags => {
                    let text = match field {
                        FormField::Title => &form.title,
                        FormField::Due => &form.due,
                        _ => &form.tags,
                    };
                    if focused {
                        let budget = usize::from(width).saturating_sub(15);
                        let (before, after) = window(text, form.cursor, budget);
                        spans.push(Span::styled(before, theme.base));
                        spans.push(Span::styled("▌", theme.accent));
                        spans.push(Span::styled(after, theme.base));
                    } else {
                        spans.push(Span::styled(text.clone(), theme.base));
                    }
                    if text.is_empty() && *field == FormField::Title {
                        spans.push(Span::styled(
                            " due:fri 09:00 pri:H +tag @project works here",
                            theme.muted,
                        ));
                    }
                }
                FormField::Priority => {
                    let (text, style) = match form.priority {
                        Priority::High => ("high", theme.pri_high),
                        Priority::Medium => ("medium", theme.pri_medium),
                        Priority::Low => ("low", theme.pri_low),
                        Priority::None => ("none", theme.muted),
                    };
                    spans.push(Span::styled(format!("< {text} >"), style));
                }
                FormField::Project => {
                    spans.push(Span::styled(format!("< {project} >"), theme.project));
                }
                FormField::Notes => {
                    let preview = form.notes.lines().next().unwrap_or_default();
                    if preview.is_empty() {
                        spans.push(Span::styled("Enter opens $EDITOR", theme.muted));
                    } else {
                        spans.push(Span::styled(preview.to_string(), theme.base));
                        if form.notes.lines().count() > 1 {
                            spans.push(Span::styled(" …", theme.muted));
                        }
                        spans.push(Span::styled("  (Enter edits)", theme.muted));
                    }
                }
            }
            Line::from(spans)
        })
        .collect();
    let title = if form.uid.is_some() {
        " Edit task "
    } else {
        " Add task "
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(theme.base)
            .block(block(theme, title, true)),
        popup,
    );
}

/// A window of `width` characters around the cursor, split at it, so a
/// long value scrolls instead of pushing the cursor out of the popup.
fn window(text: &str, cursor: usize, width: usize) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let width = width.max(8);
    let start = (cursor + 1).saturating_sub(width);
    let end = (start + width).min(chars.len());
    (
        chars[start..cursor].iter().collect(),
        chars[cursor..end].iter().collect(),
    )
}

fn hints(pairs: &[(&str, &str)], theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (key, label) in pairs {
        spans.push(Span::styled((*key).to_string(), theme.help_key));
        spans.push(Span::styled(format!(" {label}  "), theme.status_bar));
    }
    Line::from(spans)
}

fn draw_help(frame: &mut Frame, theme: &Theme, area: Rect) {
    let rows = [
        ("j / k", "move (arrows work too), g / G first / last"),
        ("Tab", "switch pane"),
        ("Enter", "open the task, or focus the task list"),
        ("J / K", "scroll the open task"),
        (
            "a",
            "add in a form; the title takes due:fri pri:H +tag @project",
        ),
        ("d", "done / reopen (recurring tasks: on the phone)"),
        ("x", "delete, after confirming"),
        ("u", "undo the last change"),
        ("e / t", "edit in a form / edit the due date"),
        ("p / T", "cycle priority / edit tags"),
        ("m / n", "move to a project / edit notes in $EDITOR"),
        ("o", "edit the raw .ics in $EDITOR (the UID must stay)"),
        ("/", "filter by text; Enter keeps it, Esc clears it"),
        ("arrows", "move the cursor in a prompt; Ctrl jumps a word"),
        ("c", "show or hide completed tasks"),
        ("s", "sync now"),
        ("?", "this help"),
        ("q", "quit"),
    ];
    let width = 62.min(area.width.saturating_sub(2));
    let height = u16::try_from(rows.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let lines: Vec<Line> = rows
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!(" {key:<8}"), theme.help_key),
                Span::raw(*what),
            ])
        })
        .collect();
    frame.render_widget(Clear, popup);
    let help = Paragraph::new(Text::from(lines))
        .style(theme.base)
        .block(block(theme, " Help ", true));
    frame.render_widget(help, popup);
}
