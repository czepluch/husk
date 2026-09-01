//! Core of husk: task model, iCalendar codec and storage layer.
//! The binary in `main.rs` and the tests in `tests/` build on this crate.

pub mod alarms;
pub mod config;
pub mod ical;
pub mod model;
pub mod notify;
pub mod quickadd;
pub mod store;
pub mod sync;
pub mod theme;
pub mod ui;
