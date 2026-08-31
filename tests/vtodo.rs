mod common;

use chrono::{DateTime, Local, NaiveDate, TimeDelta, TimeZone, Utc};
use husk::ical::codec;
use husk::ical::vtodo::{self, format_duration, parse_duration};
use husk::model::{Alarm, Anchor, Due, NewTask, Priority, ProjectId, Status, Task};

fn load(name: &str) -> Task {
    vtodo::parse_task(&common::fixture(name), ProjectId::new("life"))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn text_of(task: &Task) -> String {
    codec::serialize(&vtodo::apply(task).expect("apply"))
}

/// Lines present in `after` that were not in `before`.
fn added_lines(before: &str, after: &str) -> Vec<String> {
    let before = codec::unfold(before);
    codec::unfold(after)
        .lines()
        .filter(|l| !before.lines().any(|b| b == *l))
        .map(String::from)
        .collect()
}

#[test]
fn reads_apple_fixtures() {
    let t = load("apple/no-due.ics");
    assert_eq!(t.uid, "BD215969-28EE-4474-A2E4-61575C3C49E7");
    assert_eq!(t.summary, "Test task");
    assert_eq!(t.due, None);
    assert_eq!(t.status, Status::NeedsAction);
    assert_eq!(t.priority, Priority::None);
    assert!(t.tags.is_empty() && t.alarms.is_empty());
    assert_eq!(t.created, Some(utc(2026, 8, 31, 9, 44, 32)));

    let t = load("apple/all-day.ics");
    assert_eq!(t.due, Some(Due::Date(date(2026, 8, 31))));
    assert_eq!(t.start, Some(Due::Date(date(2026, 8, 31))));

    let t = load("apple/timed-alarm.ics");
    assert_eq!(t.due, Some(Due::DateTime(utc(2026, 8, 31, 10, 25, 0))));
    assert_eq!(t.alarms, vec![Alarm::Absolute(utc(2026, 8, 31, 10, 25, 0))]);

    let t = load("apple/timed-alarm-priority-high.ics");
    assert_eq!(t.priority, Priority::High);

    let t = load("apple/completed.ics");
    assert_eq!(t.status, Status::Completed);
    assert_eq!(t.completed, Some(utc(2026, 8, 31, 10, 14, 51)));
    assert!(t.is_done());

    let t = load("apple/recurring-weekly-until.ics");
    assert_eq!(t.rrule.as_deref(), Some("FREQ=WEEKLY;UNTIL=20260930"));
    assert!(t.is_recurring());

    let t = load("apple/priority-medium-long-title.ics");
    assert_eq!(t.priority, Priority::Medium);
    assert_eq!(t.summary, "Hvad skal der købes når man skal hjælpe får?");

    let t = load("apple/notes-priority-low.ics");
    assert_eq!(t.priority, Priority::Low);
    assert_eq!(
        t.description.as_deref(),
        Some("Her\nEr\nEn note,\n;;;\n#tag\n")
    );
    assert!(t.tags.is_empty(), "a #tag in the title is not a tag");
}

#[test]
fn reads_tasksorg_fixtures() {
    let t = load("tasksorg/priority-low.ics");
    assert_eq!(t.uid, "200671310813144693");
    assert_eq!(t.status, Status::NeedsAction, "missing STATUS");
    assert_eq!(t.priority, Priority::Low);

    let t = load("tasksorg/parent-tag-folded-notes-alarms.ics");
    assert_eq!(t.tags, vec!["Testing"]);
    assert_eq!(t.due, Some(Due::Date(date(2026, 9, 1))));
    assert_eq!(t.start, Some(Due::Date(date(2026, 8, 31))));
    assert_eq!(t.priority, Priority::High);
    assert_eq!(
        t.description.as_deref(),
        Some("Bigger parent task testing many things in one\n..\n,,//\n;;;")
    );
    assert_eq!(
        t.alarms,
        vec![
            Alarm::Relative {
                offset: TimeDelta::zero(),
                anchor: Anchor::Start
            },
            Alarm::Relative {
                offset: TimeDelta::zero(),
                anchor: Anchor::Due
            },
            Alarm::Relative {
                offset: TimeDelta::days(1),
                anchor: Anchor::Due
            },
        ]
    );

    let t = load("tasksorg/subtask.ics");
    assert_eq!(t.parent.as_deref(), Some("381262659200806456"));

    let t = load("tasksorg/timed-alarms-repeat.ics");
    assert_eq!(t.due, Some(Due::DateTime(utc(2026, 8, 31, 11, 0, 1))));
    assert_eq!(t.start, Some(Due::DateTime(utc(2026, 8, 31, 10, 30, 0))));
    assert_eq!(t.alarms.len(), 3);
}

#[test]
fn reads_todoman_fixture() {
    let t = load("todoman/timed-utc.ics");
    assert_eq!(t.due, Some(Due::DateTime(utc(2026, 9, 1, 8, 0, 0))));
    assert_eq!(t.summary, "From laptop via todoman");
}

#[test]
fn unchanged_task_applies_to_an_identical_document() {
    for (name, text) in common::fixtures() {
        let task = vtodo::parse_task(&text, ProjectId::new("p"))
            .unwrap_or_else(|e| panic!("{}: {e:#}", name.display()));
        let doc = vtodo::apply(&task).expect("apply");
        assert_eq!(&doc, task.raw(), "{}", name.display());
        assert_eq!(codec::serialize(&doc), text, "{}", name.display());
    }
}

#[test]
fn changing_summary_touches_only_summary() {
    let before = common::fixture("apple/timed-alarm.ics");
    let mut task = load("apple/timed-alarm.ics");
    task.summary = "Remember the milk, then; go".to_string();
    let after = text_of(&task);
    assert_eq!(
        added_lines(&before, &after),
        vec![r"SUMMARY:Remember the milk\, then\; go"]
    );
    assert_eq!(after.lines().count(), before.lines().count());
}

#[test]
fn changing_a_timed_due_keeps_the_tzid_form_and_moves_dtstart_with_it() {
    let before = common::fixture("apple/timed-alarm.ics");
    let mut task = load("apple/timed-alarm.ics");
    task.due = Some(Due::DateTime(utc(2026, 9, 2, 7, 0, 0)));
    let mut added = added_lines(&before, &text_of(&task));
    added.sort();
    assert_eq!(
        added,
        vec![
            "DTSTART;TZID=Europe/Copenhagen:20260902T090000",
            "DUE;TZID=Europe/Copenhagen:20260902T090000",
        ]
    );
}

#[test]
fn a_separate_start_stays_unless_it_would_follow_the_due_date() {
    let before = common::fixture("tasksorg/parent-tag-folded-notes-alarms.ics");
    let mut task = load("tasksorg/parent-tag-folded-notes-alarms.ics");

    task.due = Some(Due::Date(date(2026, 9, 5)));
    assert_eq!(
        added_lines(&before, &text_of(&task)),
        vec!["DUE;VALUE=DATE:20260905"]
    );

    task.due = Some(Due::Date(date(2026, 8, 30)));
    let mut added = added_lines(&before, &text_of(&task));
    added.sort();
    assert_eq!(
        added,
        vec!["DTSTART;VALUE=DATE:20260830", "DUE;VALUE=DATE:20260830"]
    );
}

#[test]
fn removing_a_due_date_removes_a_dtstart_that_followed_it() {
    let mut task = load("apple/all-day.ics");
    task.due = None;
    let after = text_of(&task);
    assert!(!after.contains("DUE"), "{after}");
    assert!(!after.contains("DTSTART"), "{after}");

    let mut task = load("tasksorg/parent-tag-folded-notes-alarms.ics");
    task.due = None;
    let after = text_of(&task);
    assert!(!after.contains("\nDUE"), "{after}");
    assert!(after.contains("DTSTART;VALUE=DATE:20260831"), "{after}");
}

#[test]
fn switching_a_timed_due_to_all_day_drops_the_tzid() {
    let mut task = load("apple/timed-alarm.ics");
    task.due = Some(Due::Date(date(2026, 9, 2)));
    let after = text_of(&task);
    assert!(after.contains("DUE;VALUE=DATE:20260902\n"), "{after}");
    assert!(after.contains("DTSTART;VALUE=DATE:20260902\n"), "{after}");
}

#[test]
fn floating_times_are_local_and_stay_floating() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:f\r\nDUE:20260901T100000\r\nSUMMARY:Floating\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let mut task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    let wall = date(2026, 9, 1).and_hms_opt(10, 0, 0).unwrap();
    let expected = Local
        .from_local_datetime(&wall)
        .earliest()
        .unwrap()
        .to_utc();
    assert_eq!(task.due, Some(Due::DateTime(expected)));

    task.due = Some(Due::DateTime(expected + TimeDelta::days(1)));
    let after = text_of(&task);
    assert!(after.contains("DUE:20260902T100000\r\n"), "{after}");
}

