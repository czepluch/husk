mod common;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

use husk::config::Config;
use husk::ical::codec;
use husk::ical::vtodo;
use husk::model::{Due, ProjectId, Task};
use husk::store::VdirStore;
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

struct Sample {
    _dir: common::TempDir,
    app: App,
}

/// An app over a temp vdir holding the given projects and tasks; projects a
/// task refers to without being listed get a directory with no display name.
fn make(projects: &[(&str, &str)], tasks: Vec<Task>) -> Sample {
    let dir = common::TempDir::new();
    for (id, name) in projects {
        fs::create_dir_all(dir.path().join(id)).unwrap();
        fs::write(dir.path().join(id).join("displayname"), name).unwrap();
    }
    for t in &tasks {
        let project = dir.path().join(t.project.as_str());
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join(format!("{}.ics", t.uid)),
            codec::serialize(t.raw()),
        )
        .unwrap();
    }
    let config = Config {
        vdir: dir.path().to_path_buf(),
        sync_command: vec![],
        ..Config::default()
    };
    let app = App::new(config, Box::new(VdirStore::new(dir.path())), now()).unwrap();
    Sample { _dir: dir, app }
}

fn sample() -> Sample {
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
            "STATUS:COMPLETED\r\nCOMPLETED:20260831T101451Z\r\n",
        ),
    ];
    make(&[("life", "Life"), ("argot", "Argot")], tasks)
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
    let mut s = sample();
    let app = &mut s.app;
    assert_eq!(app.view(), View::Today);
    assert_eq!(
        uids(app),
        vec!["overdue-day", "today-early", "today-all-day", "today-later"]
    );
    assert_eq!(app.due_count(), 4);

    app.view_index = 2;
    assert_eq!(app.view(), View::Upcoming);
    assert_eq!(
        uids(app),
        vec!["today-all-day", "today-later", "soon"],
        "no overdue in Upcoming, and a passed time today is overdue"
    );

    app.view_index = 0;
    assert_eq!(app.view(), View::Overdue);
    assert_eq!(uids(app), vec!["overdue-day", "today-early"]);
    assert_eq!(app.count(&View::Overdue), 2);

    app.view_index = 3;
    assert_eq!(
        uids(app),
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
    assert_eq!(uids(app), vec!["overdue-day", "soon"]);

    assert_eq!(app.count(&View::Today), 4);
    assert_eq!(app.count(&View::All), 7, "done tasks are hidden");
    assert_eq!(app.views().len(), 6);
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
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    assert_eq!(
        uids(app),
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
    let mut s = sample();
    let app = &mut s.app;
    app.view_index = 3;
    app.filter = "GUSTAV".to_string();
    assert_eq!(uids(app), vec!["today-later"]);
    app.filter = "#health".to_string();
    assert_eq!(uids(app), vec!["today-early"]);
    app.filter = "argot".to_string();
    assert_eq!(uids(app), vec!["overdue-day", "soon"]);
    app.filter = "nothing here".to_string();
    assert!(uids(app).is_empty());
    assert_eq!(app.selected_task(), None);
}

#[test]
fn keys_drive_navigation_filter_and_modes() {
    let mut s = sample();
    let app = &mut s.app;
    assert_eq!(app.pane, Pane::Tasks);
    app.handle_key(key('j'));
    assert_eq!(app.task_index(), 1);
    app.handle_key(key('G'));
    assert_eq!(app.task_index(), 3);
    app.handle_key(key('j'));
    assert_eq!(app.task_index(), 3, "clamped at the end");
    app.handle_key(key('g'));
    assert_eq!(app.task_index(), 0);
    app.handle_key(key('k'));
    assert_eq!(app.task_index(), 0, "clamped at the start");

    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.pane, Pane::Views);
    app.handle_key(key('j'));
    assert_eq!(app.view(), View::Upcoming);
    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.pane, Pane::Tasks);

    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.mode, Mode::Detail);
    app.handle_key(key('j'));
    assert_eq!(app.task_index(), 1, "j moves within the detail view too");
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Normal);

    app.handle_key(key('/'));
    assert_eq!(app.mode, Mode::Filter);
    for c in "oil".chars() {
        app.handle_key(key(c));
    }
    assert_eq!(app.filter, "oil");
    assert_eq!(uids(app), vec!["soon"]);
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

    let mut s = sample();
    let app = &mut s.app;
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.quit);
}

