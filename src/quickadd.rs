//! The quick-add grammar: one line, Taskwarrior-shaped, parsed by a small
//! tokenizer. `due:`, `pri:`, `+tag` and `@project` are recognized; every
//! other token stays in the summary and parsing never fails.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, TimeDelta, TimeZone, Weekday};

use crate::model::{Due, Priority};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickAdd {
    pub summary: String,
    pub due: Option<Due>,
    pub priority: Priority,
    pub tags: Vec<String>,
    /// `@project` as typed; matching it to a real project is the caller's job.
    pub project: Option<String>,
}

pub fn parse(input: &str, now: DateTime<Local>) -> QuickAdd {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut out = QuickAdd::default();
    let mut summary: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if let Some(tag) = token.strip_prefix('+').filter(|t| !t.is_empty()) {
            out.tags.push(tag.to_string());
            i += 1;
            continue;
        }
        if let Some(project) = token.strip_prefix('@').filter(|p| !p.is_empty()) {
            out.project = Some(project.to_string());
            i += 1;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if let Some(priority) = lower.strip_prefix("pri:").and_then(parse_priority) {
            out.priority = priority;
            i += 1;
            continue;
        }
        if let Some(value) = lower.strip_prefix("due:")
            && let Some((due, consumed)) = parse_due(value, &tokens[i + 1..], now)
        {
            out.due = Some(due);
            i += 1 + consumed;
            continue;
        }
        summary.push(token);
        i += 1;
    }
    out.summary = summary.join(" ");
    out
}

fn parse_priority(value: &str) -> Option<Priority> {
    match value.to_ascii_lowercase().as_str() {
        "h" | "high" => Some(Priority::High),
        "m" | "med" | "medium" => Some(Priority::Medium),
        "l" | "low" => Some(Priority::Low),
        _ => None,
    }
}

/// The due date, plus how many of the following tokens were consumed
/// (a weekday after `next`, an `HH:MM` after any date).
fn parse_due(value: &str, rest: &[&str], now: DateTime<Local>) -> Option<(Due, usize)> {
    // A bare time means today.
    if let Some(time) = parse_time(value) {
        return Some((at(now.date_naive(), time), 0));
    }
    let (date, mut consumed) = parse_date(value, rest, now)?;
    if let Some(time) = rest.get(consumed).and_then(|t| parse_time(t)) {
        consumed += 1;
        return Some((at(date, time), consumed));
    }
    Some((Due::Date(date), consumed))
}

fn parse_date(value: &str, rest: &[&str], now: DateTime<Local>) -> Option<(NaiveDate, usize)> {
    let today = now.date_naive();
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "today" | "tod" => return Some((today, 0)),
        "tomorrow" | "tom" | "tmrw" => return Some((today + TimeDelta::days(1), 0)),
        // "next mon" reads naturally; it means the same as a bare weekday.
        "next" => {
            let weekday = rest.first().and_then(|t| parse_weekday(t))?;
            return Some((next_weekday(today, weekday), 1));
        }
        _ => {}
    }
    if let Some(weekday) = parse_weekday(&value) {
        return Some((next_weekday(today, weekday), 0));
    }
    if let Some(days) = parse_relative(&value) {
        return Some((today + TimeDelta::days(days), 0));
    }
    NaiveDate::parse_from_str(&value, "%Y-%m-%d")
        .ok()
        .map(|date| (date, 0))
}

/// `+2d` or `+1w`, as days from today.
fn parse_relative(value: &str) -> Option<i64> {
    let rest = value.strip_prefix('+')?;
    let unit = match rest.chars().last()? {
        'd' => 1,
        'w' => 7,
        _ => return None,
    };
    let n: i64 = rest[..rest.len() - 1].parse().ok()?;
    n.checked_mul(unit)
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    value.parse::<Weekday>().ok()
}

/// The next occurrence of a weekday, never today.
fn next_weekday(today: NaiveDate, weekday: Weekday) -> NaiveDate {
    let ahead = (weekday.num_days_from_monday() + 7 - today.weekday().num_days_from_monday()) % 7;
    today + TimeDelta::days(if ahead == 0 { 7 } else { i64::from(ahead) })
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    if !value.contains(':') {
        return None;
    }
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

/// A wall-clock time today or on a given date, as an instant. A time inside
/// a DST gap is moved forward by the skipped hour, like everywhere else.
fn at(date: NaiveDate, time: NaiveTime) -> Due {
    let wall = date.and_time(time);
    let instant = Local
        .from_local_datetime(&wall)
        .earliest()
        .or_else(|| {
            Local
                .from_local_datetime(&(wall + TimeDelta::hours(1)))
                .earliest()
        })
        .map_or_else(|| wall.and_utc(), |local| local.to_utc());
    Due::DateTime(instant)
}
