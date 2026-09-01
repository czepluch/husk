use husk::recur::describe;

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
