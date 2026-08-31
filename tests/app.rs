mod common;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use husk::config::Config;
use husk::ical::vtodo;
use husk::model::{Due, Project, ProjectId, Task};
use husk::theme::Theme;
use husk::ui::app::{self, App, Bucket, Mode, Pane, View};
use husk::ui::views;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn now() -> DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 8, 31, 12, 0, 0)
        .single()
        .unwrap()
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// A local wall-clock time as the UTC value a DUE line would carry.
fn local_utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> String {
    Local
        .with_ymd_and_hms(y, m, d, h, mi, 0)
        .single()
        .unwrap()
        .to_utc()
        .format("%Y%m%dT%H%M%SZ")
        .to_string()
}

fn task(uid: &str, summary: &str, project: &str, due: Option<&str>, extra: &str) -> Task {
    let due = due.map(|d| format!("{d}\r\n")).unwrap_or_default();
    let text = format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:{uid}\r\nSUMMARY:{summary}\r\n{due}{extra}END:VTODO\r\nEND:VCALENDAR\r\n"
    );
    vtodo::parse_task(&text, ProjectId::new(project)).unwrap()
}

fn sample() -> App {
    let projects = vec![
        Project {
            id: ProjectId::new("life"),
            name: "Life".to_string(),
            color: None,
        },
        Project {
            id: ProjectId::new("argot"),
            name: "Argot".to_string(),
            color: None,
        },
    ];
    let tasks = vec![
        task(
            "later",
            "Read Crafting Interpreters",
            "life",
            Some("DUE;VALUE=DATE:20260920"),
            "CATEGORIES:reading\r\n",
        ),
        task("none", "Test task", "life", None, ""),
        task(
            "today-later",
            "Call Gustav",
            "life",
            Some(&format!("DUE:{}", local_utc(2026, 8, 31, 15, 0))),
            "",
        ),
        task(
            "soon",
            "Oil balcony",
            "argot",
            Some("DUE;VALUE=DATE:20260903"),
            "RRULE:FREQ=WEEKLY\r\nCATEGORIES:home\r\n",
        ),
        task(
            "overdue-day",
            "Pay rent",
            "argot",
            Some("DUE;VALUE=DATE:20260830"),
            "PRIORITY:1\r\n",
        ),
        task(
            "today-early",
            "Book dentist",
            "life",
            Some(&format!("DUE:{}", local_utc(2026, 8, 31, 9, 0))),
            "CATEGORIES:health\r\n",
        ),
        task(
            "today-all-day",
            "Have fun",
            "life",
            Some("DUE;VALUE=DATE:20260831"),
            "PRIORITY:5\r\n",
        ),
        task(
            "done",
            "Old thing",
            "life",
            Some("DUE;VALUE=DATE:20260830"),
            "STATUS:COMPLETED\r\n",
        ),
    ];
    App::new(Config::default(), projects, tasks, now())
}

