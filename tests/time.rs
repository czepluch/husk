use chrono::{NaiveDate, TimeDelta, TimeZone, Utc};
use chrono_tz::America::Santiago;
use chrono_tz::Europe::Copenhagen;
use husk::time::{Zone, wall_to_utc};

#[test]
fn a_wall_time_in_a_dst_gap_moves_forward_by_the_skipped_hour() {
    // Chile springs forward at midnight, so 2026-09-06 00:00 does not exist.
    let midnight = NaiveDate::from_ymd_opt(2026, 9, 6)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    assert_eq!(
        wall_to_utc(&Santiago, midnight),
        Utc.with_ymd_and_hms(2026, 9, 6, 4, 0, 0).unwrap(),
        "01:00 at UTC-3"
    );
}

#[test]
fn an_ambiguous_wall_time_takes_its_first_pass() {
    let wall = NaiveDate::from_ymd_opt(2026, 10, 25)
        .unwrap()
        .and_hms_opt(2, 30, 0)
        .unwrap();
    assert_eq!(
        wall_to_utc(&Copenhagen, wall),
        Utc.with_ymd_and_hms(2026, 10, 25, 0, 30, 0).unwrap(),
        "still summer time"
    );
}

#[test]
fn adding_days_keeps_the_wall_time_across_a_dst_change() {
    let zone = Zone::Named(Copenhagen);
    let before = Utc.with_ymd_and_hms(2026, 3, 28, 8, 0, 0).unwrap(); // 09:00 CET
    assert_eq!(
        zone.add_days(before, TimeDelta::days(1)),
        Utc.with_ymd_and_hms(2026, 3, 29, 7, 0, 0).unwrap(),
        "09:00 CEST"
    );
}
