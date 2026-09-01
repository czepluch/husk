mod common;

use std::fs;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use husk::config::Config;
use husk::ical::vtodo;
use husk::model::{Project, ProjectId, Task};
use husk::notify::{Fired, Notice, State, plan, run_with};

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

fn load(name: &str) -> Task {
    vtodo::parse_task(&common::fixture(name), ProjectId::new("life")).unwrap()
}

fn projects() -> Vec<Project> {
    vec![Project {
        id: ProjectId::new("life"),
        name: "Life".to_string(),
        color: None,
    }]
}

/// An absolute alarm at 2026-08-31 10:25Z; a timed task without alarms due
/// 2026-09-01 08:00Z (so the default leads fire at 08-31 08:00Z, 09-01
/// 07:00Z and 08:00Z); a completed task; an all-day task without alarms.
fn tasks() -> Vec<Task> {
    vec![
        load("apple/timed-alarm.ics"),
        load("todoman/timed-utc.ics"),
        load("apple/completed.ics"),
        load("apple/all-day.ics"),
    ]
}

fn titles(notices: &[Notice]) -> Vec<&str> {
    notices.iter().map(|n| n.title.as_str()).collect()
}

fn state_at(last_run: DateTime<Utc>) -> State {
    State {
        last_run: Some(last_run),
        fired: vec![],
    }
}

#[test]
fn the_first_run_covers_only_the_grace_window_and_records_the_time() {
    let now = utc(2026, 8, 31, 12, 0, 0);
    let (notices, state) = plan(
        &tasks(),
        &projects(),
        &State::default(),
        now,
        &Config::default(),
        false,
    );
    assert!(
        notices.is_empty(),
        "the 10:25 alarm is older than the window"
    );
    assert_eq!(state.last_run, Some(now));
    assert!(state.fired.is_empty());

    let soon_after = utc(2026, 8, 31, 10, 30, 0);
    let (notices, _) = plan(
        &tasks(),
        &projects(),
        &State::default(),
        soon_after,
        &Config::default(),
        false,
    );
    assert_eq!(
        titles(&notices),
        vec!["Remember the milk"],
        "five minutes old fires on a first run"
    );
}

#[test]
fn alarms_between_the_last_run_and_now_fire_once() {
    let config = Config::default();
    let now = utc(2026, 8, 31, 10, 30, 0);
    let (notices, state) = plan(
        &tasks(),
        &projects(),
        &state_at(utc(2026, 8, 31, 10, 20, 0)),
        now,
        &config,
        false,
    );
    assert_eq!(titles(&notices), vec!["Remember the milk"]);
    assert!(
        notices[0].body.starts_with("Due 2026-08-31 "),
        "{}",
        notices[0].body
    );
    assert!(notices[0].body.ends_with(" · Life"), "{}", notices[0].body);
    assert!(notices[0].urgent, "the alarm is at the due time");
    assert!(notices[0].remember);
    assert_eq!(
        state.fired,
        vec![Fired {
            uid: "18137A41-46DD-4CC3-AD7B-B606DB130741".to_string(),
            at: utc(2026, 8, 31, 10, 25, 0),
        }]
    );
    assert_eq!(state.last_run, Some(now));

    let later = now + TimeDelta::minutes(1);
    let (again, _) = plan(&tasks(), &projects(), &state, later, &config, false);
    assert!(again.is_empty(), "a second run fires nothing");
}

#[test]
fn missed_runs_fire_everything_since_the_last_run_once() {
    let config = Config::default();
    let now = utc(2026, 9, 1, 9, 0, 0);
    let (notices, state) = plan(
        &tasks(),
        &projects(),
        &state_at(utc(2026, 8, 30, 12, 0, 0)),
        now,
        &config,
        false,
    );
    assert_eq!(
        titles(&notices),
        vec![
            "From laptop via todoman",
            "Remember the milk",
            "From laptop via todoman",
            "From laptop via todoman",
        ],
        "sorted by fire time"
    );
    assert_eq!(
        notices.iter().map(|n| n.urgent).collect::<Vec<_>>(),
        vec![false, true, false, true],
        "leads before the due time are not urgent"
    );
    assert_eq!(state.fired.len(), 4);
    let (again, _) = plan(&tasks(), &projects(), &state, now, &config, false);
    assert!(again.is_empty());
}

