mod common;

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use husk::config::Config;
use husk::store::VdirStore;
use husk::theme::Theme;
use husk::ui::app::{App, EditorRequest, Mode};
use husk::ui::{self, views};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const TEST_TASK: &str = "BD215969-28EE-4474-A2E4-61575C3C49E7";
const WERKLY: &str = "9ED04B97-EAF7-4BE1-AEFF-48DA2F322B71";

fn now() -> DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 8, 31, 12, 0, 0)
        .single()
        .unwrap()
}

struct Sample {
    dir: common::TempDir,
    app: App,
}

fn sample_with(config: impl FnOnce(&mut Config)) -> Sample {
    let dir = common::fixture_vdir();
    let mut cfg = Config {
        vdir: dir.path().to_path_buf(),
        sync_command: vec![],
        ..Config::default()
    };
    config(&mut cfg);
    let app = App::new(cfg, Box::new(VdirStore::new(dir.path())), now()).unwrap();
    Sample { dir, app }
}

fn sample() -> Sample {
    sample_with(|_| {})
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle_key(key(c));
    }
}

fn clear_input(app: &mut App) {
    for _ in 0..80 {
        app.handle_key(code(KeyCode::Backspace));
    }
}

fn file(s: &Sample, rel: &str) -> String {
    fs::read_to_string(s.dir.path().join(rel)).unwrap()
}

fn find_file(s: &Sample, project: &str, needle: &str) -> Option<String> {
    fs::read_dir(s.dir.path().join(project))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| fs::read_to_string(e.path()).unwrap_or_default())
        .find(|text| text.contains(needle))
}

/// Moves the cursor onto a task in the All view.
fn select(app: &mut App, uid: &str) {
    app.view_index = 3;
    app.handle_key(key('g'));
    for _ in 0..50 {
        if app.selected_task().is_some_and(|t| t.uid == uid) {
            return;
        }
        app.handle_key(key('j'));
    }
    panic!("no visible task {uid}");
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

fn render(app: &App) -> String {
    let theme = Theme::load("phosphor", None).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|f| views::draw(f, app, &theme)).unwrap();
    screen(&terminal)
}

#[test]
fn d_completes_with_the_apple_triple_and_u_reopens() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('d'));
    let text = file(&s, "life/no-due.ics");
    for line in [
        "STATUS:COMPLETED\n",
        "PERCENT-COMPLETE:100\n",
        "COMPLETED:20260831T",
        "SEQUENCE:1\n",
    ] {
        assert!(text.contains(line), "{line:?} missing in\n{text}");
    }
    assert_eq!(s.app.message.as_deref(), Some("Done: Test task (u undo)"));
    assert!(
        !s.app.visible_tasks().iter().any(|t| t.uid == TEST_TASK),
        "done tasks leave the list"
    );

    s.app.handle_key(key('u'));
    let text = file(&s, "life/no-due.ics");
    assert!(!text.contains("COMPLETED"), "{text}");
    assert!(
        text.contains("SEQUENCE:2\n"),
        "restore outranks the completed file:\n{text}"
    );
    assert_eq!(s.app.message.as_deref(), Some("Restored: Test task"));
    assert_eq!(s.app.selected_task().unwrap().uid, TEST_TASK);

    s.app.handle_key(key('d'));
    s.app.handle_key(key('c'));
    select(&mut s.app, TEST_TASK);
    assert!(s.app.selected_task().unwrap().is_done());
    s.app.handle_key(key('d'));
    assert_eq!(
        s.app.message.as_deref(),
        Some("Reopened: Test task (u undo)")
    );
    let text = file(&s, "life/no-due.ics");
    assert!(
        text.contains("STATUS:NEEDS-ACTION\n") && !text.contains("PERCENT"),
        "{text}"
    );
}

#[test]
fn d_refuses_recurring_tasks() {
    let mut s = sample();
    let before = file(&s, "life/recurring-weekly-until.ics");
    select(&mut s.app, WERKLY);
    s.app.handle_key(key('d'));
    assert!(
        s.app.message.as_deref().unwrap().contains("phone"),
        "{:?}",
        s.app.message
    );
    assert_eq!(file(&s, "life/recurring-weekly-until.ics"), before);
}

