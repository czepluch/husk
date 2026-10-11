//! Time zones as iCalendar sees them: which zone new values are written
//! in, and the VTIMEZONE component that makes a file valid on its own.

use std::str::FromStr;

use chrono::{
    DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone,
};
use chrono_tz::{OffsetComponents, OffsetName, Tz};

use crate::ical::DATE_TIME;
use crate::ical::codec::{Component, Entry, Property};
use crate::model::Task;

/// The zone new timed values are written in: `TZ` when it names a zone
/// chrono-tz knows, else the system zone. `None` when neither does, in
/// which case values are written in UTC.
pub fn local_zone() -> Option<Tz> {
    let env = std::env::var("TZ").ok();
    let system = iana_time_zone::get_timezone().ok();
    zone_named(env.as_deref(), system.as_deref())
}

/// The zone of the task's `DUE` `TZID`, when chrono-tz knows it.
pub fn due_zone(task: &Task) -> Option<Tz> {
    let due = task.raw.root.child("VTODO")?.prop("DUE")?;
    Tz::from_str(due.param("TZID")?).ok()
}

/// `local_zone` for a given `TZ` value and system zone name. A `TZ` that
/// is a file path or a POSIX rule string is not a zone name and defers to
/// the system zone.
pub fn zone_named(tz_env: Option<&str>, system: Option<&str>) -> Option<Tz> {
    tz_env
        .and_then(|tz| Tz::from_str(tz.trim_start_matches(':')).ok())
        .or_else(|| Tz::from_str(system?).ok())
}

/// A VTIMEZONE for one zone covering one year. Phone clients read the
/// `TZID` and know the zone; the component is there so the file is valid
/// on its own. Apple and Tasks.org write theirs with RRULEs; those are
/// left alone.
pub fn vtimezone(tz: Tz, year: i32) -> Component {
    let mut component = Component::new("VTIMEZONE");
    component.set(Property::new("TZID", tz.name()));
    for observance in observances(tz, year) {
        component.push_child(observance);
    }
    component
}

/// The observance in force at midnight on 1 January, then one per change
/// of offset during the year, found by comparing day boundaries and
/// bisecting to the second.
fn observances(tz: Tz, year: i32) -> Vec<Component> {
    let start = year_start(tz, year);
    let end = year_start(tz, year + 1);
    let mut list = vec![observance(
        start.naive_local(),
        start.offset(),
        start.offset(),
    )];
    let mut previous = start;
    let mut probe = start;
    while probe < end {
        let next = (probe + TimeDelta::days(1)).min(end);
        if next.offset().fix() != previous.offset().fix() {
            let onset = transition(probe, next);
            // Written in the wall time of the observance being left, as
            // RFC 5545 wants.
            let leaving = onset.with_timezone(&previous.offset().fix()).naive_local();
            list.push(observance(leaving, previous.offset(), onset.offset()));
            previous = onset;
        }
        probe = next;
    }
    list
}

/// Midnight on 1 January in the zone, or midnight UTC should that wall
/// time not exist there.
fn year_start(tz: Tz, year: i32) -> DateTime<Tz> {
    let midnight = NaiveDate::from_ymd_opt(year, 1, 1)
        .unwrap_or_default()
        .and_time(NaiveTime::MIN);
    tz.from_local_datetime(&midnight)
        .earliest()
        .unwrap_or_else(|| tz.from_utc_datetime(&midnight))
}

/// The first instant after `lo`, to the second, with the offset in force
/// at `hi`.
fn transition(mut lo: DateTime<Tz>, mut hi: DateTime<Tz>) -> DateTime<Tz> {
    while hi - lo > TimeDelta::seconds(1) {
        let mid = lo + (hi - lo) / 2;
        if mid.offset().fix() == lo.offset().fix() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

fn observance(
    onset_wall: NaiveDateTime,
    from: &<Tz as TimeZone>::Offset,
    to: &<Tz as TimeZone>::Offset,
) -> Component {
    // tzdb models a few zones with negative daylight saving (Dublin in
    // winter); both halves of such a year are labelled STANDARD.
    let kind = if to.dst_offset() > TimeDelta::zero() {
        "DAYLIGHT"
    } else {
        "STANDARD"
    };
    let mut c = Component::new(kind);
    c.set(Property::new(
        "DTSTART",
        onset_wall.format(DATE_TIME).to_string(),
    ));
    c.set(Property::new(
        "TZOFFSETFROM",
        utc_offset(from.fix().local_minus_utc()),
    ));
    c.set(Property::new(
        "TZOFFSETTO",
        utc_offset(to.fix().local_minus_utc()),
    ));
    if let Some(name) = to.abbreviation() {
        c.set(Property::new("TZNAME", name));
    }
    c
}

/// `+0200` style offsets.
fn utc_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let total = seconds.abs() / 60;
    format!("{sign}{:02}{:02}", total / 60, total % 60)
}

/// Makes sure the file's VTIMEZONE for a zone covers `year`. One written
/// with RRULEs (Apple, Tasks.org) covers every year; one of husk's
/// per-year ones gets the observances of the years it lacks.
pub(crate) fn ensure_vtimezone(root: &mut Component, tz: Tz, year: i32) {
    let index = root.entries.iter().position(|entry| {
        matches!(entry, Entry::Component(c)
            if c.is("VTIMEZONE") && c.prop("TZID").is_some_and(|p| p.value.trim() == tz.name()))
    });
    let Some(index) = index else {
        let at = root
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Component(c) if c.is("VTODO")))
            .unwrap_or(root.entries.len());
        root.entries
            .insert(at, Entry::Component(vtimezone(tz, year)));
        return;
    };
    let Entry::Component(component) = &mut root.entries[index] else {
        return;
    };
    if component.children().any(|c| c.prop("RRULE").is_some()) {
        return;
    }
    let years: Vec<i32> = component
        .children()
        .filter_map(|c| c.prop("DTSTART"))
        .filter_map(|p| NaiveDateTime::parse_from_str(p.value.trim(), DATE_TIME).ok())
        .map(|d| d.year())
        .collect();
    let missing = match (years.iter().min(), years.iter().max()) {
        (Some(&first), Some(&last)) if (first..=last).contains(&year) => return,
        (Some(_), Some(&last)) if year > last => last + 1..=year,
        (Some(&first), Some(_)) => year..=first - 1,
        _ => year..=year,
    };
    for y in missing {
        for observance in observances(tz, y) {
            component.push_child(observance);
        }
    }
}
