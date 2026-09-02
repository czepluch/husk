//! Mapping between the codec's component tree and `Task`.
//!
//! Reading is total: any VTODO with a UID yields a task, and anything odd
//! degrades to a default rather than an error. Writing patches only the
//! properties whose model value differs from what the file already says, so
//! every other byte, including properties husk does not model, survives.
//! `apply` honours `summary`, `description`, `due`, `status`, `completed`,
//! `priority`, `tags` and `alarms`. `start`, `rrule`, `parent`, `created` and
//! `uid` are read only; `project` changes through the store's `move_to`.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{
    DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone,
    Utc,
};
use chrono_tz::{OffsetComponents, OffsetName, Tz};

use crate::ical::codec::{self, Component, Document, Entry, Param, Property};
use crate::model::{Alarm, Anchor, Due, NewTask, Priority, ProjectId, Status, Task};

const DATE: &str = "%Y%m%d";
const DATE_TIME: &str = "%Y%m%dT%H%M%S";
const UTC_DATE_TIME: &str = "%Y%m%dT%H%M%SZ";

fn prodid() -> String {
    format!("-//husk//husk {}//EN", env!("CARGO_PKG_VERSION"))
}

/// Parses a whole `.ics` file into a task of the given project.
pub fn parse_task(text: &str, project: ProjectId) -> Result<Task> {
    from_document(codec::parse(text)?, project)
}

pub fn from_document(raw: Document, project: ProjectId) -> Result<Task> {
    let todo = raw.root.child("VTODO").context("no VTODO component")?;
    let uid = todo
        .prop("UID")
        .map(Property::text)
        .filter(|u| !u.trim().is_empty())
        .context("VTODO without UID")?;
    let summary = todo.prop("SUMMARY").map(Property::text).unwrap_or_default();
    let description = todo
        .prop("DESCRIPTION")
        .map(Property::text)
        .filter(|d| !d.is_empty());
    let due = todo.prop("DUE").and_then(parse_due);
    let start = todo.prop("DTSTART").and_then(parse_due);
    let status = todo
        .prop("STATUS")
        .and_then(|p| Status::from_ical(&p.value))
        .unwrap_or(Status::NeedsAction);
    let completed = todo.prop("COMPLETED").and_then(|p| parse_utc(&p.value));
    let priority = todo
        .prop("PRIORITY")
        .and_then(|p| p.value.trim().parse().ok())
        .map_or(Priority::None, Priority::from_ical);
    let tags = todo
        .props("CATEGORIES")
        .flat_map(|p| split_list(&p.value))
        .filter(|t| !t.is_empty())
        .collect();
    let alarms = todo
        .children()
        .filter(|c| c.is("VALARM"))
        .filter_map(parse_alarm)
        .collect();
    let rrule = todo
        .prop("RRULE")
        .map(|p| p.value.trim().to_string())
        .filter(|r| !r.is_empty());
    let parent = todo
        .props("RELATED-TO")
        .find(|p| {
            p.param("RELTYPE")
                .is_none_or(|r| r.eq_ignore_ascii_case("PARENT"))
        })
        .map(Property::text);
    let created = todo.prop("CREATED").and_then(|p| parse_utc(&p.value));

    Ok(Task {
        uid,
        project,
        summary,
        description,
        due,
        start,
        status,
        completed,
        priority,
        tags,
        alarms,
        rrule,
        parent,
        created,
        raw,
    })
}