#[test]
fn x_confirms_deletes_and_u_restores() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('x'));
    assert_eq!(s.app.mode, Mode::Confirm);
    assert!(render(&s.app).contains("Delete \"Test task\"?"));
    s.app.handle_key(key('n'));
    assert_eq!(s.app.mode, Mode::Normal);
    assert!(s.dir.path().join("life/no-due.ics").exists());

    s.app.handle_key(key('x'));
    s.app.handle_key(key('y'));
    assert!(!s.dir.path().join("life/no-due.ics").exists());
    assert_eq!(
        s.app.message.as_deref(),
        Some("Deleted: Test task (u undo)")
    );
    assert!(!s.app.tasks.iter().any(|t| t.uid == TEST_TASK));

    s.app.handle_key(key('u'));
    let restored = s.dir.path().join(format!("life/{TEST_TASK}.ics"));
    assert!(restored.exists());
    assert!(
        fs::read_to_string(restored)
            .unwrap()
            .contains("SUMMARY:Test task\n")
    );
    assert_eq!(s.app.selected_task().unwrap().uid, TEST_TASK);

    s.app.handle_key(key('u'));
    s.app.handle_key(key('u'));
    assert_eq!(s.app.message.as_deref(), Some("Nothing to undo"));
}

#[test]
fn a_adds_in_the_current_project_with_default_alarms_and_u_removes_it() {
    let mut s = sample();
    s.app.view_index = 5;
    assert_eq!(s.app.view_name(&s.app.view()), "Life");
    s.app.handle_key(key('a'));
    assert_eq!(s.app.mode, Mode::Input);
    let empty = render(&s.app);
    assert!(
        empty.contains("add> ▌  title due:tomorrow"),
        "placeholder while empty:\n{empty}"
    );
    type_text(&mut s.app, "Call bank due:tomorrow 09:00 +money pri:h");
    let typing = render(&s.app);
    assert!(
        typing.contains("add> Call bank due:tomorrow 09:00 +money pri:h▌"),
        "{typing}"
    );
    assert!(
        !typing.contains("title due:tomorrow"),
        "placeholder gone once typing starts:\n{typing}"
    );
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.message.as_deref(), Some("Added: Call bank (u undo)"));
    let text = find_file(&s, "life", "SUMMARY:Call bank").expect("created in life");
    let due = Local
        .with_ymd_and_hms(2026, 9, 1, 9, 0, 0)
        .single()
        .unwrap()
        .to_utc()
        .format("DUE:%Y%m%dT%H%M%SZ")
        .to_string();
    for line in [
        due.as_str(),
        "CATEGORIES:money",
        "PRIORITY:1",
        "TRIGGER;RELATED=END:-P1D",
        "TRIGGER;RELATED=END:-PT1H",
        "TRIGGER;RELATED=END:PT0S",
    ] {
        assert!(text.contains(line), "{line} missing in\n{text}");
    }
    assert_eq!(text.matches("BEGIN:VALARM").count(), 3);
    assert_eq!(s.app.selected_task().unwrap().summary, "Call bank");

    s.app.handle_key(key('u'));
    assert!(find_file(&s, "life", "SUMMARY:Call bank").is_none());
    assert_eq!(s.app.message.as_deref(), Some("Removed the added task"));
}

#[test]
fn a_needs_a_project_and_honours_overrides() {
    let mut s = sample();
    s.app.view_index = 1;
    s.app.handle_key(key('a'));
    type_text(&mut s.app, "No home");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(
        s.app
            .message
            .as_deref()
            .unwrap()
            .contains("default_project"),
        "{:?}",
        s.app.message
    );
    assert!(find_file(&s, "life", "No home").is_none());
    assert_eq!(s.app.mode, Mode::Input, "a failed prompt stays open");
    let screen = render(&s.app);
    assert!(
        screen.contains("default_project"),
        "the reason is visible while the prompt is open:\n{screen}"
    );
    assert!(
        screen
            .lines()
            .last()
            .unwrap()
            .trim_end()
            .ends_with("default_project"),
        "at the right end of the bar, not under the cursor:\n{screen}"
    );
    s.app.handle_key(code(KeyCode::Esc));

    s.app.handle_key(key('a'));
    type_text(&mut s.app, "With override @argot");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(find_file(&s, "argot", "SUMMARY:With override").is_some());
    let text = find_file(&s, "argot", "SUMMARY:With override").unwrap();
    assert!(!text.contains("VALARM"), "no due, no alarm:\n{text}");

    s.app.handle_key(key('a'));
    type_text(&mut s.app, "Nowhere @nosuch");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(s.app.message.as_deref().unwrap().contains("nosuch"));
    s.app.handle_key(code(KeyCode::Esc));

    s.app.handle_key(key('a'));
    type_text(&mut s.app, "   ");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(s.app.message.as_deref().unwrap().contains("title"));
    s.app.handle_key(code(KeyCode::Esc));

    let mut d = sample_with(|c| c.default_project = Some("Life".to_string()));
    d.app.handle_key(key('a'));
    type_text(&mut d.app, "By default");
    d.app.handle_key(code(KeyCode::Enter));
    assert!(
        find_file(&d, "life", "SUMMARY:By default").is_some(),
        "display name resolves"
    );
}

