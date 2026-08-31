//! Alarm defaults shared by the TUI and the CLI. Fire-time computation for
//! the notifier joins this module in M4.

use chrono::TimeDelta;

use crate::model::{Alarm, Anchor};

/// A lead time from the config, `1d`, `2h`, `30m` or `0m`.
pub fn parse_lead(value: &str) -> Option<TimeDelta> {
    let value = value.trim();
    let unit = match value.chars().last()? {
        'd' => 86_400,
        'h' => 3_600,
        'm' => 60,
        _ => return None,
    };
    let n: i64 = value[..value.len() - 1].parse().ok()?;
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
