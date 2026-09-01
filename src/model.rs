//! The task model: a small typed view over one VTODO plus the collection it
//! lives in. Everything husk does not model stays inside the raw document.

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};

use crate::ical::codec::Document;

/// A vdir collection, identified by its directory name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub String);

impl ProjectId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    /// From the vdir `displayname` file, falling back to the directory name.
    pub name: String,
    /// From the vdir `color` file, as written (usually `#RRGGBB`).
    pub color: Option<String>,
}

/// A project by directory name or display name, case-insensitively.
pub fn find_project<'a>(projects: &'a [Project], name: &str) -> Option<&'a Project> {
    let wanted = name.trim().to_lowercase();
    projects
        .iter()
        .find(|p| p.id.as_str().to_lowercase() == wanted || p.name.to_lowercase() == wanted)
}

/// A project's display name, or its directory name when it is not listed.
pub fn project_name(projects: &[Project], id: &ProjectId) -> String {
    projects
        .iter()
        .find(|p| &p.id == id)
        .map_or_else(|| id.as_str().to_string(), |p| p.name.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NeedsAction,
    InProcess,
    Completed,
    Cancelled,
}

impl Status {
    pub fn as_ical(self) -> &'static str {
        match self {
            Self::NeedsAction => "NEEDS-ACTION",
            Self::InProcess => "IN-PROCESS",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub fn from_ical(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "NEEDS-ACTION" => Some(Self::NeedsAction),
            "IN-PROCESS" => Some(Self::InProcess),
            "COMPLETED" => Some(Self::Completed),
            "CANCELLED" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Ordered so that sorting ascending puts the most urgent first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,
    Medium,
    Low,
    #[default]
    None,
}

impl Priority {
    /// RFC 5545: 0 undefined, 1 to 4 high, 5 medium, 6 to 9 low.
    pub fn from_ical(value: u8) -> Self {
        match value {
            1..=4 => Self::High,
            5 => Self::Medium,
            6..=9 => Self::Low,
            _ => Self::None,
        }
    }

    /// The values both phone apps write.
    pub fn to_ical(self) -> u8 {
        match self {
            Self::High => 1,
            Self::Medium => 5,
            Self::Low => 9,
            Self::None => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
}

/// What a relative alarm trigger counts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Due,
    Start,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarm {
    Absolute(DateTime<Utc>),
    Relative { offset: TimeDelta, anchor: Anchor },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub uid: String,
    pub project: ProjectId,
    pub summary: String,
    pub description: Option<String>,
    pub due: Option<Due>,
    /// `DTSTART`, read only. Apple writes it equal to `DUE`; Tasks.org writes
    /// a separate start date and anchors alarms on it.
    pub start: Option<Due>,
    pub status: Status,
    pub completed: Option<DateTime<Utc>>,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub alarms: Vec<Alarm>,
    /// The `RRULE` as written; described, never expanded.
    pub rrule: Option<String>,
    /// UID of the parent task, from `RELATED-TO`.
    pub parent: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub(crate) raw: Document,
}

impl Task {
    /// The file this task was read from, untouched.
    pub fn raw(&self) -> &Document {
        &self.raw
    }

    pub fn is_done(&self) -> bool {
        matches!(self.status, Status::Completed | Status::Cancelled)
    }

    pub fn is_recurring(&self) -> bool {
        self.rrule.is_some()
    }

    pub fn complete(&mut self, now: DateTime<Utc>) {
        self.status = Status::Completed;
        self.completed = Some(now);
    }

    pub fn reopen(&mut self) {
        self.status = Status::NeedsAction;
        self.completed = None;
    }
}

/// What a caller supplies to create a task; the store fills in the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewTask {
    pub summary: String,
    pub description: Option<String>,
    pub due: Option<Due>,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub alarms: Vec<Alarm>,
}