#[test]
fn fired_entries_older_than_thirty_days_are_forgotten() {
    let now = utc(2026, 9, 1, 9, 0, 0);
    let old = Fired {
        uid: "old".to_string(),
        at: now - TimeDelta::days(31),
    };
    let recent = Fired {
        uid: "recent".to_string(),
        at: now - TimeDelta::days(29),
    };
    let state = State {
        last_run: Some(now - TimeDelta::minutes(1)),
        fired: vec![old, recent.clone()],
    };
    let (_, next) = plan(
        &tasks(),
        &projects(),
        &state,
        now,
        &Config::default(),
        false,
    );
    assert_eq!(next.fired, vec![recent]);
}

#[test]
fn nag_repeats_every_overdue_pending_task_without_remembering() {
    let config = Config::default();
    let now = utc(2026, 9, 2, 12, 0, 0);
    let state = state_at(now - TimeDelta::minutes(1));
    let (quiet, _) = plan(&tasks(), &projects(), &state, now, &config, false);
    assert!(quiet.is_empty());

    let (notices, next) = plan(&tasks(), &projects(), &state, now, &config, true);
    let mut names = titles(&notices);
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "Overdue: From laptop via todoman",
            "Overdue: Have fun",
            "Overdue: Remember the milk",
        ],
        "the completed task is never nagged about"
    );
    assert!(notices.iter().all(|n| n.urgent && !n.remember));
    assert!(next.fired.is_empty(), "nags are not remembered");

    let (again, _) = plan(
        &tasks(),
        &projects(),
        &next,
        now + TimeDelta::minutes(1),
        &config,
        true,
    );
    assert_eq!(again.len(), 3, "and so repeat on the next nag");
}

#[test]
fn done_tasks_never_fire_even_with_alarms_in_the_window() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:done\r\nSTATUS:COMPLETED\r\nDUE:20260831T100000Z\r\nSUMMARY:Finished\r\nBEGIN:VALARM\r\nTRIGGER;VALUE=DATE-TIME:20260831T100000Z\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let task = vtodo::parse_task(text, ProjectId::new("life")).unwrap();
    let (notices, _) = plan(
        &[task],
        &projects(),
        &state_at(utc(2026, 8, 31, 9, 0, 0)),
        utc(2026, 8, 31, 11, 0, 0),
        &Config::default(),
        true,
    );
    assert!(notices.is_empty());
}

#[test]
fn the_state_file_round_trips_and_a_missing_one_is_empty() {
    let dir = common::TempDir::new();
    let path = dir.path().join("state").join("notify.json");
    assert_eq!(State::load(&path).unwrap(), State::default());

    let state = State {
        last_run: Some(utc(2026, 9, 1, 9, 0, 0)),
        fired: vec![Fired {
            uid: "a".to_string(),
            at: utc(2026, 9, 1, 8, 0, 0),
        }],
    };
    state.save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), state);
    assert!(fs::read_to_string(&path).unwrap().contains("\"last_run\""));

    fs::write(&path, "{ not json").unwrap();
    assert_eq!(
        State::load(&path).unwrap(),
        State::default(),
        "a broken file starts fresh"
    );
    assert!(!path.exists());
    assert!(
        path.with_extension("json.corrupt").exists(),
        "and is kept aside"
    );
}