#[test]
fn frames_render_the_layout_the_detail_and_the_help() {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut s = sample();
    let app = &mut s.app;
    let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();

    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    for expected in [
        "Projects",
        "Overdue",
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
        "a add",
        "? help",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    assert!(!text.contains("Old thing"), "done task shown:\n{text}");
    assert!(
        !text.contains("Read Crafting"),
        "later task in Today:\n{text}"
    );

    app.handle_key(key('c'));
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        text.lines()
            .any(|l| l.contains("✓ Sun") && l.contains("Old thing")),
        "done task with c:
{text}"
    );
    assert!(text.contains("Today +done"), "{text}");
    assert_eq!(uids(app).last(), Some(&"done"), "done tasks sort last");
    app.handle_key(key('c'));

    app.handle_key(code(KeyCode::Enter));
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
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
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    for expected in ["Help", "switch pane", "filter by text", "quit"] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }

    app.handle_key(code(KeyCode::Esc));
    app.filter = "nothing".to_string();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    assert!(screen(&terminal).contains("No tasks"));

    let ansi = Theme::load("ansi", None).unwrap();
    for (w, h) in [(20, 5), (5, 3), (200, 60)] {
        let mut small = Terminal::new(TestBackend::new(w, h)).unwrap();
        small.draw(|f| views::draw(f, app, &ansi)).unwrap();
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
    let s = make(&[], tasks);
    let app = &s.app;
    let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    let column = |line: &str, needle: &str| line.find(needle).map(|i| line[..i].chars().count());
    let first = text
        .lines()
        .find(|l| l.contains("Her er en meget"))
        .unwrap();
    assert!(first.contains("today"), "{text}");
    let tail = text.lines().find(|l| l.contains("listen #tag")).unwrap();
    let tail_start = tail
        .chars()
        .enumerate()
        .skip(25)
        .find(|(_, c)| *c != ' ')
        .map(|(i, _)| i);
    assert_eq!(
        tail_start,
        column(first, "Her er"),
        "continuation under the summary column:\n{text}"
    );
    assert!(text.lines().all(|l| l.chars().count() <= 70));
}
#[test]
fn priority_is_shown_as_title_weight() {
    use ratatui::style::Modifier;
    let theme = Theme::load("phosphor", None).unwrap();
    let tasks = vec![
        task("high", "High one", "p", None, "PRIORITY:1\r\n"),
        task("medium", "Medium one", "p", None, "PRIORITY:5\r\n"),
        task("low", "Low one", "p", None, "PRIORITY:9\r\n"),
        task("none", "Plain one", "p", None, ""),
    ];
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = screen(&terminal);
    let modifier_at = |needle: &str| {
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(needle))
            .unwrap();
        let x = line.chars().position(|_| false).unwrap_or(0) + line.find(needle).unwrap();
        let x = line[..x].chars().count();
        buffer.cell((x as u16, y as u16)).unwrap().modifier
    };
    assert!(modifier_at("High one").contains(Modifier::BOLD));
    assert!(!modifier_at("Medium one").contains(Modifier::BOLD | Modifier::DIM));
    assert!(modifier_at("Low one").contains(Modifier::DIM));
    assert!(
        text.contains("!!! High one") && text.contains("!   Low one"),
        "{text}"
    );
}

#[test]
fn selection_follows_the_task_when_time_reorders_the_list() {
    let mut s = sample();
    let app = &mut s.app;
    app.handle_key(key('j'));
    app.handle_key(key('j'));
    assert_eq!(app.selected_task().unwrap().uid, "today-all-day");
    assert_eq!(app.task_index(), 2);

    // At 15:00:01 "Call Gustav" turns overdue and jumps ahead of the all-day task.
    app.now = Local
        .with_ymd_and_hms(2026, 8, 31, 15, 0, 1)
        .single()
        .unwrap();
    assert_eq!(
        uids(app),
        vec!["overdue-day", "today-early", "today-later", "today-all-day"]
    );
    assert_eq!(app.selected_task().unwrap().uid, "today-all-day");
    assert_eq!(app.task_index(), 3);

    // When the selected task leaves the list the cursor stays where it was.
    app.filter = "gustav".to_string();
    assert_eq!(app.selected_task().unwrap().uid, "today-later");
}

#[test]
fn completed_tasks_qualify_for_today_and_upcoming_by_completion_time() {
    let mut s = sample();
    let app = &mut s.app;
    app.tasks.push(task(
        "done-old",
        "Done ten days ago",
        "life",
        Some("DUE;VALUE=DATE:20260101"),
        "STATUS:COMPLETED\r\nCOMPLETED:20260821T100000Z\r\n",
    ));
    app.tasks.push(task(
        "done-when",
        "Done, no timestamp",
        "life",
        Some("DUE;VALUE=DATE:20260101"),
        "STATUS:COMPLETED\r\n",
    ));
    app.show_done = true;
    assert_eq!(
        uids(app).last(),
        Some(&"done"),
        "completed today shows in Today"
    );
    assert!(!uids(app).contains(&"done-old"));
    assert!(!uids(app).contains(&"done-when"));

    app.view_index = 2;
    assert!(uids(app).contains(&"done"));
    assert!(
        !uids(app).contains(&"done-old"),
        "ten days ago is outside Upcoming"
    );

    app.view_index = 3;
    let all = uids(app);
    assert!(all.contains(&"done") && all.contains(&"done-old") && all.contains(&"done-when"));
    assert_eq!(app.count(&View::All), 7, "counts stay pending-only");
}