#[test]
fn e_t_p_and_upper_t_edit_the_selected_task() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);

    s.app.handle_key(key('e'));
    assert_eq!(s.app.input.as_ref().unwrap().buffer, "Test task");
    for _ in 0..4 {
        s.app.handle_key(code(KeyCode::Backspace));
    }
    type_text(&mut s.app, "thing");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(file(&s, "life/no-due.ics").contains("SUMMARY:Test thing\n"));
    assert_eq!(
        s.app.selected_task().unwrap().uid,
        TEST_TASK,
        "stays selected"
    );

    s.app.handle_key(key('t'));
    assert_eq!(s.app.input.as_ref().unwrap().buffer, "");
    assert!(render(&s.app).contains("due> "));
    type_text(&mut s.app, "fri 10:00");
    s.app.handle_key(code(KeyCode::Enter));
    let text = file(&s, "life/no-due.ics");
    let due = Local
        .with_ymd_and_hms(2026, 9, 4, 10, 0, 0)
        .single()
        .unwrap()
        .to_utc()
        .format("DUE:%Y%m%dT%H%M%SZ\n")
        .to_string();
    assert!(text.contains(&due), "{text}");
    assert_eq!(
        text.matches("BEGIN:VALARM").count(),
        3,
        "alarms added with a timed due:\n{text}"
    );

    s.app.handle_key(key('t'));
    assert_eq!(s.app.input.as_ref().unwrap().buffer, "2026-09-04 10:00");
    s.app.handle_key(code(KeyCode::Esc));
    assert_eq!(s.app.mode, Mode::Normal);
    assert!(
        file(&s, "life/no-due.ics").contains(&due),
        "Esc changes nothing"
    );

    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    type_text(&mut s.app, "someday");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(
        s.app
            .message
            .as_deref()
            .unwrap()
            .contains("could not read a date")
    );

    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    s.app.handle_key(code(KeyCode::Enter));
    assert!(
        !file(&s, "life/no-due.ics").contains("\nDUE"),
        "empty clears the due date"
    );

    for expected in ["PRIORITY:1\n", "PRIORITY:5\n", "PRIORITY:9\n"] {
        s.app.handle_key(key('p'));
        assert!(file(&s, "life/no-due.ics").contains(expected), "{expected}");
    }
    s.app.handle_key(key('p'));
    assert!(!file(&s, "life/no-due.ics").contains("PRIORITY"));

    s.app.handle_key(key('T'));
    type_text(&mut s.app, "home, #garden ,home");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(file(&s, "life/no-due.ics").contains("CATEGORIES:home,garden\n"));
    s.app.handle_key(key('T'));
    assert_eq!(s.app.input.as_ref().unwrap().buffer, "home, garden");
    clear_input(&mut s.app);
    s.app.handle_key(code(KeyCode::Enter));
    assert!(!file(&s, "life/no-due.ics").contains("CATEGORIES"));
}

#[test]
fn m_moves_through_the_picker_and_u_moves_back() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('m'));
    assert_eq!(s.app.mode, Mode::Pick);
    assert_eq!(s.app.pick_index, 1, "starts on the task's own project");
    assert!(render(&s.app).contains("Move to"));
    s.app.handle_key(key('k'));
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.message.as_deref(), Some("Moved to Argot (u undo)"));
    assert!(s.dir.path().join("argot/no-due.ics").exists());
    assert!(!s.dir.path().join("life/no-due.ics").exists());
    assert_eq!(s.app.selected_task().unwrap().project.as_str(), "argot");

    s.app.handle_key(key('u'));
    assert!(!s.dir.path().join("argot/no-due.ics").exists());
    assert!(
        s.dir.path().join("life/no-due.ics").exists(),
        "back under its own name"
    );
    assert_eq!(s.app.selected_task().unwrap().project.as_str(), "life");
}

