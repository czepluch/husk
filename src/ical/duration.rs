//! RFC 5545 durations, as alarm triggers use them.

use chrono::TimeDelta;

/// Parses an RFC 5545 duration such as `-PT15M`, `P1D` or `P2W`. Values
/// that do not fit in a `TimeDelta` are rejected rather than overflowed.
pub fn parse_duration(value: &str) -> Option<TimeDelta> {
    let value = value.trim();
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut seconds: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    let mut components = 0;
    let mut time_components = 0;
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' if !in_time => in_time = true,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                let unit = match (c, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
                seconds = seconds.checked_add(n.checked_mul(unit)?)?;
                components += 1;
                if in_time {
                    time_components += 1;
                }
            }
            _ => return None,
        }
    }
    if !number.is_empty() || components == 0 || (in_time && time_components == 0) {
        return None;
    }
    TimeDelta::try_seconds(if negative { -seconds } else { seconds })
}

/// Formats a duration the way `parse_duration` reads it; zero is `PT0S`.
pub fn format_duration(duration: TimeDelta) -> String {
    let total = duration.num_seconds();
    let mut rest = total.abs();
    let days = rest / 86_400;
    rest %= 86_400;
    let hours = rest / 3_600;
    rest %= 3_600;
    let minutes = rest / 60;
    let seconds = rest % 60;

    let mut out = String::from(if total < 0 { "-P" } else { "P" });
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if hours > 0 || minutes > 0 || seconds > 0 || days == 0 {
        out.push('T');
        if hours > 0 {
            out.push_str(&format!("{hours}H"));
        }
        if minutes > 0 {
            out.push_str(&format!("{minutes}M"));
        }
        if seconds > 0 || (hours == 0 && minutes == 0) {
            out.push_str(&format!("{seconds}S"));
        }
    }
    out
}
