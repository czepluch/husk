//! Wall-clock conversions in one place. Everything else does its time
//! arithmetic on UTC instants; the naive steps here exist because DST gaps
//! and "same wall time, another day" have no meaning on an instant.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;

use crate::model::Due;

/// In a DST gap the wall time does not exist; it is moved forward by the
/// hour the clocks skipped, which keeps it after the times before the gap.
/// An ambiguous time (clocks set back) takes its first occurrence.
pub fn wall_to_utc<Z: TimeZone>(zone: &Z, wall: NaiveDateTime) -> DateTime<Utc> {
    zone.from_local_datetime(&wall)
        .earliest()
        .or_else(|| {
            zone.from_local_datetime(&(wall + TimeDelta::hours(1)))
                .earliest()
        })
        .map_or_else(|| wall.and_utc(), |dt| dt.with_timezone(&Utc))
}

/// The local calendar date of an instant.
pub fn local_date(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&Local).date_naive()
}

/// The local calendar date a due value falls on.
pub fn due_date(due: Due) -> NaiveDate {
    match due {
        Due::Date(date) => date,
        Due::DateTime(at) => local_date(at),
    }
}

/// A zone wall times are read and written in: a named one, or the
/// machine's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Named(Tz),
    Local,
}

impl Zone {
    pub fn from_tz(tz: Option<Tz>) -> Self {
        tz.map_or(Self::Local, Self::Named)
    }

    pub fn wall(self, at: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Self::Named(tz) => at.with_timezone(&tz).naive_local(),
            Self::Local => at.with_timezone(&Local).naive_local(),
        }
    }

    pub fn to_utc(self, wall: NaiveDateTime) -> DateTime<Utc> {
        match self {
            Self::Named(tz) => wall_to_utc(&tz, wall),
            Self::Local => wall_to_utc(&Local, wall),
        }
    }

    /// The instant the same wall time falls on `days` later, so 09:00
    /// stays 09:00 across a DST change.
    pub fn add_days(self, at: DateTime<Utc>, days: TimeDelta) -> DateTime<Utc> {
        self.to_utc(self.wall(at) + days)
    }
}
