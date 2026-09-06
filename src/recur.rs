//! Human text for an `RRULE` ("weekly", "every 2nd Tuesday", "monthly on
//! the 15th until 2026-12-31") and the single step from one occurrence to
//! the next, for completing. Nothing is ever expanded into a list of
//! occurrences; a rule with parts this does not understand is shown as
//! written and refused for advancing.

use std::fmt::Write;

use chrono::{Datelike, Days, Local, NaiveDate, NaiveDateTime, Weekday};

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
    weekday: Weekday,
    short: &'static str,
    long: &'static str,
}

struct Rule {
    freq: Freq,
    interval: u32,
    days: Vec<Day>,
    month_day: Option<i32>,
    month: Option<u32>,
    until: Option<NaiveDate>,
    count: Option<u32>,
    /// A `WKST` other than Monday moves week boundaries; describing such
    /// a rule is fine, advancing it is not.
    foreign_wkst: bool,
}

fn parse(text: &str) -> Option<Rule> {
    let mut freq = None;
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        days: Vec::new(),
        month_day: None,
        month: None,
        until: None,
        count: None,
        foreign_wkst: false,
    };
    for part in text.split(';').filter(|p| !p.is_empty()) {
        let (key, value) = part.split_once('=')?;
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match value.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                });
            }
            "INTERVAL" => rule.interval = value.parse().ok().filter(|n| *n > 0)?,
            "BYDAY" => {
                for token in value.split(',') {
                    rule.days.push(parse_day(token)?);
                }
            }
            "BYMONTHDAY" => {
                rule.month_day = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|n: &i32| *n != 0 && n.abs() <= 31)?,
                );
            }
            "BYMONTH" => rule.month = Some(value.parse().ok().filter(|n| (1..=12).contains(n))?),
            "UNTIL" => rule.until = Some(until_date(value.trim())?),
            "COUNT" => rule.count = Some(value.parse().ok().filter(|n| *n > 0)?),
            "WKST" => rule.foreign_wkst = !value.trim().eq_ignore_ascii_case("MO"),
            _ => return None,
        }
    }
    rule.freq = freq?;
    Some(rule)
}

pub fn describe(rule: &str) -> String {
    text(rule).unwrap_or_else(|| rule.to_string())
}

