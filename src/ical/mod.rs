//! iCalendar support: the content-line codec, the VTODO to Task mapping,
//! time zones and durations.

pub mod codec;
pub mod duration;
pub mod tz;
pub mod vtodo;

// Value formats: DATE, floating or zoned DATE-TIME, and UTC DATE-TIME.
const DATE: &str = "%Y%m%d";
const DATE_TIME: &str = "%Y%m%dT%H%M%S";
const UTC_DATE_TIME: &str = "%Y%m%dT%H%M%SZ";