#[test]
fn notes_go_through_the_editor_request_and_apply_notes() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    assert_eq!(s.app.take_editor_request(), None);
    s.app.handle_key(key('n'));
    assert_eq!(
        s.app.take_editor_request(),
        Some(EditorRequest {
            uid: TEST_TASK.to_string(),
            text: String::new(),
            raw: false
        })
    );
    assert_eq!(s.app.take_editor_request(), None, "taken once");

    s.app.apply_notes(TEST_TASK, "line one\nline two\n\n");
    assert_eq!(
        s.app.message.as_deref(),
        Some("Notes saved: Test task (u undo)")
    );
    assert!(file(&s, "life/no-due.ics").contains("DESCRIPTION:line one\\nline two\n"));
    s.app.handle_key(key('n'));
    assert_eq!(
        s.app.take_editor_request().unwrap().text,
        "line one\nline two",
        "the editor starts from the current notes"
    );
    s.app.apply_notes(TEST_TASK, "");
    assert!(!file(&s, "life/no-due.ics").contains("DESCRIPTION"));
}

#[test]
fn edit_with_runs_the_editor_on_a_temp_file() {
    let dir = common::TempDir::new();
    let script = dir.path().join("editor.sh");
    fs::write(
        &script,
        "#!/bin/sh\nfor last; do :; done\nprintf 'from script\\n' >> \"$last\"\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&script, perms).unwrap();
    let editor = format!("{} --flag", script.display());
    assert_eq!(
        ui::edit_with(
            &editor, "start
", "md"
        )
        .unwrap(),
        "start\nfrom script\n"
    );

    let failing = dir.path().join("fail.sh");
    fs::write(&failing, "#!/bin/sh\nexit 2\n").unwrap();
    let mut perms = fs::metadata(&failing).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&failing, perms).unwrap();
    let err = ui::edit_with(&failing.display().to_string(), "x", "ics").unwrap_err();
    assert!(err.to_string().contains("exited"), "{err}");
}

#[test]
fn writes_trigger_a_sync_and_a_finished_run_reloads() {
    let mut s = sample_with(|c| {
        c.sync_command = vec!["sh".to_string(), "-c".to_string(), "true".to_string()];
    });
    assert!(s.app.sync_enabled());
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('d'));
    let start = Instant::now();
    while s.app.sync_state().runs == 0 && start.elapsed() < Duration::from_secs(8) {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(s.app.sync_state().runs, 1, "a write asked for a sync");

    let path = s.dir.path().join("life/all-day.ics");
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("SUMMARY:Have fun", "SUMMARY:Edited on the phone");
    fs::write(&path, edited).unwrap();
    s.app.poll();
    assert!(
        s.app
            .tasks
            .iter()
            .any(|t| t.summary == "Edited on the phone"),
        "reloaded after the run"
    );
    assert!(render(&s.app).contains("synced "), "{}", render(&s.app));

    s.app.handle_key(key('s'));
    assert_eq!(s.app.message.as_deref(), Some("Sync requested"));

    let mut off = sample();
    off.app.handle_key(key('s'));
    assert!(off.app.message.as_deref().unwrap().contains("off"));
}

#[test]
fn actions_work_from_the_detail_view_and_return_to_it() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.mode, Mode::Detail);
    s.app.handle_key(key('e'));
    assert_eq!(s.app.mode, Mode::Input);
    assert!(
        render(&s.app).contains("Summary"),
        "detail stays behind the prompt"
    );
    type_text(&mut s.app, "!");
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.mode, Mode::Detail);
    assert!(file(&s, "life/no-due.ics").contains("SUMMARY:Test task!\n"));
    s.app.handle_key(key('x'));
    s.app.handle_key(code(KeyCode::Esc));
    assert_eq!(s.app.mode, Mode::Detail);
}