fn text(rule: &str) -> Option<String> {
    let rule = parse(rule)?;
    let freq = rule.freq;
    let interval = rule.interval;
    let days = rule.days;
    let month_day = rule.month_day;
    let month = rule.month;
    let until = rule.until.map(|date| date.format("%Y-%m-%d").to_string());
    let count = rule.count;

    let mut out = String::new();
    let with_ordinal = days.iter().filter(|d| d.ordinal.is_some()).count();
    if with_ordinal > 0 {
        // "every 2nd Tuesday", the shape people say out loud; a mix of
        // ordinal and plain days has no such shape and is shown as written.
        if with_ordinal != days.len() {
            return None;
        }
        let named: Vec<String> = days
            .iter()
            .filter_map(|d| Some(format!("{} {}", ordinal_text(d.ordinal?), d.long)))
            .collect();
        write!(out, "every {}", named.join(" and ")).ok()?;
        if interval > 1 {
            write!(out, " every {interval} {}s", freq.unit()).ok()?;
        }
    } else if interval > 1 {
        write!(out, "every {interval} {}s", freq.unit()).ok()?;
    } else {
        out.push_str(freq.plain());
    }
    if with_ordinal == 0 && !days.is_empty() {
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
    let (weekday, short, long) = match name {
        "MO" => (Weekday::Mon, "Mon", "Monday"),
        "TU" => (Weekday::Tue, "Tue", "Tuesday"),
        "WE" => (Weekday::Wed, "Wed", "Wednesday"),
        "TH" => (Weekday::Thu, "Thu", "Thursday"),
        "FR" => (Weekday::Fri, "Fri", "Friday"),
        "SA" => (Weekday::Sat, "Sat", "Saturday"),
        "SU" => (Weekday::Sun, "Sun", "Sunday"),
        _ => return None,
    };
    Some(Day {
        ordinal,
        weekday,
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

/// The local date an `UNTIL` value names; the rule includes it.
fn until_date(value: &str) -> Option<NaiveDate> {
    if let Some(utc) = value.strip_suffix('Z')
        && let Ok(at) = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S")
    {
        return Some(at.and_utc().with_timezone(&Local).date_naive());
    }
    NaiveDate::parse_from_str(value.get(..8)?, "%Y%m%d").ok()
}

/// What completing one occurrence does to the schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Advance {
    /// The rule has parts the computation does not cover.
    Unsupported,
    /// The completed occurrence was the rule's last.
    Ended,
    /// The next occurrence, and the rule rewritten when completing one
    /// of a counted series used up a count.
    Next {
        date: NaiveDate,
        rrule: Option<String>,
    },
}

/// The occurrence after `from`, computed as one step; nothing is expanded.
/// Interval phases are anchored on `from` itself, so completing late does
/// not skip ahead to the present.
pub fn advance(rule: &str, from: NaiveDate) -> Advance {
    let Some(parsed) = parse(rule) else {
        return Advance::Unsupported;
    };
    if parsed.foreign_wkst {
        return Advance::Unsupported;
    }
    let Some(next) = next_date(&parsed, from) else {
        return Advance::Unsupported;
    };
    if let Some(until) = parsed.until
        && next > until
    {
        return Advance::Ended;
    }
    match parsed.count {
        Some(count) if count <= 1 => Advance::Ended,
        Some(count) => Advance::Next {
            date: next,
            rrule: Some(with_count(rule, count - 1)),
        },
        None => Advance::Next {
            date: next,
            rrule: None,
        },
    }
}

fn next_date(rule: &Rule, from: NaiveDate) -> Option<NaiveDate> {
    match rule.freq {
        Freq::Daily if rule.days.is_empty() && rule.month_day.is_none() && rule.month.is_none() => {
            from.checked_add_days(Days::new(u64::from(rule.interval)))
        }
        Freq::Weekly if rule.month_day.is_none() && rule.month.is_none() => {
            if rule.days.is_empty() {
                return from.checked_add_days(Days::new(7 * u64::from(rule.interval)));
            }
            if rule.days.iter().any(|d| d.ordinal.is_some()) {
                return None;
            }
            let anchor = week_start(from)?;
            let mut candidate = from.succ_opt()?;
            for _ in 0..(7 * rule.interval.min(1000) + 7) {
                let weeks = (week_start(candidate)? - anchor).num_days() / 7;
                if weeks.rem_euclid(i64::from(rule.interval)) == 0
                    && rule.days.iter().any(|d| d.weekday == candidate.weekday())
                {
                    return Some(candidate);
                }
                candidate = candidate.succ_opt()?;
            }
            None
        }
        Freq::Monthly if rule.month.is_none() => {
            // BYMONTHDAY and BYDAY together mean their intersection (a
            // Friday the 13th); computing that is not worth the risk of a
            // wrong date reaching the phones.
            if rule.month_day.is_some() && !rule.days.is_empty() {
                return None;
            }
            for step in 0..=48u32 {
                let (year, month) =
                    shift_month(from.year(), from.month(), step.checked_mul(rule.interval)?)?;
                if let Some(next) = month_candidates(rule, year, month, from.day())
                    .into_iter()
                    .find(|d| *d > from)
                {
                    return Some(next);
                }
            }
            None
        }
        Freq::Yearly if rule.days.is_empty() => {
            let month = rule.month.unwrap_or(from.month());
            let day = rule
                .month_day
                .unwrap_or_else(|| i32::try_from(from.day()).unwrap_or(1));
            for step in 0..=200u32 {
                let years = i32::try_from(step.checked_mul(rule.interval)?).ok()?;
                let year = from.year().checked_add(years)?;
                if let Some(next) = month_date(year, month, day).filter(|d| *d > from) {
                    return Some(next);
                }
            }
            None
        }
        _ => None,
    }
}

/// The candidate occurrences inside one month, sorted.
fn month_candidates(rule: &Rule, year: i32, month: u32, fallback_day: u32) -> Vec<NaiveDate> {
    if let Some(day) = rule.month_day {
        return month_date(year, month, day).into_iter().collect();
    }
    if !rule.days.is_empty() {
        let mut out: Vec<NaiveDate> = Vec::new();
        for day in &rule.days {
            match day.ordinal {
                Some(n) => out.extend(nth_weekday(year, month, day.weekday, n)),
                None => out.extend(weekdays_in(year, month, day.weekday)),
            }
        }
        out.sort_unstable();
        out.dedup();
        return out;
    }
    month_date(year, month, i32::try_from(fallback_day).unwrap_or(1))
        .into_iter()
        .collect()
}

/// Day `day` of a month, `-1` for its last day; a month without that day
/// yields nothing (no 31 February).
fn month_date(year: i32, month: u32, day: i32) -> Option<NaiveDate> {
    if day == -1 {
        let (next_year, next_month) = shift_month(year, month, 1)?;
        return NaiveDate::from_ymd_opt(next_year, next_month, 1)?.pred_opt();
    }
    NaiveDate::from_ymd_opt(year, month, u32::try_from(day).ok()?)
}

/// The `n`th (`-1` last, `-2` second last) given weekday of a month.
fn nth_weekday(year: i32, month: u32, weekday: Weekday, n: i32) -> Option<NaiveDate> {
    let all = weekdays_in(year, month, weekday);
    if n > 0 {
        all.get(usize::try_from(n).ok()?.checked_sub(1)?).copied()
    } else {
        all.len()
            .checked_sub(usize::try_from(-n).ok()?)
            .and_then(|i| all.get(i).copied())
    }
}

fn weekdays_in(year: i32, month: u32, weekday: Weekday) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut day = NaiveDate::from_ymd_opt(year, month, 1);
    while let Some(d) = day.filter(|d| d.month() == month) {
        if d.weekday() == weekday {
            out.push(d);
        }
        day = d.succ_opt();
    }
    out
}

/// `months` months after a (year, month) pair.
fn shift_month(year: i32, month: u32, months: u32) -> Option<(i32, u32)> {
    let total = i64::from(year) * 12 + i64::from(month) - 1 + i64::from(months);
    Some((
        i32::try_from(total.div_euclid(12)).ok()?,
        u32::try_from(total.rem_euclid(12)).ok()? + 1,
    ))
}

/// The Monday a date's week starts on.
fn week_start(date: NaiveDate) -> Option<NaiveDate> {
    date.checked_sub_days(Days::new(u64::from(date.weekday().num_days_from_monday())))
}

/// The rule with its `COUNT` replaced, everything else byte for byte.
fn with_count(rule: &str, count: u32) -> String {
    rule.split(';')
        .map(|part| match part.split_once('=') {
            Some((key, _)) if key.trim().eq_ignore_ascii_case("COUNT") => {
                format!("COUNT={count}")
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}
