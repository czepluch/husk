use chrono::NaiveDate;
use husk::recur::{Advance, advance, describe};

#[test]
fn plain_frequencies() {
    assert_eq!(describe("FREQ=DAILY"), "daily");
    assert_eq!(describe("FREQ=WEEKLY"), "weekly");
    assert_eq!(describe("FREQ=MONTHLY"), "monthly");
    assert_eq!(describe("FREQ=YEARLY"), "yearly");
    assert_eq!(describe("freq=weekly"), "weekly", "case does not matter");
}

#[test]
fn intervals() {
    assert_eq!(describe("FREQ=DAILY;INTERVAL=1"), "daily");
    assert_eq!(describe("FREQ=DAILY;INTERVAL=2"), "every 2 days");
    assert_eq!(describe("FREQ=WEEKLY;INTERVAL=2"), "every 2 weeks");
    assert_eq!(describe("FREQ=MONTHLY;INTERVAL=3"), "every 3 months");
    assert_eq!(describe("FREQ=YEARLY;INTERVAL=2"), "every 2 years");
}

#[test]
fn weekdays_and_ordinals() {
    assert_eq!(describe("FREQ=WEEKLY;BYDAY=MO"), "weekly on Mon");
    assert_eq!(
        describe("FREQ=WEEKLY;BYDAY=MO,WE,FR"),
        "weekly on Mon, Wed, Fri"
    );
    assert_eq!(
        describe("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU"),
        "every 2 weeks on Tue"
    );
    assert_eq!(describe("FREQ=MONTHLY;BYDAY=2TU"), "every 2nd Tuesday");
    assert_eq!(describe("FREQ=MONTHLY;BYDAY=1MO"), "every 1st Monday");
    assert_eq!(describe("FREQ=MONTHLY;BYDAY=3SU"), "every 3rd Sunday");
    assert_eq!(describe("FREQ=MONTHLY;BYDAY=-1FR"), "every last Friday");
    assert_eq!(
        describe("FREQ=MONTHLY;BYDAY=-2TH"),
        "every 2nd last Thursday"
    );
    assert_eq!(
        describe("FREQ=MONTHLY;INTERVAL=2;BYDAY=2TU"),
        "every 2nd Tuesday every 2 months"
    );
    assert_eq!(
        describe("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR"),
        "daily on Mon, Tue, Wed, Thu, Fri"
    );
}

#[test]
fn month_days_and_months() {
    assert_eq!(
        describe("FREQ=MONTHLY;BYMONTHDAY=15"),
        "monthly on the 15th"
    );
    assert_eq!(describe("FREQ=MONTHLY;BYMONTHDAY=1"), "monthly on the 1st");
    assert_eq!(
        describe("FREQ=MONTHLY;BYMONTHDAY=-1"),
        "monthly on the last day"
    );
    assert_eq!(
        describe("FREQ=YEARLY;BYMONTH=6;BYMONTHDAY=1"),
        "yearly on 1 Jun"
    );
    assert_eq!(describe("FREQ=YEARLY;BYMONTH=12"), "yearly in Dec");
}

#[test]
fn until_and_count() {
    assert_eq!(
        describe("FREQ=WEEKLY;UNTIL=20260930"),
        "weekly until 2026-09-30"
    );
    assert_eq!(
        describe("FREQ=WEEKLY;UNTIL=20260930T215959Z"),
        "weekly until 2026-09-30"
    );
    assert_eq!(describe("FREQ=DAILY;COUNT=5"), "daily, 5 times");
    assert_eq!(describe("FREQ=DAILY;COUNT=1"), "daily, once");
    assert_eq!(
        describe("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261231;INTERVAL=2"),
        "every 2 weeks on Mon until 2026-12-31"
    );
}

#[test]
fn anything_else_is_shown_as_written() {
    assert_eq!(describe("FREQ=HOURLY;INTERVAL=6"), "FREQ=HOURLY;INTERVAL=6");
    assert_eq!(describe("FREQ=WEEKLY;BYDAY=XX"), "FREQ=WEEKLY;BYDAY=XX");
    assert_eq!(describe("nonsense"), "nonsense");
    assert_eq!(describe(""), "");
    assert_eq!(
        describe("FREQ=WEEKLY;BYSETPOS=1"),
        "FREQ=WEEKLY;BYSETPOS=1",
        "unhandled parts are not silently dropped"
    );
}

#[test]
fn several_ordinal_days_are_all_named_and_mixed_forms_are_shown_as_written() {
    assert_eq!(
        describe("FREQ=MONTHLY;BYDAY=1MO,3MO"),
        "every 1st Monday and 3rd Monday"
    );
    assert_eq!(
        describe("FREQ=MONTHLY;BYDAY=2TU,-1FR"),
        "every 2nd Tuesday and last Friday"
    );
    assert_eq!(
        describe("FREQ=WEEKLY;BYDAY=MO,2TU"),
        "FREQ=WEEKLY;BYDAY=MO,2TU"
    );
    assert_eq!(describe("FREQ=WEEKLY;WKST=MO;BYDAY=MO"), "weekly on Mon");
}

