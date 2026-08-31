mod common;

use husk::ical::codec::{self, Component, Document, Entry, LineEnding, Property};

fn parse(text: &str) -> Document {
    codec::parse(text).unwrap_or_else(|e| panic!("parse failed: {e:#}"))
}

fn entry_names(c: &Component) -> Vec<&str> {
    c.entries
        .iter()
        .map(|e| match e {
            Entry::Property(p) => p.name.as_str(),
            Entry::Component(c) => c.name.as_str(),
        })
        .collect()
}

fn first_difference(a: &str, b: &str) -> String {
    a.lines()
        .zip(b.lines())
        .enumerate()
        .find(|(_, (x, y))| x != y)
        .map(|(i, (x, y))| format!("line {}: expected {x:?}, got {y:?}", i + 1))
        .unwrap_or_else(|| format!("lengths differ: {} vs {}", a.len(), b.len()))
}

#[test]
fn fixtures_round_trip_byte_identical() {
    for (name, text) in common::fixtures() {
        let doc = codec::parse(&text).unwrap_or_else(|e| panic!("{}: {e:#}", name.display()));
        let out = codec::serialize(&doc);
        assert!(
            out == text,
            "{} changed on round trip: {}",
            name.display(),
            first_difference(&text, &out)
        );
    }
}

#[test]
fn fixtures_round_trip_modulo_folding() {
    for (name, text) in common::fixtures() {
        let out = codec::serialize(&parse(&text));
        assert_eq!(
            codec::unfold(&out),
            codec::unfold(&text),
            "{}",
            name.display()
        );
    }
}

#[test]
fn unfold_joins_continuation_lines() {
    assert_eq!(
        codec::unfold("A:b\r\n c\r\n\td\r\nX:y\r\n"),
        "A:bcd\r\nX:y\r\n"
    );
    assert_eq!(codec::unfold("A:b\n c\nX:y\n"), "A:bc\nX:y\n");
    assert_eq!(codec::unfold("A:b\r\nX:y"), "A:b\r\nX:y");
}

#[test]
fn folds_long_lines_at_75_octets_on_char_boundaries() {
    let value = "æøå ".repeat(40);
    let mut root = Component::new("VCALENDAR");
    root.set(Property::new("DESCRIPTION", value.clone()));
    let out = codec::serialize(&Document::new(root));

    let physical: Vec<&str> = out.lines().collect();
    assert!(physical.len() > 4, "expected folding, got {physical:?}");
    for line in &physical {
        assert!(line.len() <= 75, "physical line over 75 octets: {line:?}");
    }
    assert!(physical[1].starts_with("DESCRIPTION:"));
    assert!(physical[2].starts_with(' '));
    assert_eq!(
        codec::unfold(&out),
        format!("BEGIN:VCALENDAR\r\nDESCRIPTION:{value}\r\nEND:VCALENDAR\r\n")
    );
}

#[test]
fn lines_of_exactly_75_octets_are_not_folded() {
    let fits = "x".repeat(75 - "DESCRIPTION:".len());
    let mut root = Component::new("VCALENDAR");
    root.set(Property::new("DESCRIPTION", fits.clone()));
    let out = codec::serialize(&Document::new(root));
    assert!(out.contains(&format!("DESCRIPTION:{fits}\r\nEND:VCALENDAR")));

    let mut root = Component::new("VCALENDAR");
    root.set(Property::new("DESCRIPTION", format!("{fits}y")));
    let out = codec::serialize(&Document::new(root));
    assert!(out.contains(&format!("DESCRIPTION:{fits}\r\n y\r\nEND:VCALENDAR")));
}

#[test]
fn parses_parameters_with_quoted_values() {
    let doc = parse(
        "BEGIN:VCALENDAR\r\nATTENDEE;CN=\"Doe, John; Jr\";ROLE=REQ-PARTICIPANT:mailto:john@example.com\r\nEND:VCALENDAR\r\n",
    );
    let p = doc.root.prop("ATTENDEE").expect("ATTENDEE");
    assert_eq!(p.param("CN"), Some("Doe, John; Jr"));
    assert_eq!(p.param("role"), Some("REQ-PARTICIPANT"));
    assert_eq!(p.param("missing"), None);
    assert_eq!(p.value, "mailto:john@example.com");
    assert_eq!(p.params.len(), 2);
}