/// The zone new timed values are written in: `TZ` when it names a zone
/// chrono-tz knows, else the system zone. `None` when neither does, in
/// which case values are written in UTC.
pub fn local_zone() -> Option<Tz> {
    let env = std::env::var("TZ").ok();
    let system = iana_time_zone::get_timezone().ok();
    zone_named(env.as_deref(), system.as_deref())
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
fn ensure_vtimezone(root: &mut Component, tz: Tz, year: i32) {
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

/// The task's file with the model's changes patched in. Nothing else moves,
/// and a task that was not changed yields a document equal to its raw one.
pub fn apply(task: &Task) -> Result<Document> {
    apply_in(task, local_zone())
}

/// `apply` with the zone for new timed values given explicitly.
pub fn apply_in(task: &Task, zone: Option<Tz>) -> Result<Document> {
    let original = from_document(task.raw.clone(), task.project.clone())?;
    let mut doc = task.raw.clone();
    let todo = doc.root.child_mut("VTODO").context("no VTODO component")?;

    if task.summary != original.summary {
        todo.set_text("SUMMARY", &task.summary);
    }
    if task.description != original.description {
        match &task.description {
            Some(text) => todo.set_text("DESCRIPTION", text),
            None => {
                todo.remove("DESCRIPTION");
            }
        }
    }
    let mut zone_used = None;
    if task.due != original.due {
        zone_used = set_due(todo, task.due, zone);
    }
    if task.status != original.status || task.completed != original.completed {
        set_status(todo, task.status, task.completed);
    }
    if task.priority != original.priority {
        match task.priority {
            Priority::None => {
                todo.remove("PRIORITY");
            }
            p => todo.set(Property::new("PRIORITY", p.to_ical().to_string())),
        }
    }
    if task.tags != original.tags {
        set_tags(todo, &task.tags);
    }
    if task.alarms != original.alarms {
        set_alarms(todo, &task.alarms, &task.summary);
    }
    if let (Some(tz), Some(Due::DateTime(at))) = (zone_used, task.due) {
        ensure_vtimezone(&mut doc.root, tz, at.with_timezone(&tz).year());
    }
    Ok(doc)
}

/// The `SEQUENCE` a document carries; missing counts as 0.
pub fn sequence(doc: &Document) -> u64 {
    doc.root
        .child("VTODO")
        .and_then(|todo| todo.prop("SEQUENCE"))
        .and_then(|p| p.value.trim().parse().ok())
        .unwrap_or(0)
}

pub fn set_sequence(doc: &mut Document, sequence: u64) -> Result<()> {
    let todo = doc.root.child_mut("VTODO").context("no VTODO component")?;
    todo.set(Property::new("SEQUENCE", sequence.to_string()));
    Ok(())
}

/// Marks a rewrite the way phone clients expect: `SEQUENCE` up by one
/// (missing counts as 0), `LAST-MODIFIED` and `DTSTAMP` set to now.
pub fn bump(doc: &mut Document, now: DateTime<Utc>) -> Result<()> {
    let sequence = sequence(doc);
    let todo = doc.root.child_mut("VTODO").context("no VTODO component")?;
    todo.set(Property::new(
        "SEQUENCE",
        sequence.saturating_add(1).to_string(),
    ));
    let stamp = now.format(UTC_DATE_TIME).to_string();
    todo.set(Property::new("LAST-MODIFIED", stamp.clone()));
    todo.set(Property::new("DTSTAMP", stamp));
    Ok(())
}

/// A complete file for a new task. Timed due dates are written in the local
/// zone with a VTIMEZONE, the form Apple Reminders shows as plain local
/// time; a UTC value it shows with a GMT label.
pub fn new_document(new: &NewTask, uid: &str, now: DateTime<Utc>) -> Document {
    new_document_in(new, uid, now, local_zone())
}

/// `new_document` with the zone for timed values given explicitly.
pub fn new_document_in(new: &NewTask, uid: &str, now: DateTime<Utc>, zone: Option<Tz>) -> Document {
    let stamp = now.format(UTC_DATE_TIME).to_string();
    let mut todo = Component::new("VTODO");
    todo.set(Property::new("UID", uid));
    todo.set(Property::new("DTSTAMP", stamp.clone()));
    todo.set(Property::new("CREATED", stamp.clone()));
    todo.set(Property::new("LAST-MODIFIED", stamp));
    todo.set(Property::new("SEQUENCE", "0"));
    todo.set(Property::new("STATUS", Status::NeedsAction.as_ical()));
    todo.set(Property::text_value("SUMMARY", &new.summary));
    if let Some(text) = &new.description {
        todo.set(Property::text_value("DESCRIPTION", text));
    }
    if let Some(due) = new.due {
        todo.set(due_property("DUE", due, None, zone));
    }
    if new.priority != Priority::None {
        todo.set(Property::new(
            "PRIORITY",
            new.priority.to_ical().to_string(),
        ));
    }
    if !new.tags.is_empty() {
        todo.set(Property::new("CATEGORIES", join_list(&new.tags)));
    }
    for alarm in &new.alarms {
        todo.push_child(alarm_component(alarm, &new.summary));
    }

    let mut root = Component::new("VCALENDAR");
    root.set(Property::new("VERSION", "2.0"));
    root.set(Property::new("PRODID", prodid()));
    if let (Some(tz), Some(Due::DateTime(at))) = (zone, new.due) {
        root.push_child(vtimezone(tz, at.with_timezone(&tz).year()));
    }
    root.push_child(todo);
    Document::new(root)
}

fn alarm_component(alarm: &Alarm, summary: &str) -> Component {
    let mut c = Component::new("VALARM");
    c.set(Property::new("ACTION", "DISPLAY"));
    c.set(Property::text_value("DESCRIPTION", summary));
    c.set(match alarm {
        Alarm::Absolute(at) => Property::new("TRIGGER", at.format(UTC_DATE_TIME).to_string())
            .with_param("VALUE", "DATE-TIME"),
        Alarm::Relative { offset, anchor } => {
            let trigger = Property::new("TRIGGER", format_duration(*offset));
            // RFC 5545 defaults RELATED to START, and husk never adds a DTSTART.
            match anchor {
                Anchor::Start => trigger.with_param("RELATED", "START"),
                Anchor::Due => trigger.with_param("RELATED", "END"),
            }
        }
    });
    c
}

/// One CATEGORIES line, in the place of the first one the file had.
fn set_tags(todo: &mut Component, tags: &[String]) {
    if tags.is_empty() {
        todo.remove("CATEGORIES");
        return;
    }
    let mut seen = false;
    todo.entries.retain(|e| {
        if !matches!(e, Entry::Property(p) if p.is("CATEGORIES")) {
            return true;
        }
        let keep = !seen;
        seen = true;
        keep
    });
    match todo.prop_mut("CATEGORIES") {
        Some(existing) => existing.value = join_list(tags),
        None => todo.set(Property::new("CATEGORIES", join_list(tags))),
    }
}

/// Keeps every VALARM whose trigger is still wanted, byte for byte, drops
/// the rest and appends the new ones. Phone clients keep metadata in their
/// alarms (Apple a UID, Tasks.org REPEAT and DURATION) that the model does
/// not carry, and an alarm the model cannot read is left alone.
fn set_alarms(todo: &mut Component, alarms: &[Alarm], summary: &str) {
    let mut wanted: Vec<Alarm> = alarms.to_vec();
    todo.entries.retain(|e| {
        let Entry::Component(c) = e else {
            return true;
        };
        if !c.is("VALARM") {
            return true;
        }
        match parse_alarm(c) {
            None => true,
            Some(alarm) => match wanted.iter().position(|w| *w == alarm) {
                Some(i) => {
                    wanted.remove(i);
                    true
                }
                None => false,
            },
        }
    });
    for alarm in wanted {
        todo.push_child(alarm_component(&alarm, summary));
    }
}

fn set_due(todo: &mut Component, due: Option<Due>, zone: Option<Tz>) -> Option<Tz> {
    let existing = todo.prop("DUE").cloned();
    let start = todo.prop("DTSTART").cloned();
    // Apple writes DTSTART equal to DUE, so it follows DUE. A separate start
    // (Tasks.org) is left alone unless it would end up after the due date.
    let start_follows = matches!(
        (&existing, &start),
        (Some(d), Some(s)) if d.value == s.value && d.params == s.params
    );
    match due {
        None => {
            todo.remove("DUE");
            if start_follows {
                todo.remove("DTSTART");
            }
            None
        }
        Some(due) => {
            let prop = due_property("DUE", due, existing.as_ref(), zone);
            let start_after_due = start
                .as_ref()
                .and_then(parse_due)
                .is_some_and(|s| is_after(s, due));
            if start_follows || start_after_due {
                let mut moved = prop.clone();
                moved.name = "DTSTART".to_string();
                todo.set(moved);
            }
            let written_zone = prop.param("TZID").and_then(|name| Tz::from_str(name).ok());
            todo.set(prop);
            written_zone
        }
    }
}

fn set_status(todo: &mut Component, status: Status, completed: Option<DateTime<Utc>>) {
    todo.set(Property::new("STATUS", status.as_ical()));
    match (status, completed) {
        // Apple marks completion with all three; writing fewer confuses it.
        (Status::Completed, Some(at)) => {
            todo.set(Property::new(
                "COMPLETED",
                at.format(UTC_DATE_TIME).to_string(),
            ));
            todo.set(Property::new("PERCENT-COMPLETE", "100"));
        }
        (Status::Completed, None) => {}
        _ => {
            todo.remove("COMPLETED");
            todo.remove("PERCENT-COMPLETE");
        }
    }
}

/// A DUE or DTSTART property in the form the file already uses for it: a
/// TZID with wall-clock time, or floating local time. A new timed value,
/// or one the file had in UTC, is written in `zone` (UTC when there is
/// none). Parameters other than VALUE and TZID are kept.
fn due_property(name: &str, due: Due, existing: Option<&Property>, zone: Option<Tz>) -> Property {
    let mut params: Vec<Param> = existing
        .map(|p| {
            p.params
                .iter()
                .filter(|param| !is_param(param, "VALUE") && !is_param(param, "TZID"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let value = match due {
        Due::Date(date) => {
            params.push(Param {
                name: "VALUE".to_string(),
                value: "DATE".to_string(),
            });
            date.format(DATE).to_string()
        }
        Due::DateTime(utc) => {
            let timed = existing.filter(|p| p.value.contains('T'));
            if let Some(explicit) = timed
                .and_then(|p| p.params.iter().find(|param| is_param(param, "VALUE")))
                .filter(|param| param.value.eq_ignore_ascii_case("DATE-TIME"))
            {
                params.push(explicit.clone());
            }
            let tzid = timed.and_then(|p| p.params.iter().find(|param| is_param(param, "TZID")));
            let known = tzid.and_then(|param| {
                Tz::from_str(param.value.trim_matches('"'))
                    .ok()
                    .map(|tz| (param, tz))
            });
            let floating = timed.is_some_and(|p| !p.value.ends_with('Z')) && tzid.is_none();
            let zoned = match known {
                Some((param, tz)) => Some((param.clone(), tz)),
                None => zone.filter(|_| !floating).map(|tz| {
                    let param = Param {
                        name: "TZID".to_string(),
                        value: tz.name().to_string(),
                    };
                    (param, tz)
                }),
            };
            match zoned {
                Some((param, tz)) => match nameable_wall(utc, tz) {
                    Some(wall) => {
                        params.push(param);
                        wall.format(DATE_TIME).to_string()
                    }
                    None => utc.format(UTC_DATE_TIME).to_string(),
                },
                None if floating => utc
                    .with_timezone(&Local)
                    .naive_local()
                    .format(DATE_TIME)
                    .to_string(),
                None => utc.format(UTC_DATE_TIME).to_string(),
            }
        }
    };
    Property {
        name: name.to_string(),
        params,
        value,
    }
}

/// The wall time of an instant in a zone, when reading it back gives the
/// same instant. RFC 5545 reads a wall time in the hour repeated when
/// daylight time ends as its first pass, so the second pass has no name.
fn nameable_wall(utc: DateTime<Utc>, tz: Tz) -> Option<NaiveDateTime> {
    let wall = utc.with_timezone(&tz).naive_local();
    (tz.from_local_datetime(&wall).earliest()? == utc).then_some(wall)
}

fn is_param(param: &Param, name: &str) -> bool {
    param.name.eq_ignore_ascii_case(name)
}

fn is_after(start: Due, due: Due) -> bool {
    match (start, due) {
        (Due::DateTime(s), Due::DateTime(d)) => s > d,
        (s, d) => local_date(s) > local_date(d),
    }
}

fn local_date(due: Due) -> NaiveDate {
    match due {
        Due::Date(d) => d,
        Due::DateTime(t) => t.with_timezone(&Local).date_naive(),
    }
}

fn parse_due(p: &Property) -> Option<Due> {
    let value = p.value.trim();
    let is_date = p
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
    if is_date || !value.contains('T') {
        return NaiveDate::parse_from_str(value, DATE).ok().map(Due::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        return NaiveDateTime::parse_from_str(utc, DATE_TIME)
            .ok()
            .map(|n| Due::DateTime(n.and_utc()));
    }
    let wall = NaiveDateTime::parse_from_str(value, DATE_TIME).ok()?;
    // A TZID chrono-tz does not know, or none at all (floating), means local.
    let instant = match p.param("TZID").and_then(|z| Tz::from_str(z).ok()) {
        Some(tz) => wall_to_utc(&tz, wall),
        None => wall_to_utc(&Local, wall),
    };
    Some(Due::DateTime(instant))
}

/// In a DST gap the wall time does not exist; it is moved forward by the
/// hour the clocks skipped, which keeps it after the times before the gap.
/// An ambiguous time (clocks set back) takes its first occurrence.
fn wall_to_utc<Z: TimeZone>(zone: &Z, wall: NaiveDateTime) -> DateTime<Utc> {
    zone.from_local_datetime(&wall)
        .earliest()
        .or_else(|| {
            zone.from_local_datetime(&(wall + TimeDelta::hours(1)))
                .earliest()
        })
        .map_or_else(|| wall.and_utc(), |dt| dt.with_timezone(&Utc))
}

fn parse_utc(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    NaiveDateTime::parse_from_str(value.strip_suffix('Z').unwrap_or(value), DATE_TIME)
        .ok()
        .map(|n| n.and_utc())
}

fn parse_alarm(c: &Component) -> Option<Alarm> {
    let trigger = c.prop("TRIGGER")?;
    let absolute = trigger
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME"))
        || trigger.value.trim().ends_with('Z');
    if absolute {
        return parse_utc(&trigger.value).map(Alarm::Absolute);
    }
    let offset = parse_duration(&trigger.value)?;
    let anchor = if trigger
        .param("RELATED")
        .is_some_and(|r| r.eq_ignore_ascii_case("START"))
    {
        Anchor::Start
    } else {
        Anchor::Due
    };
    Some(Alarm::Relative { offset, anchor })
}

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

/// Splits a comma-separated value on unescaped commas and unescapes the parts.
fn split_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' => items.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|raw| codec::unescape_text(raw.trim()))
        .collect()
}

fn join_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| codec::escape_text(item))
        .collect::<Vec<_>>()
        .join(",")
}
