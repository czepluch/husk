//! Human text for an `RRULE`: "weekly", "every 2nd Tuesday", "monthly on
//! the 15th until 2026-12-31". Display only; nothing is ever expanded, and
//! a rule with parts this does not understand is shown as written.

use std::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    fn unit(self) -> &'static str {
        match self {
            Self::Daily => "day",
            Self::Weekly => "week",
            Self::Monthly => "month",
            Self::Yearly => "year",
        }
    }

    fn plain(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
        }
    }
}

/// A weekday, with an optional ordinal like `2TU` (second Tuesday) or
/// `-1FR` (last Friday).
struct Day {
    ordinal: Option<i32>,
    short: &'static str,
    long: &'static str,
}

pub fn describe(rule: &str) -> String {
    text(rule).unwrap_or_else(|| rule.to_string())
}

fn text(rule: &str) -> Option<String> {
    let mut freq = None;
    let mut interval = 1u32;
    let mut days: Vec<Day> = Vec::new();
    let mut month_day: Option<i32> = None;
    let mut month: Option<u32> = None;
    let mut until: Option<String> = None;
    let mut count: Option<u32> = None;
    for part in rule.split(';').filter(|p| !p.is_empty()) {
        let (key, value) = part.split_once('=')?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match value.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                });
            }
            "INTERVAL" => interval = value.parse().ok().filter(|n| *n > 0)?,
            "BYDAY" => {
                for token in value.split(',') {
                    days.push(parse_day(token)?);
                }
            }
            "BYMONTHDAY" => month_day = Some(value.parse().ok().filter(|n| *n != 0)?),
            "BYMONTH" => month = Some(value.parse().ok().filter(|n| (1..=12).contains(n))?),
            "UNTIL" => until = Some(format_until(value)?),
            "COUNT" => count = Some(value.parse().ok()?),
            "WKST" => {}
            _ => return None,
        }
    }
    let freq = freq?;

    let mut out = String::new();
    let ordinal_day = days.iter().find(|d| d.ordinal.is_some());
    if let Some(day) = ordinal_day {
        // "every 2nd Tuesday", the shape people say out loud.
        write!(out, "every {} {}", ordinal_text(day.ordinal?), day.long).ok()?;
        if interval > 1 {
            write!(out, " every {interval} {}s", freq.unit()).ok()?;
        }
    } else if interval > 1 {
        write!(out, "every {interval} {}s", freq.unit()).ok()?;
    } else {
        out.push_str(freq.plain());
    }
    if ordinal_day.is_none() && !days.is_empty() {
        let names: Vec<&str> = days.iter().map(|d| d.short).collect();
        write!(out, " on {}", names.join(", ")).ok()?;
    }
    match (month, month_day) {
        (Some(m), Some(d)) => write!(out, " on {d} {}", month_name(m)?).ok()?,
        (Some(m), None) => write!(out, " in {}", month_name(m)?).ok()?,
        (None, Some(-1)) => out.push_str(" on the last day"),
        (None, Some(d)) if d > 0 => write!(out, " on the {}", ordinal_text(d)).ok()?,
        (None, Some(_)) => return None,
        (None, None) => {}
    }
    if let Some(until) = until {
        write!(out, " until {until}").ok()?;
    }
    match count {
        Some(1) => out.push_str(", once"),
        Some(n) => write!(out, ", {n} times").ok()?,
        None => {}
    }
    Some(out)
}

fn parse_day(token: &str) -> Option<Day> {
    let token = token.trim().to_ascii_uppercase();
    let split = token.len().checked_sub(2)?;
    let (prefix, name) = token.split_at(split);
    let ordinal = if prefix.is_empty() {
        None
    } else {
        Some(
            prefix
                .parse::<i32>()
                .ok()
                .filter(|n| *n != 0 && n.abs() <= 5)?,
        )
    };
    let (short, long) = match name {
        "MO" => ("Mon", "Monday"),
        "TU" => ("Tue", "Tuesday"),
        "WE" => ("Wed", "Wednesday"),
        "TH" => ("Thu", "Thursday"),
        "FR" => ("Fri", "Friday"),
        "SA" => ("Sat", "Saturday"),
        "SU" => ("Sun", "Sunday"),
        _ => return None,
    };
    Some(Day {
        ordinal,
        short,
        long,
    })
}

fn ordinal_text(n: i32) -> String {
    match n {
        -1 => "last".to_string(),
        n if n < 0 => format!("{} last", ordinal_text(-n)),
        n => {
            let suffix = match (n % 10, n % 100) {
                (1, 11) | (2, 12) | (3, 13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
    }
}

fn month_name(m: u32) -> Option<&'static str> {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(m.checked_sub(1)? as usize)
    .copied()
}

/// `20260930` or `20260930T215959Z` as `2026-09-30`.
fn format_until(value: &str) -> Option<String> {
    let date = value.get(..8)?;
    if !date.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..8]))
}
