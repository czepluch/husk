mod common;

use chrono::{DateTime, Local, NaiveDate, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
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

const CPH: Tz = Tz::Europe__Copenhagen;

fn text_in(task: &Task, zone: Tz) -> String {
    codec::serialize(&vtodo::apply_in(task, Some(zone)).expect("apply"))
}

fn tz_text(zone: Tz, year: i32) -> String {
    codec::serialize(&codec::Document::new(vtodo::vtimezone(zone, year)))
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
fn new_timed_due_is_written_in_the_local_zone_with_a_vtimezone() {
    let mut task = load("apple/no-due.ics");
    task.due = Some(Due::DateTime(utc(2026, 9, 3, 7, 0, 0)));
    let after = text_in(&task, CPH);
    assert!(
        after.contains("DUE;TZID=Europe/Copenhagen:20260903T090000\n"),
        "{after}"
    );
    let todo = after.split("BEGIN:VTODO").nth(1).unwrap();
    assert!(!todo.contains("DTSTART"), "never adds DTSTART: {after}");
    let tz_at = after
        .find("BEGIN:VTIMEZONE")
        .expect("a VTIMEZONE was added");
    assert!(
        tz_at < after.find("BEGIN:VTODO").unwrap(),
        "before the VTODO:\n{after}"
    );
    assert!(after.contains("TZID:Europe/Copenhagen\n"), "{after}");
    let again = vtodo::parse_task(&after, ProjectId::new("p")).unwrap();
    assert_eq!(again.due, task.due, "same instant when read back");

    let none = text_in_none(&task);
    assert!(
        none.contains("DUE:20260903T070000Z\n"),
        "no zone known: UTC\n{none}"
    );
    assert!(!none.contains("VTIMEZONE"));
}

fn text_in_none(task: &Task) -> String {
    codec::serialize(&vtodo::apply_in(task, None).expect("apply"))
}

#[test]
fn a_utc_due_is_rewritten_in_the_zone_when_edited_and_apple_files_keep_theirs() {
    let mut task = load("todoman/timed-utc.ics");
    task.due = Some(Due::DateTime(utc(2026, 9, 2, 10, 0, 0)));
    let after = text_in(&task, CPH);
    assert!(
        after.contains("DUE;TZID=Europe/Copenhagen:20260902T120000\r\n"),
        "{after}"
    );
    assert_eq!(after.matches("BEGIN:VTIMEZONE").count(), 1, "{after}");

    let mut apple = load("apple/timed-alarm.ics");
    apple.due = Some(Due::DateTime(utc(2026, 9, 2, 10, 0, 0)));
    let after = text_in(&apple, Tz::America__New_York);
    assert!(
        after.contains("DUE;TZID=Europe/Copenhagen:20260902T120000\n"),
        "the file's own zone wins:\n{after}"
    );
    assert_eq!(
        after.matches("BEGIN:VTIMEZONE").count(),
        1,
        "Apple's VTIMEZONE is left alone"
    );
}

#[test]
fn generated_vtimezones_carry_the_years_transitions() {
    let text = tz_text(CPH, 2026);
    for line in [
        "TZID:Europe/Copenhagen",
        "BEGIN:STANDARD\r\nDTSTART:20260101T000000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\nTZNAME:CET",
        "BEGIN:DAYLIGHT\r\nDTSTART:20260329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST",
        "BEGIN:STANDARD\r\nDTSTART:20261025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:CET",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
    let utc_zone = tz_text(Tz::UTC, 2026);
    assert_eq!(utc_zone.matches("BEGIN:STANDARD").count(), 1, "{utc_zone}");
    assert!(!utc_zone.contains("DAYLIGHT"));
    assert!(utc_zone.contains("TZOFFSETTO:+0000"));
    let sydney = tz_text(Tz::Australia__Sydney, 2026);
    assert!(
        sydney.contains(
            "BEGIN:DAYLIGHT\r\nDTSTART:20260101T000000\r\nTZOFFSETFROM:+1100\r\nTZOFFSETTO:+1100"
        ),
        "summer on 1 January:\n{sydney}"
    );
    assert_eq!(
        sydney.matches("BEGIN:").count(),
        4,
        "VTIMEZONE plus three observances:\n{sydney}"
    );
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
    let doc = vtodo::new_document_in(&new, "abc-123", utc(2026, 8, 31, 13, 0, 0), Some(CPH));
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
        "DUE;TZID=Europe/Copenhagen:20260903T090000",
        "TZID:Europe/Copenhagen",
        "PRIORITY:1",
        "CATEGORIES:health",
        "ACTION:DISPLAY",
        "TRIGGER;RELATED=END:PT0S",
        "TRIGGER;RELATED=END:-PT1H",
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

#[test]
fn dst_gap_wall_times_move_forward_instead_of_becoming_utc() {
    let text = |t: &str| {
        format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:g\r\nDUE;TZID=Europe/Copenhagen:{t}\r\nSUMMARY:g\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
        )
    };
    let due = |t: &str| {
        vtodo::parse_task(&text(t), ProjectId::new("p"))
            .unwrap()
            .due
            .unwrap()
    };
    // Clocks jump from 02:00 CET to 03:00 CEST on 2026-03-29.
    assert_eq!(
        due("20260329T013000"),
        Due::DateTime(utc(2026, 3, 29, 0, 30, 0))
    );
    assert_eq!(
        due("20260329T023000"),
        Due::DateTime(utc(2026, 3, 29, 1, 30, 0))
    );
    assert_eq!(
        due("20260329T033000"),
        Due::DateTime(utc(2026, 3, 29, 1, 30, 0))
    );
    // Clocks fall back from 03:00 CEST to 02:00 CET on 2026-10-25; first occurrence wins.
    assert_eq!(
        due("20261025T023000"),
        Due::DateTime(utc(2026, 10, 25, 0, 30, 0))
    );
}

#[test]
fn multiple_categories_lines_merge_and_an_edit_keeps_the_first_in_place() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nCATEGORIES:a,b\r\nUID:c\r\nCATEGORIES:c\r\nSUMMARY:s\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let mut task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    assert_eq!(task.tags, vec!["a", "b", "c"]);
    task.tags = vec!["x".to_string()];
    assert_eq!(
        text_of(&task),
        "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nCATEGORIES:x\r\nUID:c\r\nSUMMARY:s\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
    );
}

#[test]
fn edits_keep_a_property_s_other_parameters() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:p\r\nSUMMARY;LANGUAGE=da:Hej\r\nDUE;VALUE=DATE-TIME;TZID=Europe/Copenhagen:20260831T120000\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let mut task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    assert_eq!(task.due, Some(Due::DateTime(utc(2026, 8, 31, 10, 0, 0))));
    task.summary = "Farvel".to_string();
    task.due = Some(Due::DateTime(utc(2026, 9, 1, 10, 0, 0)));
    let after = text_of(&task);
    assert!(after.contains("SUMMARY;LANGUAGE=da:Farvel\r\n"), "{after}");
    assert!(
        after.contains("DUE;VALUE=DATE-TIME;TZID=Europe/Copenhagen:20260901T120000\r\n"),
        "{after}"
    );
}

#[test]
fn unknown_tzid_reads_as_local_and_is_rewritten_in_a_known_zone() {
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:u\r\nDUE;TZID=W. Europe Standard Time:20260901T100000\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let mut task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    let wall = date(2026, 9, 1).and_hms_opt(10, 0, 0).unwrap();
    let expected = Local
        .from_local_datetime(&wall)
        .earliest()
        .unwrap()
        .to_utc();
    assert_eq!(task.due, Some(Due::DateTime(expected)));
    task.due = Some(Due::DateTime(utc(2026, 9, 2, 8, 0, 0)));
    let after = text_in(&task, CPH);
    assert!(
        after.contains("DUE;TZID=Europe/Copenhagen:20260902T100000\r\n"),
        "{after}"
    );
    assert!(
        after.contains("BEGIN:VTIMEZONE\r\nTZID:Europe/Copenhagen\r\n"),
        "{after}"
    );
    assert!(text_in_none(&task).contains("DUE:20260902T080000Z\r\n"));
}

#[test]
fn alarm_edits_keep_untouched_alarms_byte_for_byte() {
    let before = common::fixture("apple/timed-alarm.ics");
    let mut task = load("apple/timed-alarm.ics");
    task.alarms.push(Alarm::Relative {
        offset: -TimeDelta::hours(1),
        anchor: Anchor::Due,
    });
    let after = text_of(&task);
    assert!(
        after.contains("X-WR-ALARMUID:A90355B1-AC9C-4477-97A8-34DC5B1A3A65\n"),
        "{after}"
    );
    assert_eq!(after.matches("BEGIN:VALARM").count(), 2);
    let mut added = added_lines(&before, &after);
    added.sort();
    assert_eq!(
        added,
        vec!["DESCRIPTION:Remember the milk", "TRIGGER;RELATED=END:-PT1H"]
    );
    assert_eq!(
        vtodo::parse_task(&after, ProjectId::new("p"))
            .unwrap()
            .alarms,
        task.alarms
    );

    task.alarms.clear();
    assert!(!text_of(&task).contains("VALARM"));

    let mut task = load("tasksorg/timed-alarms-repeat.ics");
    task.alarms.retain(|a| {
        *a != Alarm::Relative {
            offset: TimeDelta::days(1),
            anchor: Anchor::Due,
        }
    });
    let after = text_of(&task);
    assert_eq!(after.matches("BEGIN:VALARM").count(), 2);
    assert!(!after.contains("REPEAT"), "{after}");
    assert!(after.contains("TRIGGER;RELATED=START:PT0S\n"), "{after}");
    assert!(
        after.ends_with("END:VALARM\nX-APPLE-SORT-ORDER:809862352\nEND:VTODO\nEND:VCALENDAR\n"),
        "{after}"
    );
}

#[test]
fn new_properties_go_before_the_alarms() {
    let mut task = load("tasksorg/parent-tag-folded-notes-alarms.ics");
    task.complete(utc(2026, 8, 31, 12, 0, 0));
    let after = text_of(&task);
    let first_alarm = after.find("BEGIN:VALARM").unwrap();
    for line in [
        "STATUS:COMPLETED",
        "COMPLETED:20260831T120000Z",
        "PERCENT-COMPLETE:100",
    ] {
        assert!(
            after.find(line).unwrap() < first_alarm,
            "{line} after the alarms:\n{after}"
        );
    }
    assert!(after.ends_with("X-APPLE-SORT-ORDER:809862353\nEND:VTODO\nEND:VCALENDAR\n"));
}

#[test]
fn absurd_durations_and_sequences_degrade_instead_of_panicking() {
    assert_eq!(parse_duration("P100000000000000D"), None);
    assert_eq!(parse_duration("P99999999999999999D"), None);
    assert_eq!(parse_duration("PT99999999999999999999S"), None);
    let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:s\r\nSEQUENCE:18446744073709551615\r\nBEGIN:VALARM\r\nTRIGGER:P100000000000000D\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let task = vtodo::parse_task(text, ProjectId::new("p")).unwrap();
    assert!(task.alarms.is_empty());
    let mut doc = task.raw().clone();
    vtodo::bump(&mut doc, utc(2026, 8, 31, 13, 0, 0)).unwrap();
    assert!(codec::serialize(&doc).contains("SEQUENCE:18446744073709551615\r\n"));
}

#[test]
fn vtimezone_onsets_are_exact_for_half_hour_zones_and_labels_follow_the_offset() {
    let st_johns = tz_text(Tz::America__St_Johns, 2026);
    for line in [
        "BEGIN:DAYLIGHT\r\nDTSTART:20260308T020000\r\nTZOFFSETFROM:-0330\r\nTZOFFSETTO:-0230\r\nTZNAME:NDT",
        "BEGIN:STANDARD\r\nDTSTART:20261101T020000\r\nTZOFFSETFROM:-0230\r\nTZOFFSETTO:-0330\r\nTZNAME:NST",
    ] {
        assert!(st_johns.contains(line), "missing {line:?} in\n{st_johns}");
    }
    let dublin = tz_text(Tz::Europe__Dublin, 2026);
    assert!(
        dublin.contains("BEGIN:STANDARD\r\nDTSTART:20260101T000000\r\nTZOFFSETFROM:+0000\r\nTZOFFSETTO:+0000\r\nTZNAME:GMT"),
        "winter is not daylight time:\n{dublin}"
    );
    assert!(!dublin.contains("DAYLIGHT"), "{dublin}");
}

#[test]
fn a_husk_vtimezone_grows_when_the_due_moves_to_another_year() {
    let new = NewTask {
        summary: "Later".to_string(),
        due: Some(Due::DateTime(utc(2026, 9, 3, 7, 0, 0))),
        ..NewTask::default()
    };
    let text = codec::serialize(&vtodo::new_document_in(
        &new,
        "u-1",
        utc(2026, 8, 31, 13, 0, 0),
        Some(CPH),
    ));
    let mut task = vtodo::parse_task(&text, ProjectId::new("p")).unwrap();
    task.due = Some(Due::DateTime(utc(2027, 7, 1, 10, 0, 0)));
    let after = text_in(&task, CPH);
    assert!(
        after.contains("DUE;TZID=Europe/Copenhagen:20270701T120000\r\n"),
        "{after}"
    );
    assert_eq!(after.matches("BEGIN:VTIMEZONE").count(), 1, "{after}");
    assert!(
        after.contains("DTSTART:20260329T020000\r\n"),
        "2026 kept:\n{after}"
    );
    assert!(
        after.contains("DTSTART:20270328T020000\r\n"),
        "2027 added:\n{after}"
    );
    assert_eq!(
        vtodo::parse_task(&after, ProjectId::new("p")).unwrap().due,
        task.due
    );

    let before = common::fixture("apple/timed-alarm.ics");
    let mut apple = load("apple/timed-alarm.ics");
    apple.due = Some(Due::DateTime(utc(2027, 7, 1, 10, 0, 0)));
    let added = added_lines(&before, &text_in(&apple, CPH));
    assert_eq!(
        added.len(),
        2,
        "only DUE and DTSTART; Apple's RRULE VTIMEZONE covers 2027: {added:?}"
    );
}

#[test]
fn the_second_pass_through_the_repeated_hour_is_written_in_utc() {
    let mut task = load("apple/no-due.ics");
    task.due = Some(Due::DateTime(utc(2026, 10, 25, 0, 30, 0)));
    let first = text_in(&task, CPH);
    assert!(
        first.contains("DUE;TZID=Europe/Copenhagen:20261025T023000\n"),
        "{first}"
    );
    task.due = Some(Due::DateTime(utc(2026, 10, 25, 1, 30, 0)));
    let second = text_in(&task, CPH);
    assert!(second.contains("DUE:20261025T013000Z\n"), "{second}");
    assert!(!second.contains("VTIMEZONE"), "{second}");
    assert_eq!(
        vtodo::parse_task(&second, ProjectId::new("p")).unwrap().due,
        task.due
    );
}

#[test]
fn a_generated_file_round_trips_unchanged() {
    let new = NewTask {
        summary: "Round".to_string(),
        due: Some(Due::DateTime(utc(2026, 9, 3, 7, 0, 0))),
        ..NewTask::default()
    };
    let doc = vtodo::new_document_in(&new, "u-2", utc(2026, 8, 31, 13, 0, 0), Some(CPH));
    let text = codec::serialize(&doc);
    let task = vtodo::parse_task(&text, ProjectId::new("p")).unwrap();
    assert_eq!(vtodo::apply_in(&task, Some(CPH)).unwrap(), *task.raw());
    assert_eq!(vtodo::apply(&task).unwrap(), *task.raw());
    assert_eq!(codec::serialize(&vtodo::apply(&task).unwrap()), text);
}

#[test]
fn the_zone_comes_from_tz_when_it_is_a_name_else_from_the_system() {
    use vtodo::zone_named;
    assert_eq!(
        zone_named(Some("Europe/Oslo"), Some("Europe/Copenhagen")),
        Some(Tz::Europe__Oslo)
    );
    assert_eq!(
        zone_named(Some(":Europe/Oslo"), None),
        Some(Tz::Europe__Oslo)
    );
    assert_eq!(
        zone_named(Some(":/etc/localtime"), Some("Europe/Copenhagen")),
        Some(CPH)
    );
    assert_eq!(
        zone_named(
            Some("CET-1CEST,M3.5.0,M10.5.0/3"),
            Some("Europe/Copenhagen")
        ),
        Some(CPH)
    );
    assert_eq!(zone_named(Some("UTC"), None), Some(Tz::UTC));
    assert_eq!(zone_named(None, Some("Nowhere/Nope")), None);
    assert_eq!(zone_named(None, None), None);
}