#[test]
fn the_window_is_open_after_the_last_run_and_closed_at_it() {
    let config = Config::default();
    let at = utc(2026, 8, 31, 10, 25, 0);
    let (notices, _) = plan(
        &tasks(),
        &projects(),
        &state_at(at),
        at + TimeDelta::minutes(30),
        &config,
        false,
    );
    assert!(
        !notices.iter().any(|n| n.at == at),
        "an alarm exactly at last_run was the previous run's, unless the grace window applies"
    );
    let (notices, _) = plan(
        &tasks(),
        &projects(),
        &state_at(at - TimeDelta::hours(1)),
        at,
        &config,
        false,
    );
    assert!(
        notices.iter().any(|n| n.at == at),
        "an alarm exactly at now fires"
    );
}

#[test]
fn an_alarm_that_arrived_late_still_fires_within_the_grace_window() {
    let config = Config::default();
    // The previous run was a minute ago, but the alarm at 10:25Z reached
    // the vdir only now, ten minutes after its time.
    let now = utc(2026, 8, 31, 10, 35, 0);
    let (notices, state) = plan(
        &tasks(),
        &projects(),
        &state_at(now - TimeDelta::minutes(1)),
        now,
        &config,
        false,
    );
    assert_eq!(titles(&notices), vec!["Remember the milk"]);
    let (again, _) = plan(
        &tasks(),
        &projects(),
        &state,
        now + TimeDelta::minutes(1),
        &config,
        false,
    );
    assert!(
        again.is_empty(),
        "the fired set keeps the grace window from repeating it"
    );
    let late = utc(2026, 8, 31, 10, 41, 0);
    let (nothing, _) = plan(
        &tasks(),
        &projects(),
        &state_at(late - TimeDelta::minutes(1)),
        late,
        &config,
        false,
    );
    assert!(
        nothing.is_empty(),
        "sixteen minutes late is outside the window"
    );
}

#[test]
fn a_last_run_in_the_future_fires_nothing_and_corrects_itself() {
    let now = utc(2026, 8, 31, 12, 0, 0);
    let (notices, state) = plan(
        &tasks(),
        &projects(),
        &state_at(now + TimeDelta::days(1)),
        now,
        &Config::default(),
        false,
    );
    assert!(notices.is_empty());
    assert_eq!(state.last_run, Some(now));
}

#[test]
fn a_failed_delivery_is_retried_on_the_next_run() {
    let dir = common::TempDir::new();
    let path = dir.path().join("notify.json");
    let config = Config::default();
    state_at(utc(2026, 8, 31, 10, 20, 0)).save(&path).unwrap();
    let now = utc(2026, 8, 31, 10, 30, 0);

    let failing = |_: &Notice| anyhow::bail!("no daemon");
    let err = run_with(
        &tasks(),
        &projects(),
        &path,
        now,
        &config,
        false,
        false,
        failing,
    )
    .unwrap_err();
    assert!(err.to_string().contains("1 of 1"), "{err}");
    let saved = State::load(&path).unwrap();
    assert_eq!(
        saved.last_run,
        Some(utc(2026, 8, 31, 10, 20, 0)),
        "the window stays open"
    );
    assert!(saved.fired.is_empty(), "not marked fired");

    let mut delivered = Vec::new();
    let later = now + TimeDelta::minutes(1);
    let shown = run_with(
        &tasks(),
        &projects(),
        &path,
        later,
        &config,
        false,
        false,
        |n| {
            delivered.push(n.title.clone());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(shown, 1);
    assert_eq!(delivered, vec!["Remember the milk"]);
    let saved = State::load(&path).unwrap();
    assert_eq!(saved.last_run, Some(later));
    assert_eq!(saved.fired.len(), 1);
    assert!(path.with_extension("lock").exists());

    let shown = run_with(
        &tasks(),
        &projects(),
        &path,
        later + TimeDelta::minutes(1),
        &config,
        false,
        false,
        |_| panic!("nothing left to send"),
    )
    .unwrap();
    assert_eq!(shown, 0);
}