#[test]
fn new_timed_due_on_a_task_without_one_is_written_in_utc() {
    let mut task = load("apple/no-due.ics");
    task.due = Some(Due::DateTime(utc(2026, 9, 3, 7, 0, 0)));
    let after = text_of(&task);
    assert!(after.contains("DUE:20260903T070000Z\n"), "{after}");
    assert!(!after.contains("DTSTART"), "never adds DTSTART: {after}");
}

#[test]
fn completing_writes_the_apple_triple_and_reopening_removes_it() {
    let before = common::fixture("apple/no-due.ics");
    let mut task = load("apple/no-due.ics");
    task.complete(utc(2026, 8, 31, 12, 0, 0));
    let mut added = added_lines(&before, &text_of(&task));
    added.sort();
    assert_eq!(
        added,
        vec![
            "COMPLETED:20260831T120000Z",
            "PERCENT-COMPLETE:100",
            "STATUS:COMPLETED",
        ]
    );

    let mut task = load("apple/completed.ics");
    task.reopen();
    let after = text_of(&task);
    assert!(after.contains("STATUS:NEEDS-ACTION\n"), "{after}");
    assert!(!after.contains("COMPLETED:"), "{after}");
    assert!(!after.contains("PERCENT-COMPLETE"), "{after}");
}