fn uids(app: &App) -> Vec<&str> {
    app.visible_tasks().iter().map(|t| t.uid.as_str()).collect()
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer.cell((x, y)).map_or(" ", |c| c.symbol()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn buckets_follow_the_local_calendar() {
    assert_eq!(app::bucket(None, now()), Bucket::None);
    assert_eq!(
        app::bucket(Some(Due::Date(date(2026, 8, 30))), now()),
        Bucket::Overdue
    );
    assert_eq!(
        app::bucket(Some(Due::Date(date(2026, 8, 31))), now()),
        Bucket::Today
    );
    assert_eq!(
        app::bucket(Some(Due::Date(date(2026, 9, 7))), now()),
        Bucket::Soon
    );
    assert_eq!(
        app::bucket(Some(Due::Date(date(2026, 9, 8))), now()),
        Bucket::Later
    );
    let early = task(
        "e",
        "e",
        "p",
        Some(&format!("DUE:{}", local_utc(2026, 8, 31, 9, 0))),
        "",
    );
    assert_eq!(
        app::bucket(early.due, now()),
        Bucket::Overdue,
        "timed and passed"
    );
    let later = task(
        "l",
        "l",
        "p",
        Some(&format!("DUE:{}", local_utc(2026, 8, 31, 15, 0))),
        "",
    );
    assert_eq!(app::bucket(later.due, now()), Bucket::Today);
}

#[test]
fn views_select_by_bucket_hide_done_and_sort_overdue_first() {
    let mut app = sample();
    assert_eq!(app.view(), View::Today);
    assert_eq!(
        uids(&app),
        vec!["overdue-day", "today-early", "today-all-day", "today-later"]
    );
    assert_eq!(app.due_count(), 4);

    app.view_index = 1;
    assert_eq!(app.view(), View::Upcoming);
    assert_eq!(
        uids(&app),
        vec![
            "overdue-day",
            "today-early",
            "today-all-day",
            "today-later",
            "soon"
        ]
    );

    app.view_index = 2;
    assert_eq!(
        uids(&app),
        vec![
            "overdue-day",
            "today-early",
            "today-all-day",
            "today-later",
            "soon",
            "later",
            "none"
        ]
    );

    app.view_index = 4;
    assert_eq!(app.view(), View::Project(ProjectId::new("argot")));
    assert_eq!(uids(&app), vec!["overdue-day", "soon"]);

    assert_eq!(app.count(&View::Today), 4);
    assert_eq!(app.count(&View::All), 7, "done tasks are hidden");
    assert_eq!(app.views().len(), 5);
}

#[test]
fn priority_then_creation_break_ties_on_equal_due() {
    let tasks = vec![
        task(
            "none",
            "n",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "CREATED:20260801T000000Z\r\n",
        ),
        task(
            "high",
            "h",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "PRIORITY:1\r\nCREATED:20260802T000000Z\r\n",
        ),
        task(
            "low",
            "l",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "PRIORITY:9\r\n",
        ),
        task(
            "medium-older",
            "m",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "PRIORITY:5\r\nCREATED:20260801T000000Z\r\n",
        ),
        task(
            "medium-newer",
            "m",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "PRIORITY:5\r\nCREATED:20260803T000000Z\r\n",
        ),
    ];
    let mut app = App::new(Config::default(), vec![], tasks, now());
    app.view_index = 2;
    assert_eq!(
        uids(&app),
        vec!["high", "medium-older", "medium-newer", "low", "none"]
    );
}

#[test]
fn due_labels_are_short_and_relative() {
    let config = Config::default();
    let label = |due: Option<Due>| app::due_label(due, now(), &config);
    assert_eq!(label(None), "-");
    assert_eq!(label(Some(Due::Date(date(2026, 8, 31)))), "today");
    assert_eq!(label(Some(Due::Date(date(2026, 8, 30)))), "Sun");
    assert_eq!(label(Some(Due::Date(date(2026, 9, 3)))), "Thu");
    assert_eq!(label(Some(Due::Date(date(2026, 9, 20)))), "2026-09-20");
    let today = task(
        "t",
        "t",
        "p",
        Some(&format!("DUE:{}", local_utc(2026, 8, 31, 15, 0))),
        "",
    );
    assert_eq!(label(today.due), "15:00");
    let thursday = task(
        "t",
        "t",
        "p",
        Some(&format!("DUE:{}", local_utc(2026, 9, 3, 10, 0))),
        "",
    );
    assert_eq!(label(thursday.due), "Thu 10:00");

    assert_eq!(
        app::due_detail(Due::Date(date(2026, 9, 3)), now(), &config),
        "2026-09-03 (in 3 days)"
    );
    assert_eq!(
        app::due_detail(Due::Date(date(2026, 8, 30)), now(), &config),
        "2026-08-30 (yesterday)"
    );
    assert_eq!(
        app::due_detail(today.due.unwrap(), now(), &config),
        "2026-08-31 15:00 (today)"
    );
}

#[test]
fn filter_matches_summary_tags_and_project_case_insensitively() {
    let mut app = sample();
    app.view_index = 2;
    app.filter = "GUSTAV".to_string();
    assert_eq!(uids(&app), vec!["today-later"]);
    app.filter = "#health".to_string();
    assert_eq!(uids(&app), vec!["today-early"]);
    app.filter = "argot".to_string();
    assert_eq!(uids(&app), vec!["overdue-day", "soon"]);
    app.filter = "nothing here".to_string();
    assert!(uids(&app).is_empty());
    assert_eq!(app.selected_task(), None);
}

#[test]
fn keys_drive_navigation_filter_and_modes() {
    let mut app = sample();
    assert_eq!(app.pane, Pane::Tasks);
    app.handle_key(key('j'));
    assert_eq!(app.task_index, 1);
    app.handle_key(key('G'));
    assert_eq!(app.task_index, 3);
    app.handle_key(key('j'));
    assert_eq!(app.task_index, 3, "clamped at the end");
    app.handle_key(key('g'));
    assert_eq!(app.task_index, 0);
    app.handle_key(key('k'));
    assert_eq!(app.task_index, 0, "clamped at the start");

    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.pane, Pane::Views);
    app.handle_key(key('j'));
    assert_eq!(app.view(), View::Upcoming);
    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.pane, Pane::Tasks);

    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.mode, Mode::Detail);
    app.handle_key(key('j'));
    assert_eq!(app.task_index, 1, "j moves within the detail view too");
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Normal);

    app.handle_key(key('/'));
    assert_eq!(app.mode, Mode::Filter);
    for c in "oil".chars() {
        app.handle_key(key(c));
    }
    assert_eq!(app.filter, "oil");
    assert_eq!(uids(&app), vec!["soon"]);
    app.handle_key(code(KeyCode::Backspace));
    assert_eq!(app.filter, "oi");
    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.filter, "oi");
    app.handle_key(code(KeyCode::Esc));
    assert!(app.filter.is_empty());

    app.handle_key(key('?'));
    assert_eq!(app.mode, Mode::Help);
    app.handle_key(key('q'));
    assert_eq!(app.mode, Mode::Normal);
    assert!(!app.quit);
    app.handle_key(key('q'));
    assert!(app.quit);

    let mut app = sample();
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.quit);
}

