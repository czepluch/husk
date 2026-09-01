mod common;

use chrono::{DateTime, Local, NaiveDate, TimeDelta, TimeZone, Utc};
use husk::alarms::{default_alarms, fire_times, parse_lead};
use husk::ical::vtodo;
use husk::model::{Alarm, Anchor, ProjectId, Task};

fn load(name: &str) -> Task {
    vtodo::parse_task(&common::fixture(name), ProjectId::new("p")).unwrap()
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

/// 09:00 local on a date, as the UTC instant all-day alarms count from.
fn nine(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Local
        .from_local_datetime(
            &NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
        )
        .single()
        .unwrap()
        .to_utc()
}

fn leads() -> Vec<String> {
    vec!["1d".to_string(), "1h".to_string(), "0m".to_string()]
}

#[test]
fn leads_parse_into_offsets() {
    assert_eq!(parse_lead("1d"), Some(TimeDelta::days(1)));
    assert_eq!(parse_lead("2h"), Some(TimeDelta::hours(2)));
    assert_eq!(parse_lead("30m"), Some(TimeDelta::minutes(30)));
    assert_eq!(parse_lead(" 0m "), Some(TimeDelta::zero()));
    for bad in ["", "x", "1w", "d", "-1h", "1.5h"] {
        assert_eq!(parse_lead(bad), None, "{bad:?}");
    }
    assert_eq!(
        default_alarms(&[
            "1h".to_string(),
            "bogus".to_string(),
            "1h".to_string(),
            "0m".to_string()
        ]),
        vec![
            Alarm::Relative {
                offset: -TimeDelta::hours(1),
                anchor: Anchor::Due
            },
            Alarm::Relative {
                offset: TimeDelta::zero(),
                anchor: Anchor::Due
            },
        ]
    );
}

#[test]
fn absolute_alarms_fire_at_their_instant() {
    let task = load("apple/timed-alarm.ics");
    assert_eq!(
        fire_times(&task, &leads()),
        vec![utc(2026, 8, 31, 10, 25, 0)]
    );
}

#[test]
fn relative_alarms_count_from_due_or_start_and_repeat_is_ignored() {
    let task = load("tasksorg/timed-alarms-repeat.ics");
    assert_eq!(
        fire_times(&task, &leads()),
        vec![
            utc(2026, 8, 31, 10, 30, 0),
            utc(2026, 8, 31, 11, 0, 1),
            utc(2026, 9, 1, 11, 0, 1),
        ]
    );
}

#[test]
fn all_day_dates_anchor_at_nine_local() {
    let task = load("tasksorg/parent-tag-folded-notes-alarms.ics");
    assert_eq!(
        fire_times(&task, &leads()),
        vec![nine(2026, 8, 31), nine(2026, 9, 1), nine(2026, 9, 2)]
    );
}

#[test]
fn a_timed_task_without_alarms_gets_the_lead_times() {
    let task = load("todoman/timed-utc.ics");
    assert_eq!(
        fire_times(&task, &leads()),
        vec![
            utc(2026, 8, 31, 8, 0, 0),
            utc(2026, 9, 1, 7, 0, 0),
            utc(2026, 9, 1, 8, 0, 0),
        ]
    );
    assert!(fire_times(&task, &[]).is_empty(), "no leads, no alarms");
}

#[test]
fn undated_and_all_day_tasks_without_alarms_are_silent() {
    assert!(fire_times(&load("apple/no-due.ics"), &leads()).is_empty());
    assert!(fire_times(&load("apple/all-day.ics"), &leads()).is_empty());
    assert!(fire_times(&load("tasksorg/subtask.ics"), &leads()).is_empty());
}

#[test]
fn duplicate_instants_collapse_and_times_are_sorted() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:d\r\nDUE:20260901T080000Z\r\nBEGIN:VALARM\r\nTRIGGER;RELATED=END:PT0S\r\nEND:VALARM\r\nBEGIN:VALARM\r\nTRIGGER;VALUE=DATE-TIME:20260901T080000Z\r\nEND:VALARM\r\nBEGIN:VALARM\r\nTRIGGER;RELATED=END:-PT1H\r\nEND:VALARM\r\nBEGIN:VALARM\r\nTRIGGER;RELATED=START:PT0S\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    assert_eq!(
        fire_times(&task, &leads()),
        vec![utc(2026, 9, 1, 7, 0, 0), utc(2026, 9, 1, 8, 0, 0)],
        "a START-relative alarm without DTSTART is skipped"
    );
}