#[test]
fn a_phone_edit_synced_in_between_is_never_overwritten() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    let path = s.dir.path().join("life/no-due.ics");
    let phone = fs::read_to_string(&path)
        .unwrap()
        .replace("SUMMARY:Test task", "SUMMARY:Renamed on the phone");
    fs::write(&path, &phone).unwrap();

    s.app.handle_key(key('e'));
    assert_eq!(
        s.app.input.as_ref().unwrap().buffer,
        "Test task",
        "the prompt shows what was loaded"
    );
    type_text(&mut s.app, "!");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(
        s.app
            .message
            .as_deref()
            .unwrap()
            .contains("changed on disk"),
        "{:?}",
        s.app.message
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        phone,
        "the phone's rename survives"
    );
    assert!(
        s.app
            .tasks
            .iter()
            .any(|t| t.summary == "Renamed on the phone"),
        "reloaded"
    );
    s.app.handle_key(code(KeyCode::Esc));

    let recurring = phone.replace(
        "SUMMARY:Renamed on the phone",
        "RRULE:FREQ=WEEKLY\nSUMMARY:Now weekly",
    );
    fs::write(&path, &recurring).unwrap();
    s.app.handle_key(key('d'));
    assert!(
        !fs::read_to_string(&path).unwrap().contains("COMPLETED"),
        "no completion on a task the phone made recurring"
    );
    s.app.handle_key(key('d'));
    assert!(
        s.app.message.as_deref().unwrap().contains("phone"),
        "after the reload the refusal applies: {:?}",
        s.app.message
    );
}

#[test]
fn external_changes_are_picked_up_by_polling() {
    let mut s = sample();
    let path = s.dir.path().join("life/all-day.ics");
    let tmp = s.dir.path().join("life/all-day.ics.tmp");
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("SUMMARY:Have fun", "SUMMARY:Have more fun");
    thread::sleep(Duration::from_millis(20));
    fs::write(&tmp, edited).unwrap();
    fs::rename(&tmp, &path).unwrap();
    s.app.poll();
    assert!(
        s.app.tasks.iter().any(|t| t.summary == "Have more fun"),
        "a rename into the vdir reloads"
    );
    fs::remove_file(&path).unwrap();
    s.app.poll();
    assert!(
        !s.app.tasks.iter().any(|t| t.summary == "Have more fun"),
        "a removed file reloads"
    );
}

#[test]
fn unchanged_prompts_leave_no_trace() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    let before = file(&s, "life/no-due.ics");
    for k in ['e', 'T', 't'] {
        s.app.handle_key(key(k));
        s.app.handle_key(code(KeyCode::Enter));
        assert_eq!(s.app.mode, Mode::Normal);
        assert_eq!(s.app.message, None, "{k}");
    }
    assert_eq!(file(&s, "life/no-due.ics"), before);
    s.app.handle_key(key('u'));
    assert_eq!(s.app.message.as_deref(), Some("Nothing to undo"));

    // Tasks.org writes seconds; opening and closing the due prompt keeps them.
    select(&mut s.app, "1315299692917196932");
    let before = file(&s, "argot/timed-alarms-repeat.ics");
    s.app.handle_key(key('t'));
    // 13:00:01 in Europe/Copenhagen, shown in whatever zone the test runs in.
    let expected = chrono::Utc
        .with_ymd_and_hms(2026, 8, 31, 11, 0, 1)
        .unwrap()
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert_eq!(s.app.input.as_ref().unwrap().buffer, expected);
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(file(&s, "argot/timed-alarms-repeat.ics"), before);
    s.app.apply_notes("1315299692917196932", "");
    assert_eq!(
        file(&s, "argot/timed-alarms-repeat.ics"),
        before,
        "no notes to no notes"
    );
}

#[test]
fn tags_with_spaces_survive_the_prompt() {
    let mut s = sample();
    let path = s.dir.path().join("life/no-due.ics");
    let tagged = fs::read_to_string(&path).unwrap().replace(
        "SUMMARY:Test task",
        "CATEGORIES:Home improvement,urgent\nSUMMARY:Test task",
    );
    fs::write(&path, tagged).unwrap();
    s.app.reload();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('T'));
    assert_eq!(
        s.app.input.as_ref().unwrap().buffer,
        "Home improvement, urgent"
    );
    type_text(&mut s.app, ", new one");
    s.app.handle_key(code(KeyCode::Enter));
    assert!(file(&s, "life/no-due.ics").contains("CATEGORIES:Home improvement,urgent,new one\n"));
}

#[test]
fn due_prefill_ignores_the_display_time_format() {
    let mut s = sample_with(|c| c.time_format = "%I:%M %p".to_string());
    select(&mut s.app, "18137A41-46DD-4CC3-AD7B-B606DB130741");
    let before = file(&s, "life/timed-alarm.ics");
    s.app.handle_key(key('t'));
    let buffer = s.app.input.as_ref().unwrap().buffer.clone();
    assert!(
        buffer.starts_with("2026-08-31 ") && !buffer.contains('M'),
        "{buffer}"
    );
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(file(&s, "life/timed-alarm.ics"), before);
}