#[test]
fn filter_ignores_keys_with_control_or_alt() {
    let mut s = sample();
    let app = &mut s.app;
    app.handle_key(key('/'));
    app.handle_key(key('b'));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    app.handle_key(KeyEvent::new(KeyCode::Char('Å'), KeyModifiers::SHIFT));
    assert_eq!(app.filter, "bÅ");
}

#[test]
fn due_column_grows_for_wide_labels() {
    let theme = Theme::load("phosphor", None).unwrap();
    let tasks = vec![
        task(
            "far",
            "aaaa bbbb cccc dddd eeee",
            "p",
            Some(&format!("DUE:{}", local_utc(2026, 9, 9, 10, 0))),
            "",
        ),
        task("near", "Short", "p", Some("DUE;VALUE=DATE:20260831"), ""),
    ];
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    let far = text
        .lines()
        .find(|l| l.contains("2026-09-09 10:00"))
        .unwrap();
    let near = text.lines().find(|l| l.contains("Short")).unwrap();
    assert!(far.contains("2026-09-09 10:00     aaaa"), "{text}");
    assert_eq!(
        far.find("aaaa"),
        near.find("Short"),
        "summaries align:\n{text}"
    );
}

#[test]
fn tall_rows_are_capped_and_long_words_break() {
    let theme = Theme::load("phosphor", None).unwrap();
    let many = "et to tre fire fem seks syv otte ni ti elleve tolv tretten fjorten femten seksten sytten atten nitten tyve";
    let url = "https://example.invalid/a/very/long/path/that/has/no/spaces/at/all/anywhere";
    let tasks = vec![
        task("many", many, "p", None, ""),
        task("after", "After the tall one", "p", None, ""),
        task("url", url, "p", None, ""),
    ];
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        text.contains("…"),
        "capped rows end in an ellipsis:\n{text}"
    );
    assert!(
        !text.contains("tyve"),
        "the tail of a capped row is gone:\n{text}"
    );
    assert!(
        text.contains("After the") && text.contains("tall one"),
        "tasks after a tall one still render:\n{text}"
    );
    assert!(
        text.contains("mple.invali"),
        "a long word is broken by width:\n{text}"
    );

    let mut wide = Terminal::new(TestBackend::new(90, 12)).unwrap();
    wide.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&wide);
    assert!(
        text.contains("anywhere"),
        "the whole word shows when three rows suffice:\n{text}"
    );
}
#[test]
fn detail_scrolls_and_help_returns_to_it() {
    let theme = Theme::load("phosphor", None).unwrap();
    let notes: Vec<String> = (1..=30).map(|i| format!("note line {i}")).collect();
    let extra = format!(
        "DTSTART;VALUE=DATE:20260830\r\nSTATUS:COMPLETED\r\nCOMPLETED:20260831T090000Z\r\nDESCRIPTION:{}\r\n",
        notes.join("\\n")
    );
    let tasks = vec![task(
        "notes",
        "Notes",
        "p",
        Some("DUE;VALUE=DATE:20260831"),
        &extra,
    )];
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    app.show_done = true;
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();

    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.mode, Mode::Detail);
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(text.contains("Start"), "{text}");
    assert!(text.contains("Completed"), "{text}");
    assert!(!text.contains("note line 20"));

    for _ in 0..3 {
        app.handle_key(key('J'));
    }
    assert_eq!(app.detail_scroll, 15);
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        !text.contains("Summary"),
        "scrolled past the header:\n{text}"
    );
    assert!(text.contains("note line"), "{text}");
    app.handle_key(key('K'));
    assert_eq!(app.detail_scroll, 10);

    app.handle_key(key('?'));
    assert_eq!(app.mode, Mode::Help);
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Detail, "help returns where it came from");
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn long_project_names_are_cut_with_the_count_kept() {
    let theme = Theme::load("phosphor", None).unwrap();
    let tasks = vec![task("t", "t", "house", None, "")];
    let s = make(&[("house", "Indkøb og husholdning i lejligheden")], tasks);
    let app = &s.app;
    let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    let row = text.lines().find(|l| l.contains("Indkøb")).unwrap();
    assert!(row.contains("…"), "{text}");
    assert!(
        row.contains("   1 │"),
        "count stays inside the pane:\n{text}"
    );
}