#[test]
fn text_escaping_round_trips() {
    let text = "a,b;c\\d\ne";
    assert_eq!(codec::escape_text(text), r"a\,b\;c\\d\ne");
    assert_eq!(codec::unescape_text(&codec::escape_text(text)), text);
    assert_eq!(codec::escape_text("crlf\r\nhere"), "crlf\\nhere");
    assert_eq!(codec::unescape_text("a\\Nb"), "a\nb");
    assert_eq!(codec::unescape_text("keep\\x"), "keep\\x");
    assert_eq!(
        codec::unescape_text(r"Her\nEr\nEn note\,\n\;\;\;\n#tag\n"),
        "Her\nEr\nEn note,\n;;;\n#tag\n"
    );
}

#[test]
fn property_text_accessor_unescapes_fixture_notes() {
    let doc = parse(&common::fixture("apple/notes-priority-low.ics"));
    let vtodo = doc.root.child("VTODO").expect("VTODO");
    assert_eq!(
        vtodo.prop("DESCRIPTION").expect("DESCRIPTION").text(),
        "Her\nEr\nEn note,\n;;;\n#tag\n"
    );
    assert_eq!(
        vtodo.prop("summary").expect("SUMMARY").text(),
        "Afhænger #tets"
    );
}

#[test]
fn line_ending_is_detected_and_preserved() {
    let lf = "BEGIN:VCALENDAR\nX:1\nEND:VCALENDAR\n";
    let doc = parse(lf);
    assert_eq!(doc.line_ending, LineEnding::Lf);
    assert_eq!(codec::serialize(&doc), lf);

    let crlf = lf.replace('\n', "\r\n");
    let doc = parse(&crlf);
    assert_eq!(doc.line_ending, LineEnding::Crlf);
    assert_eq!(codec::serialize(&doc), crlf);

    let fresh = Document::new(Component::new("VCALENDAR"));
    assert_eq!(
        codec::serialize(&fresh),
        "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n"
    );
}

#[test]
fn properties_after_child_components_keep_their_place() {
    let doc = parse(&common::fixture("tasksorg/timed-alarms-repeat.ics"));
    let vtodo = doc.root.child("VTODO").expect("VTODO");
    let names = entry_names(vtodo);
    assert_eq!(names.last(), Some(&"X-APPLE-SORT-ORDER"));
    assert_eq!(names.iter().filter(|n| **n == "VALARM").count(), 3);
    assert_eq!(vtodo.children().count(), 3);
    assert_eq!(doc.root.children().count(), 2, "VTIMEZONE and VTODO");
}

#[test]
fn set_replaces_in_place_or_inserts_after_the_last_property() {
    let doc = parse(&common::fixture("apple/timed-alarm.ics"));
    let mut vtodo = doc.root.child("VTODO").cloned().expect("VTODO");
    let before: Vec<String> = entry_names(&vtodo).into_iter().map(String::from).collect();

    vtodo.set(Property::new("SUMMARY", "Renamed"));
    assert_eq!(entry_names(&vtodo), before);
    assert_eq!(vtodo.prop("SUMMARY").unwrap().value, "Renamed");

    vtodo.set(Property::new("PRIORITY", "5"));
    let names = entry_names(&vtodo);
    let uid = names.iter().position(|n| *n == "UID").unwrap();
    assert_eq!(names[uid + 1], "PRIORITY");
    assert_eq!(names[uid + 2], "VALARM");

    assert_eq!(vtodo.remove("PRIORITY"), 1);
    assert_eq!(vtodo.remove("PRIORITY"), 0);
    assert_eq!(entry_names(&vtodo), before);
}

#[test]
fn rejects_malformed_input() {
    assert!(codec::parse("").is_err(), "empty");
    assert!(
        codec::parse("BEGIN:VCALENDAR\r\nX:1\r\n").is_err(),
        "unterminated"
    );
    assert!(
        codec::parse("BEGIN:VCALENDAR\r\nEND:VTODO\r\n").is_err(),
        "mismatched END"
    );
    assert!(
        codec::parse("BEGIN:VCALENDAR\r\nNOCOLON\r\nEND:VCALENDAR\r\n").is_err(),
        "no colon"
    );
    assert!(
        codec::parse("X:1\r\n").is_err(),
        "property outside a component"
    );
    assert!(
        codec::parse("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\nX:1\r\n").is_err(),
        "content after the root component"
    );
}