#[test]
fn clearing_a_due_date_drops_relative_alarms_but_keeps_absolute_ones() {
    let mut s = sample();
    select(&mut s.app, "AF400CA3-DCAA-4DC1-ACDF-0F5E6E981C1D");
    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    type_text(&mut s.app, "fri 10:00");
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(
        file(&s, "life/all-day.ics").matches("BEGIN:VALARM").count(),
        3
    );
    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    s.app.handle_key(code(KeyCode::Enter));
    let text = file(&s, "life/all-day.ics");
    assert!(
        !text.contains("VALARM") && !text.contains("\nDUE"),
        "{text}"
    );

    select(&mut s.app, "18137A41-46DD-4CC3-AD7B-B606DB130741");
    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    s.app.handle_key(code(KeyCode::Enter));
    let text = file(&s, "life/timed-alarm.ics");
    assert!(
        text.contains("TRIGGER;VALUE=DATE-TIME:20260831T102500Z"),
        "absolute alarm kept:\n{text}"
    );

    select(&mut s.app, "1315299692917196932");
    s.app.handle_key(key('t'));
    clear_input(&mut s.app);
    type_text(&mut s.app, "2026-09-10 12:00");
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(
        file(&s, "argot/timed-alarms-repeat.ics")
            .matches("BEGIN:VALARM")
            .count(),
        3,
        "existing alarms are kept"
    );
}

#[test]
fn the_picker_moves_the_task_m_was_pressed_on() {
    let mut s = sample();
    let argot_before = fs::read_dir(s.dir.path().join("argot")).unwrap().count();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('m'));
    fs::remove_file(s.dir.path().join("life/no-due.ics")).unwrap();
    s.app.reload();
    s.app.handle_key(key('k'));
    s.app.handle_key(code(KeyCode::Enter));
    assert!(
        s.app.message.as_deref().unwrap().contains(TEST_TASK),
        "{:?}",
        s.app.message
    );
    assert_eq!(
        fs::read_dir(s.dir.path().join("argot")).unwrap().count(),
        argot_before,
        "nothing else moved"
    );
}

#[test]
fn deleting_from_the_detail_returns_to_the_list() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(code(KeyCode::Enter));
    s.app.handle_key(key('x'));
    s.app.handle_key(key('y'));
    assert_eq!(s.app.mode, Mode::Normal);
}

#[test]
fn undoing_a_move_keeps_the_task_when_the_way_back_is_gone() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('m'));
    s.app.handle_key(key('k'));
    s.app.handle_key(code(KeyCode::Enter));
    assert!(s.dir.path().join("argot/no-due.ics").exists());
    fs::remove_dir_all(s.dir.path().join("life")).unwrap();
    s.app.handle_key(key('u'));
    assert!(
        s.app.message.as_deref().unwrap().contains("life"),
        "{:?}",
        s.app.message
    );
    assert!(
        s.dir.path().join("argot/no-due.ics").exists(),
        "a failed undo loses nothing"
    );
}

#[test]
fn a_failed_prompt_keeps_its_text() {
    let mut s = sample();
    s.app.view_index = 5;
    s.app.handle_key(key('a'));
    type_text(&mut s.app, "reply to @jacob about lunch");
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.mode, Mode::Input);
    assert_eq!(
        s.app.input.as_ref().unwrap().buffer,
        "reply to @jacob about lunch"
    );
    assert!(s.app.message.as_deref().unwrap().contains("jacob"));
    for _ in 0..18 {
        s.app.handle_key(code(KeyCode::Backspace));
    }
    type_text(&mut s.app, "about lunch");
    s.app.handle_key(code(KeyCode::Enter));
    assert_eq!(s.app.mode, Mode::Normal);
    assert_eq!(
        s.app.message.as_deref(),
        Some("Added: reply to about lunch (u undo)")
    );
}

#[test]
fn messages_fade_after_five_seconds_or_on_a_key() {
    use chrono::TimeDelta;
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('d'));
    assert!(s.app.message.is_some());
    s.app.now += TimeDelta::seconds(4);
    s.app.poll();
    assert!(s.app.message.is_some(), "still shown after four seconds");
    s.app.now += TimeDelta::seconds(2);
    s.app.poll();
    assert_eq!(s.app.message, None, "gone after five");

    s.app.handle_key(key('u'));
    assert!(s.app.message.is_some());
    s.app.handle_key(key('j'));
    assert_eq!(s.app.message, None, "any key clears it");
}