#[test]
fn small_terminals_render_content_in_every_mode() {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut s = sample();
    let app = &mut s.app;
    app.view_index = 3;
    for (w, h) in [(20, 5), (30, 8), (50, 12), (5, 3)] {
        for mode in [Mode::Normal, Mode::Detail, Mode::Help] {
            app.mode = mode;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
        }
    }
    app.mode = Mode::Normal;
    let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(text.contains("Pay"), "{text}");
}
#[test]
fn alarm_texts_read_naturally() {
    use chrono::TimeDelta;
    use husk::model::{Alarm, Anchor};
    let config = Config::default();
    let rel = |secs: i64, anchor: Anchor| {
        app::alarm_text(
            &Alarm::Relative {
                offset: TimeDelta::seconds(secs),
                anchor,
            },
            &config,
        )
    };
    assert_eq!(rel(0, Anchor::Due), "at due");
    assert_eq!(rel(0, Anchor::Start), "at start");
    assert_eq!(rel(-900, Anchor::Due), "15m before due");
    assert_eq!(rel(-3600, Anchor::Due), "1h before due");
    assert_eq!(rel(86_400, Anchor::Due), "1d after due");
    assert_eq!(rel(-90, Anchor::Due), "90s before due");
    let absolute = app::alarm_text(&Alarm::Absolute(local_utc_at(2026, 9, 3, 9, 0)), &config);
    assert_eq!(absolute, "2026-09-03 09:00");
}

fn local_utc_at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
    Local
        .with_ymd_and_hms(y, m, d, h, mi, 0)
        .single()
        .unwrap()
        .to_utc()
}

#[test]
fn recurring_tasks_show_the_rule_in_words() {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut s = sample();
    let app = &mut s.app;
    app.view_index = 3;
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(text.contains("Oil balcony #home ↻ weekly"), "{text}");

    while app.selected_task().map(|t| t.uid.as_str()) != Some("soon") {
        app.handle_key(key('j'));
    }
    app.handle_key(code(KeyCode::Enter));
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(text.contains("Repeats   ↻ weekly  FREQ=WEEKLY"), "{text}");
}

#[test]
fn subtasks_follow_their_parent_and_are_indented() {
    let theme = Theme::load("phosphor", None).unwrap();
    let tasks = vec![
        task("child", "Small step", "p", None, "RELATED-TO:parent\r\n"),
        task(
            "other",
            "Other thing",
            "p",
            Some("DUE;VALUE=DATE:20260902"),
            "",
        ),
        task(
            "parent",
            "Big thing",
            "p",
            Some("DUE;VALUE=DATE:20260901"),
            "",
        ),
        task(
            "grandchild",
            "Tiny step",
            "p",
            None,
            "RELATED-TO;RELTYPE=PARENT:child\r\n",
        ),
        task("orphan", "Lost step", "p", None, "RELATED-TO:nobody\r\n"),
        task("loop-a", "Loop A", "p", None, "RELATED-TO:loop-b\r\n"),
        task("loop-b", "Loop B", "p", None, "RELATED-TO:loop-a\r\n"),
    ];
    let mut s = make(&[], tasks);
    let app = &mut s.app;
    app.view_index = 3;
    assert_eq!(
        uids(app),
        vec![
            "parent",
            "child",
            "grandchild",
            "other",
            "orphan",
            "loop-a",
            "loop-b"
        ],
        "children follow parents, a cycle falls to the end"
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    let col = |needle: &str| {
        let line = text.lines().find(|l| l.contains(needle)).unwrap();
        line.find(needle).unwrap()
    };
    assert!(text.contains("└ Small step"), "{text}");
    assert!(text.contains("  └ Tiny step"), "{text}");
    assert_eq!(
        col("└ Small step"),
        col("Big thing"),
        "child under the parent's title column"
    );
    assert!(
        !text.contains("└ Lost step"),
        "an orphan renders as a plain row:\n{text}"
    );

    app.filter = "step".to_string();
    assert_eq!(
        uids(app),
        vec!["child", "grandchild", "orphan"],
        "a filtered-out parent leaves its children at sort order"
    );
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    assert!(
        text.contains("└ Tiny step") && !text.contains("└ Small step"),
        "{text}"
    );
}

#[test]
fn projects_wear_the_color_from_their_vdir_color_file() {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut s = sample();
    fs::write(s._dir.path().join("life/color"), "#83D754\n").unwrap();
    fs::write(s._dir.path().join("argot/color"), "not a color\n").unwrap();
    s.app.reload();
    let app = &s.app;
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    let text = screen(&terminal);
    let buffer = terminal.backend().buffer();
    let style_of = |needle: &str| {
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(needle))
            .unwrap();
        let x = line[..line.find(needle).unwrap()].chars().count();
        buffer.cell((x as u16, y as u16)).unwrap().style()
    };
    assert_eq!(style_of("Life").fg, theme.hex_style("#83D754").unwrap().fg);
    assert_eq!(
        style_of("Argot").fg,
        theme.project.fg,
        "an unreadable color falls back"
    );
}