#[test]
fn frames_render_the_layout_the_detail_and_the_help() {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut app = sample();
    let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();

    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    let text = screen(&terminal);
    for expected in [
        "Projects",
        "Today",
        "Upcoming",
        "All",
        "Life",
        "Argot",
        "!!! Pay rent",
        "Book dentist",
        "#health",
        "09:00",
        "◂ Sun",
        "!!  Have fun",
        "4 due",
        "j/k",
        "quit",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    assert!(!text.contains("Old thing"), "done task shown:\n{text}");
    assert!(
        !text.contains("Read Crafting"),
        "later task in Today:\n{text}"
    );

    app.handle_key(key('c'));
    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        text.lines()
            .any(|l| l.contains("✓ Sun") && l.contains("Old thing")),
        "done task with c:
{text}"
    );
    assert!(text.contains("Today +done"), "{text}");
    assert_eq!(uids(&app).last(), Some(&"done"), "done tasks sort last");
    app.handle_key(key('c'));

    app.handle_key(code(KeyCode::Enter));
    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    let text = screen(&terminal);
    for expected in [
        "Task",
        "Summary",
        "Pay rent",
        "Project",
        "Argot",
        "Priority",
        "High",
        "yesterday",
        "Esc",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }

    app.handle_key(code(KeyCode::Esc));
    app.handle_key(key('?'));
    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    let text = screen(&terminal);
    for expected in ["Help", "switch pane", "filter by text", "quit"] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }

    app.handle_key(code(KeyCode::Esc));
    app.filter = "nothing".to_string();
    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    assert!(screen(&terminal).contains("No tasks"));

    let ansi = Theme::load("ansi", None).unwrap();
    for (w, h) in [(20, 5), (5, 3), (200, 60)] {
        let mut small = Terminal::new(TestBackend::new(w, h)).unwrap();
        small.draw(|f| views::draw(f, &app, &ansi)).unwrap();
    }
}

#[test]
fn long_summaries_wrap_under_the_summary_column() {
    let theme = Theme::load("phosphor", None).unwrap();
    let long =
        "Her er en meget lang titel som med sikkerhed fylder mere end en enkelt linje i listen";
    let tasks = vec![task(
        "long",
        long,
        "p",
        Some("DUE;VALUE=DATE:20260831"),
        "CATEGORIES:tag\r\n",
    )];
    let app = App::new(Config::default(), vec![], tasks, now());
    let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
    terminal.draw(|f| views::draw(f, &app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        text.contains("│   today          Her er en meget lang"),
        "{text}"
    );
    assert!(
        text.contains("listen #tag"),
        "wrapped tail with the tag:\n{text}"
    );
    let continuation = text.lines().find(|l| l.contains("listen #tag")).unwrap();
    assert!(
        continuation.starts_with("│                  "),
        "indented under the summary column:\n{text}"
    );
    assert!(text.lines().all(|l| l.chars().count() <= 70));
}