#[test]
fn o_edits_the_raw_file_through_the_store() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('o'));
    let request = s.app.take_editor_request().unwrap();
    assert!(request.raw);
    assert_eq!(request.text, file(&s, "life/no-due.ics"));

    let edited = request
        .text
        .replace("SUMMARY:Test task", "SUMMARY:Hand edited\nX-HUSK-NOTE:kept");
    s.app.apply_raw(TEST_TASK, &edited);
    assert_eq!(
        s.app.message.as_deref(),
        Some("File saved: Hand edited (u undo)")
    );
    let text = file(&s, "life/no-due.ics");
    assert!(text.contains("SUMMARY:Hand edited\n"), "{text}");
    assert!(
        text.contains("X-HUSK-NOTE:kept\n"),
        "unknown properties pass through:\n{text}"
    );
    assert!(
        text.contains("SEQUENCE:1\n"),
        "a raw edit is still a rewrite:\n{text}"
    );

    let before = text.clone();
    s.app
        .apply_raw(TEST_TASK, &request.text.replace(TEST_TASK, "someone-else"));
    assert!(
        s.app.message.as_deref().unwrap().contains("UID must stay"),
        "{:?}",
        s.app.message
    );
    s.app.apply_raw(TEST_TASK, "BEGIN:VCALENDAR\nnot really\n");
    assert!(s.app.message.is_some());
    assert_eq!(
        file(&s, "life/no-due.ics"),
        before,
        "refused edits change nothing"
    );

    s.app.apply_raw(TEST_TASK, &before);
    assert_eq!(s.app.message, None, "an unchanged file is a no-op");
    assert_eq!(file(&s, "life/no-due.ics"), before);

    s.app.handle_key(key('u'));
    assert!(file(&s, "life/no-due.ics").contains("SUMMARY:Test task\n"));
}

#[test]
fn o_refuses_a_file_that_changed_on_disk_or_holds_more_than_one_task() {
    let mut s = sample();
    select(&mut s.app, TEST_TASK);
    s.app.handle_key(key('o'));
    let request = s.app.take_editor_request().unwrap();
    let path = s.dir.path().join("life/no-due.ics");
    let phone = fs::read_to_string(&path)
        .unwrap()
        .replace("SUMMARY:Test task", "SUMMARY:Phone rename\nSEQUENCE:3");
    fs::write(&path, &phone).unwrap();
    s.app.apply_raw(
        TEST_TASK,
        &request
            .text
            .replace("SUMMARY:Test task", "SUMMARY:Laptop edit"),
    );
    assert!(
        s.app
            .message
            .as_deref()
            .unwrap()
            .contains("changed on disk"),
        "{:?}",
        s.app.message
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        phone,
        "the phone's version survives"
    );
    assert!(
        s.app.tasks.iter().any(|t| t.summary == "Phone rename"),
        "reloaded"
    );

    s.app.handle_key(key('o'));
    let request = s.app.take_editor_request().unwrap();
    let two = request.text.replace(
        "END:VCALENDAR",
        "BEGIN:VTODO\nUID:other\nSUMMARY:Smuggled\nEND:VTODO\nEND:VCALENDAR",
    );
    s.app.apply_raw(TEST_TASK, &two);
    assert!(
        s.app
            .message
            .as_deref()
            .unwrap()
            .contains("exactly one VTODO"),
        "{:?}",
        s.app.message
    );
    assert!(!fs::read_to_string(&path).unwrap().contains("Smuggled"));

    let crlf = request.text.replace('\n', "\r\n");
    s.app.apply_raw(TEST_TASK, &crlf);
    assert_eq!(s.app.message, None, "line endings alone are not an edit");
    assert_eq!(fs::read_to_string(&path).unwrap(), phone);
}

#[test]
fn messages_set_from_outside_a_key_press_still_fade_correctly() {
    use chrono::TimeDelta;
    let mut s = sample();
    s.app.now += TimeDelta::seconds(30);
    s.app.notify("theme: broken");
    s.app.poll();
    assert_eq!(
        s.app.message.as_deref(),
        Some("theme: broken"),
        "fresh messages stay"
    );
    s.app.now += TimeDelta::seconds(6);
    s.app.poll();
    assert_eq!(s.app.message, None);
}
