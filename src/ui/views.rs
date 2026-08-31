//! Rendering of the panes, the detail view, the bottom bar and the help
//! overlay. Colors come from `Theme` slots only.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Bucket, Mode, Pane, View, alarm_text, bucket, due_detail, due_label};
use crate::model::{Priority, Status, Task};
use crate::theme::Theme;

const VIEWS_WIDTH: u16 = 24;

pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let area = frame.area();
    frame.render_widget(Block::new().style(theme.base), area);
    let [main, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Length(VIEWS_WIDTH), Constraint::Min(20)]).areas(main);
    draw_views(frame, app, theme, left);
    match app.mode {
        Mode::Detail => draw_detail(frame, app, theme, right),
        _ => draw_tasks(frame, app, theme, right),
    }
    draw_bar(frame, app, theme, bar);
    if app.mode == Mode::Help {
        draw_help(frame, theme, area);
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

fn draw_views(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let inner_width = usize::from(area.width.saturating_sub(2));
    let name_width = inner_width.saturating_sub(6);
    let mut items = Vec::new();
    let mut selected = 0;
    for (i, view) in app.views().iter().enumerate() {
        if i == 3 {
            items.push(ListItem::new(Line::styled(
                "─".repeat(inner_width),
                theme.border,
            )));
        }
        let style = match view {
            View::Project(_) => theme.project,
            _ => Style::default(),
        };
        let name = app.view_name(view);
        let count = app.count(view);
        let row = format!(" {name:<name_width$}{count:>4} ");
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
    let title = if app.filter.is_empty() {
        format!(" {name} ")
    } else {
        format!(" {name} /{} ", app.filter)
    };
    let due = Line::styled(format!(" {} due ", app.due_count()), theme.muted).right_aligned();
    let block = block(theme, title, app.pane == Pane::Tasks).title_top(due);
    if tasks.is_empty() {
        let empty = Paragraph::new(Line::styled("No tasks", theme.muted))
            .centered()
            .block(block);
        frame.render_widget(empty, area);
        return;
    }
    let items: Vec<ListItem> = tasks
        .iter()
        .map(|task| ListItem::new(task_line(task, app, theme)))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selected);
    let mut state = ListState::default().with_selected(Some(app.task_index));
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

fn task_line<'a>(task: &'a Task, app: &App, theme: &Theme) -> Line<'a> {
    let due = due_style(task, app, theme);
    let flag = if bucket(task.due, app.now) == Bucket::Overdue {
        theme.symbols.overdue.clone()
    } else {
        " ".to_string()
    };
    let label = due_label(task.due, app.now, &app.config);
    let mut spans = vec![
        Span::styled(format!(" {flag} "), due),
        Span::styled(format!("{label:<10} "), due),
        Span::styled(task.summary.as_str(), priority_style(task.priority, theme)),
    ];
    for tag in &task.tags {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("#{tag}"), theme.tag));
    }
    if task.is_recurring() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            theme.symbols.recurring.clone(),
            theme.recurring,
        ));
    }
    Line::from(spans)
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
        Line::from(vec![
            label("Priority"),
            Span::styled(priority, priority_style(task.priority, theme)),
        ]),
        Line::from(vec![label("Status"), Span::raw(status)]),
    ];
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
                format!("{} {rrule}", theme.symbols.recurring),
                theme.recurring,
            ),
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
        let format = format!("{} {}", config.date_format, config.time_format);
        let text = created
            .with_timezone(&chrono::Local)
            .format(&format)
            .to_string();
        lines.push(Line::from(vec![
            label("Created"),
            Span::styled(text, theme.muted),
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
        .block(block);
    frame.render_widget(detail, area);
}

fn draw_bar(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let line = match app.mode {
        Mode::Filter => Line::from(vec![
            Span::styled(" /", theme.help_key),
            Span::raw(app.filter.clone()),
            Span::styled("▌", theme.accent),
        ]),
        Mode::Detail => hints(
            &[("Esc", "back"), ("j/k", "next/prev"), ("?", "help")],
            theme,
        ),
        Mode::Normal | Mode::Help => hints(
            &[
                ("j/k", "move"),
                ("Tab", "pane"),
                ("Enter", "detail"),
                ("/", "filter"),
                ("?", "help"),
                ("q", "quit"),
            ],
            theme,
        ),
    };
    frame.render_widget(Paragraph::new(line).style(theme.status_bar), area);
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
        ("j / k", "move (arrows work too)"),
        ("g / G", "first / last"),
        ("Tab", "switch pane"),
        ("Enter", "open the task, or focus the task list"),
        ("/", "filter by text; Enter keeps it, Esc clears it"),
        ("Esc", "back, or clear the filter"),
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