#[test]
fn invalid_values_are_shown_as_written() {
    assert_eq!(describe("FREQ=DAILY;COUNT=0"), "FREQ=DAILY;COUNT=0");
    assert_eq!(
        describe("FREQ=MONTHLY;BYMONTHDAY=32"),
        "FREQ=MONTHLY;BYMONTHDAY=32"
    );
    assert_eq!(
        describe("FREQ=MONTHLY;BYMONTHDAY=-31"),
        "FREQ=MONTHLY;BYMONTHDAY=-31"
    );
    assert_eq!(describe("FREQ=DAILY;INTERVAL=0"), "FREQ=DAILY;INTERVAL=0");
}

#[test]
fn a_utc_until_shows_the_local_date_it_falls_on() {
    use chrono::{Local, TimeZone, Utc};
    let expected = Utc
        .with_ymd_and_hms(2026, 12, 31, 23, 0, 0)
        .unwrap()
        .with_timezone(&Local)
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(
        describe("FREQ=WEEKLY;UNTIL=20261231T230000Z"),
        format!("weekly until {expected}")
    );
    assert_eq!(
        describe("FREQ=WEEKLY;UNTIL=20261231T230000"),
        "weekly until 2026-12-31",
        "floating stays as written"
    );
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn next(rule: &str, from: NaiveDate) -> NaiveDate {
    match advance(rule, from) {
        Advance::Next { date, rrule: None } => date,
        other => panic!("{rule}: {other:?}"),
    }
}

#[test]
fn advancing_steps_by_the_frequency_and_interval() {
    assert_eq!(next("FREQ=DAILY", d(2026, 9, 7)), d(2026, 9, 8));
    assert_eq!(next("FREQ=DAILY;INTERVAL=3", d(2026, 9, 7)), d(2026, 9, 10));
    assert_eq!(next("FREQ=WEEKLY", d(2026, 8, 31)), d(2026, 9, 7));
    assert_eq!(
        next("FREQ=WEEKLY;INTERVAL=2", d(2026, 8, 31)),
        d(2026, 9, 14)
    );
    assert_eq!(
        next("FREQ=MONTHLY", d(2026, 1, 31)),
        d(2026, 3, 31),
        "February has no 31st"
    );
    assert_eq!(
        next("FREQ=MONTHLY;INTERVAL=2", d(2026, 9, 7)),
        d(2026, 11, 7)
    );
    assert_eq!(next("FREQ=YEARLY", d(2026, 9, 7)), d(2027, 9, 7));
    assert_eq!(
        next("FREQ=YEARLY", d(2024, 2, 29)),
        d(2028, 2, 29),
        "a leap day skips to the next leap year"
    );
}

#[test]
fn advancing_honours_byday_and_bymonthday() {
    assert_eq!(
        next("FREQ=WEEKLY;BYDAY=MO,TH", d(2026, 9, 7)),
        d(2026, 9, 10)
    );
    assert_eq!(
        next("FREQ=WEEKLY;BYDAY=MO,TH", d(2026, 9, 10)),
        d(2026, 9, 14)
    );
    assert_eq!(
        next("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH", d(2026, 9, 7)),
        d(2026, 9, 10),
        "the Thursday of the same week still counts"
    );
    assert_eq!(
        next("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH", d(2026, 9, 10)),
        d(2026, 9, 21),
        "the next week is off phase"
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYMONTHDAY=15", d(2026, 9, 15)),
        d(2026, 10, 15)
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYMONTHDAY=-1", d(2026, 9, 30)),
        d(2026, 10, 31)
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYMONTHDAY=31", d(2026, 8, 31)),
        d(2026, 10, 31),
        "September has no 31st"
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYDAY=2TU", d(2026, 9, 8)),
        d(2026, 10, 13)
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYDAY=-1FR", d(2026, 9, 25)),
        d(2026, 10, 30)
    );
    assert_eq!(
        next("FREQ=MONTHLY;BYDAY=1MO,3FR", d(2026, 9, 7)),
        d(2026, 9, 18),
        "the same month's later day comes first"
    );
    assert_eq!(
        next("FREQ=YEARLY;BYMONTH=12;BYMONTHDAY=24", d(2026, 12, 24)),
        d(2027, 12, 24)
    );
}

#[test]
fn until_count_and_uncovered_parts_end_or_refuse() {
    assert_eq!(
        next("FREQ=WEEKLY;UNTIL=20260930", d(2026, 9, 21)),
        d(2026, 9, 28),
        "until is inclusive"
    );
    assert_eq!(
        advance("FREQ=WEEKLY;UNTIL=20260930", d(2026, 9, 28)),
        Advance::Ended
    );
    assert_eq!(
        advance("FREQ=WEEKLY;COUNT=3", d(2026, 9, 7)),
        Advance::Next {
            date: d(2026, 9, 14),
            rrule: Some("FREQ=WEEKLY;COUNT=2".to_string())
        }
    );
    assert_eq!(
        advance("FREQ=WEEKLY;COUNT=1", d(2026, 9, 7)),
        Advance::Ended
    );
    assert_eq!(
        advance("FREQ=WEEKLY;WKST=MO", d(2026, 9, 7)),
        Advance::Next {
            date: d(2026, 9, 14),
            rrule: None
        }
    );
    for rule in [
        "FREQ=HOURLY",
        "FREQ=WEEKLY;BYSETPOS=2",
        "FREQ=WEEKLY;BYDAY=MO;WKST=SU",
        "FREQ=WEEKLY;BYDAY=2TU",
        "FREQ=YEARLY;BYDAY=2TU",
        "not a rule",
    ] {
        assert_eq!(advance(rule, d(2026, 9, 7)), Advance::Unsupported, "{rule}");
    }
}
