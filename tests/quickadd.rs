use chrono::{DateTime, Local, NaiveDate, TimeZone};
use husk::model::{Due, Priority};
use husk::quickadd::{self, QuickAdd};

/// A Monday at noon.
fn now() -> DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 8, 31, 12, 0, 0)
        .single()
        .unwrap()
}

fn parse(input: &str) -> QuickAdd {
    quickadd::parse(input, now())
}

fn date(y: i32, m: u32, d: u32) -> Due {
    Due::Date(NaiveDate::from_ymd_opt(y, m, d).unwrap())
}

fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> Due {
    Due::DateTime(
        Local
            .with_ymd_and_hms(y, m, d, h, mi, 0)
            .single()
            .unwrap()
            .to_utc(),
    )
}

#[test]
fn the_spec_example_parses_fully() {
    let q = parse("Book dentist due:tomorrow 09:00 pri:H +health @personal");
    assert_eq!(q.summary, "Book dentist");
    assert_eq!(q.due, Some(at(2026, 9, 1, 9, 0)));
    assert_eq!(q.priority, Priority::High);
    assert_eq!(q.tags, vec!["health"]);
    assert_eq!(q.project.as_deref(), Some("personal"));
}

#[test]
fn date_forms() {
    assert_eq!(parse("x due:today").due, Some(date(2026, 8, 31)));
    assert_eq!(parse("x due:tomorrow").due, Some(date(2026, 9, 1)));
    assert_eq!(parse("x due:fri").due, Some(date(2026, 9, 4)));
    assert_eq!(parse("x due:friday").due, Some(date(2026, 9, 4)));
    assert_eq!(
        parse("x due:mon").due,
        Some(date(2026, 9, 7)),
        "a weekday is never today"
    );
    assert_eq!(parse("x due:next mon").due, Some(date(2026, 9, 7)));
    assert_eq!(parse("x due:+2d").due, Some(date(2026, 9, 2)));
    assert_eq!(parse("x due:+1w").due, Some(date(2026, 9, 7)));
    assert_eq!(parse("x due:2026-09-03").due, Some(date(2026, 9, 3)));
}

#[test]
fn times_attach_to_the_preceding_date() {
    assert_eq!(
        parse("x due:2026-09-03 14:30").due,
        Some(at(2026, 9, 3, 14, 30))
    );
    assert_eq!(parse("x due:fri 9:00").due, Some(at(2026, 9, 4, 9, 0)));
    assert_eq!(
        parse("x due:next mon 08:15").due,
        Some(at(2026, 9, 7, 8, 15))
    );
    assert_eq!(
        parse("x due:14:30").due,
        Some(at(2026, 8, 31, 14, 30)),
        "a bare time means today"
    );
    let q = parse("call due:tomorrow 09:00 about the thing");
    assert_eq!(q.summary, "call about the thing");
    assert_eq!(q.due, Some(at(2026, 9, 1, 9, 0)));
}

#[test]
fn times_not_after_a_due_token_stay_in_the_summary() {
    let q = parse("lunch at 12:00");
    assert_eq!(q.summary, "lunch at 12:00");
    assert_eq!(q.due, None);
}

#[test]
fn tags_projects_and_priority() {
    let q = parse("hello +a world +b @x @y pri:l pri:h");
    assert_eq!(q.summary, "hello world");
    assert_eq!(q.tags, vec!["a", "b"]);
    assert_eq!(q.project.as_deref(), Some("y"), "the last project wins");
    assert_eq!(q.priority, Priority::High, "the last priority wins");
    assert_eq!(parse("x pri:M").priority, Priority::Medium);
    assert_eq!(parse("x pri:low").priority, Priority::Low);
}

#[test]
fn unknown_tokens_stay_in_the_summary_and_nothing_errors() {
    assert_eq!(
        parse("Pay rent due:garbage").summary,
        "Pay rent due:garbage"
    );
    assert_eq!(parse("x due:next foo").summary, "x due:next foo");
    assert_eq!(parse("x pri:z").summary, "x pri:z");
    assert_eq!(parse("+ @ pri: due:").summary, "+ @ pri: due:");
    assert_eq!(parse("").summary, "");
    assert_eq!(parse("   ").summary, "");
    let q = parse("due:mon due:tue");
    assert_eq!(q.due, Some(date(2026, 9, 1)), "the last due wins");
    assert_eq!(q.summary, "");
}
