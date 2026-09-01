//! Alarm defaults and fire times, shared by the TUI, the CLI and the
//! notifier.

use chrono::{DateTime, Local, NaiveTime, TimeDelta, TimeZone, Utc};

use crate::model::{Alarm, Anchor, Due, Task};

/// All-day dates have no time; their alarms count from this local hour.
const ALL_DAY_HOUR: u32 = 9;

/// A lead time from the config, `1d`, `2h`, `30m` or `0m`.
pub fn parse_lead(value: &str) -> Option<TimeDelta> {
    let value = value.trim();
    let unit = match value.chars().last()? {
        'd' => 86_400,
        'h' => 3_600,
        'm' => 60,
        _ => return None,
    };
    let digits = &value[..value.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    TimeDelta::try_seconds(n.checked_mul(unit)?)
}

/// The alarms a new timed task gets: one per configured lead time, relative
/// to the due time. Unreadable leads are skipped; duplicates collapse.
pub fn default_alarms(leads: &[String]) -> Vec<Alarm> {
    let mut alarms: Vec<Alarm> = Vec::new();
    for lead in leads.iter().filter_map(|l| parse_lead(l)) {
        let alarm = Alarm::Relative {
            offset: -lead,
            anchor: Anchor::Due,
        };
        if !alarms.contains(&alarm) {
            alarms.push(alarm);
        }
    }
    alarms
}

/// When a task's alarms fire, in UTC, sorted and without duplicates.
/// Relative triggers count from the due instant (`RELATED=END`) or the
/// start (`RELATED=START`, skipped when there is no `DTSTART`). A timed
/// task with no alarms gets one fire time per lead. `REPEAT` is ignored:
/// every alarm fires once.
pub fn fire_times(task: &Task, leads: &[String]) -> Vec<DateTime<Utc>> {
    let due = task.due.map(anchor);
    let start = task.start.map(anchor);
    let mut times: Vec<DateTime<Utc>> = task
        .alarms
        .iter()
        .filter_map(|alarm| match alarm {
            Alarm::Absolute(at) => Some(*at),
            Alarm::Relative {
                offset,
                anchor: Anchor::Due,
            } => due.map(|at| at + *offset),
            Alarm::Relative {
                offset,
                anchor: Anchor::Start,
            } => start.map(|at| at + *offset),
        })
        .collect();
    if task.alarms.is_empty()
        && let Some(Due::DateTime(at)) = task.due
    {
        times.extend(
            leads
                .iter()
                .filter_map(|l| parse_lead(l))
                .map(|lead| at - lead),
        );
    }
    times.sort();
    times.dedup();
    times
}

/// Past due: a timed due before now, or an all-day date before today.
pub fn is_overdue(due: Option<Due>, now: DateTime<Utc>) -> bool {
    match due {
        None => false,
        Some(Due::DateTime(at)) => at < now,
        Some(Due::Date(date)) => date < now.with_timezone(&Local).date_naive(),
    }
}

/// The instant a due or start value counts from: the time itself, or
/// `ALL_DAY_HOUR` local for a date.
fn anchor(due: Due) -> DateTime<Utc> {
    match due {
        Due::DateTime(at) => at,
        Due::Date(date) => {
            let wall =
                date.and_time(NaiveTime::from_hms_opt(ALL_DAY_HOUR, 0, 0).unwrap_or_default());
            Local
                .from_local_datetime(&wall)
                .earliest()
                .or_else(|| {
                    Local
                        .from_local_datetime(&(wall + TimeDelta::hours(1)))
                        .earliest()
                })
                .map_or_else(|| wall.and_utc(), |local| local.to_utc())
        }
    }
}