#[test]
fn tags_are_written_as_one_escaped_categories_line() {
    let mut task = load("tasksorg/parent-tag-folded-notes-alarms.ics");
    task.tags = vec!["a,b".to_string(), "c;d".to_string()];
    let after = text_of(&task);
    assert!(
        after.contains(concat!("CATEGORIES:a\\,b,c\\", ";d\n")),
        "{after}"
    );
    assert_eq!(after.matches("CATEGORIES").count(), 1);
    let again = vtodo::parse_task(&after, ProjectId::new("p")).unwrap();
    assert_eq!(again.tags, vec!["a,b", "c;d"]);

    task.tags.clear();
    assert!(!text_of(&task).contains("CATEGORIES"));
}

#[test]
fn priority_none_removes_the_property() {
    let mut task = load("apple/timed-alarm-priority-high.ics");
    task.priority = Priority::None;
    assert!(!text_of(&task).contains("PRIORITY"));
    task.priority = Priority::Low;
    assert!(text_of(&task).contains("PRIORITY:9\n"));
}

#[test]
fn bump_increments_sequence_and_stamps_both_dates() {
    let now = utc(2026, 8, 31, 13, 0, 0);
    let mut doc = load("apple/no-due.ics").raw().clone();
    vtodo::bump(&mut doc, now).unwrap();
    let text = codec::serialize(&doc);
    assert!(text.contains("SEQUENCE:1\n"), "{text}");
    assert!(text.contains("LAST-MODIFIED:20260831T130000Z\n"), "{text}");
    assert!(text.contains("DTSTAMP:20260831T130000Z\n"), "{text}");
    assert_eq!(text.matches("DTSTAMP").count(), 1);

    let mut doc = load("todoman/timed-utc.ics").raw().clone();
    vtodo::bump(&mut doc, now).unwrap();
    assert!(codec::serialize(&doc).contains("SEQUENCE:2\r\n"));
}

#[test]
fn new_document_round_trips_through_the_reader() {
    let new = NewTask {
        summary: "Book dentist, soon".to_string(),
        description: Some("line one\nline two".to_string()),
        due: Some(Due::DateTime(utc(2026, 9, 3, 7, 0, 0))),
        priority: Priority::High,
        tags: vec!["health".to_string()],
        alarms: vec![
            Alarm::Relative {
                offset: TimeDelta::zero(),
                anchor: Anchor::Due,
            },
            Alarm::Relative {
                offset: -TimeDelta::hours(1),
                anchor: Anchor::Due,
            },
        ],
    };
    let doc = vtodo::new_document(&new, "abc-123", utc(2026, 8, 31, 13, 0, 0));
    let text = codec::serialize(&doc);
    assert!(
        text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//husk//"),
        "{text}"
    );
    for line in [
        "UID:abc-123",
        "DTSTAMP:20260831T130000Z",
        "CREATED:20260831T130000Z",
        "LAST-MODIFIED:20260831T130000Z",
        "SEQUENCE:0",
        "STATUS:NEEDS-ACTION",
        r"SUMMARY:Book dentist\, soon",
        r"DESCRIPTION:line one\nline two",
        "DUE:20260903T070000Z",
        "PRIORITY:1",
        "CATEGORIES:health",
        "ACTION:DISPLAY",
        "TRIGGER:PT0S",
        "TRIGGER:-PT1H",
    ] {
        assert!(
            text.contains(&format!("{line}\r\n")),
            "missing {line} in\n{text}"
        );
    }

    let task = vtodo::from_document(doc, ProjectId::new("p")).unwrap();
    assert_eq!(task.uid, "abc-123");
    assert_eq!(task.summary, new.summary);
    assert_eq!(task.description, new.description);
    assert_eq!(task.due, new.due);
    assert_eq!(task.priority, new.priority);
    assert_eq!(task.tags, new.tags);
    assert_eq!(task.alarms, new.alarms);
    assert_eq!(task.status, Status::NeedsAction);
}

#[test]
fn durations_parse_and_format() {
    let cases = [
        ("PT0S", 0),
        ("-PT15M", -900),
        ("P1D", 86_400),
        ("-P1DT2H30M", -(86_400 + 9_000)),
        ("PT1H", 3_600),
        ("P2W", 14 * 86_400),
    ];
    for (text, seconds) in cases {
        assert_eq!(
            parse_duration(text),
            Some(TimeDelta::seconds(seconds)),
            "{text}"
        );
    }
    assert_eq!(parse_duration("-PT0M"), Some(TimeDelta::zero()));
    assert_eq!(parse_duration("+PT5M"), Some(TimeDelta::minutes(5)));
    for bad in ["", "15M", "P1X", "PT", "P1H", "P1DT"] {
        assert_eq!(parse_duration(bad), None, "{bad}");
    }
    for (text, seconds) in [
        ("PT0S", 0),
        ("-PT15M", -900),
        ("P1D", 86_400),
        ("-P1DT2H30M", -(86_400 + 9_000)),
        ("PT1H", 3_600),
        ("P14D", 14 * 86_400),
    ] {
        assert_eq!(format_duration(TimeDelta::seconds(seconds)), text);
    }
}
