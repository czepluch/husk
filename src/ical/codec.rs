//! RFC 5545 content-line codec: unfolding, folding, escaping and parsing into
//! an ordered component tree that serializes back byte for byte.
//!
//! Properties and child components share one ordered list because clients
//! interleave them (Tasks.org writes `X-APPLE-SORT-ORDER` after the VALARMs).
//! Values and parameters are stored exactly as written, so anything husk does
//! not understand survives a rewrite unchanged.

use anyhow::{Context, Result, bail};

/// Longest physical line in octets, excluding the line break (RFC 5545 3.1).
const MAX_LINE_OCTETS: usize = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Crlf,
    Lf,
}

impl LineEnding {
    fn as_str(self) -> &'static str {
        match self {
            Self::Crlf => "\r\n",
            Self::Lf => "\n",
        }
    }

    /// The convention of the first line break; LF when there is none.
    fn detect(input: &str) -> Self {
        match input.find('\n') {
            Some(i) if input[..i].ends_with('\r') => Self::Crlf,
            _ => Self::Lf,
        }
    }
}

/// A parsed file: the root component plus the line-ending convention it was
/// read with, so it can be written back the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub root: Component,
    pub line_ending: LineEnding,
}

impl Document {
    /// A new document, written with CRLF as RFC 5545 requires.
    pub fn new(root: Component) -> Self {
        Self {
            root,
            line_ending: LineEnding::Crlf,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Property(Property),
    Component(Component),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub params: Vec<Param>,
    /// The value as written, escapes included. Use [`Property::text`] for text.
    pub value: String,
}

/// A property parameter, value as written (quotes included when present).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub value: String,
}

impl Component {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            entries: Vec::new(),
        }
    }

    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn properties(&self) -> impl Iterator<Item = &Property> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Property(p) => Some(p),
            Entry::Component(_) => None,
        })
    }

    /// All properties with this name, in file order.
    pub fn props<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> {
        self.properties().filter(move |p| p.is(name))
    }

    pub fn prop(&self, name: &str) -> Option<&Property> {
        self.properties().find(|p| p.is(name))
    }

    pub fn prop_mut(&mut self, name: &str) -> Option<&mut Property> {
        self.entries.iter_mut().find_map(|e| match e {
            Entry::Property(p) if p.is(name) => Some(p),
            _ => None,
        })
    }

    pub fn children(&self) -> impl Iterator<Item = &Component> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Component(c) => Some(c),
            Entry::Property(_) => None,
        })
    }

    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut Component> {
        self.entries.iter_mut().filter_map(|e| match e {
            Entry::Component(c) => Some(c),
            Entry::Property(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Component> {
        self.children().find(|c| c.is(name))
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Component> {
        self.children_mut().find(|c| c.is(name))
    }

    /// Replaces the first property with this name in place. A new property
    /// goes before the first child component, after the properties already
    /// there, since RFC 5545 puts a component's properties before its
    /// sub-components (Tasks.org puts one after them; that one stays put).
    pub fn set(&mut self, prop: Property) {
        if let Some(existing) = self.prop_mut(&prop.name) {
            *existing = prop;
            return;
        }
        let at = self
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Component(_)))
            .unwrap_or(self.entries.len());
        self.entries.insert(at, Entry::Property(prop));
    }

    /// Sets a text value, escaped, keeping the property's parameters.
    pub fn set_text(&mut self, name: &str, text: &str) {
        match self.prop_mut(name) {
            Some(existing) => existing.value = escape_text(text),
            None => self.set(Property::text_value(name, text)),
        }
    }

    /// Removes every property with this name and returns how many there were.
    pub fn remove(&mut self, name: &str) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|e| !matches!(e, Entry::Property(p) if p.is(name)));
        before - self.entries.len()
    }

    pub fn push_child(&mut self, child: Component) {
        self.entries.push(Entry::Component(child));
    }
}

impl Property {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            value: value.into(),
        }
    }

    /// A property whose value is free text, escaped on the way in.
    pub fn text_value(name: impl Into<String>, text: &str) -> Self {
        Self::new(name, escape_text(text))
    }

    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push(Param {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// A parameter value with surrounding quotes removed.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(|p| p.value.trim_matches('"'))
    }

    /// The value with text escapes resolved.
    pub fn text(&self) -> String {
        unescape_text(&self.value)
    }

    fn content_line(&self) -> String {
        let mut line = self.name.clone();
        for p in &self.params {
            line.push(';');
            line.push_str(&p.name);
            line.push('=');
            line.push_str(&p.value);
        }
        line.push(':');
        line.push_str(&self.value);
        line
    }
}

/// Joins folded lines: a line break followed by one space or tab is removed.
pub fn unfold(input: &str) -> String {
    input
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "")
}

pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Resolves text escapes. Unknown escapes are kept as written rather than
/// rejected, since phone clients are not strict about them either.
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(e @ ('\\' | ';' | ',')) => out.push(e),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn parse(input: &str) -> Result<Document> {
    let line_ending = LineEnding::detect(input);
    let unfolded = unfold(input);
    let mut stack: Vec<Component> = Vec::new();
    let mut root: Option<Component> = None;

    for (i, line) in unfolded.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let n = i + 1;
        if root.is_some() {
            bail!("line {n}: content after the end of the root component");
        }
        let prop = parse_content_line(line).with_context(|| format!("line {n}"))?;
        if prop.is("BEGIN") {
            stack.push(Component::new(prop.value));
        } else if prop.is("END") {
            let component = stack
                .pop()
                .with_context(|| format!("line {n}: END without BEGIN"))?;
            if !component.is(&prop.value) {
                bail!(
                    "line {n}: END:{} closes BEGIN:{}",
                    excerpt(&prop.value),
                    excerpt(&component.name)
                );
            }
            match stack.last_mut() {
                Some(parent) => parent.push_child(component),
                None => root = Some(component),
            }
        } else {
            match stack.last_mut() {
                Some(component) => component.entries.push(Entry::Property(prop)),
                None => bail!("line {n}: property outside a component"),
            }
        }
    }

    if let Some(open) = stack.last() {
        bail!("unterminated component BEGIN:{}", excerpt(&open.name));
    }
    let root = root.context("no component found")?;
    Ok(Document { root, line_ending })
}

fn parse_content_line(line: &str) -> Result<Property> {
    let name_end = line
        .find([';', ':'])
        .with_context(|| format!("missing ':' in {:?}", excerpt(line)))?;
    let name = &line[..name_end];
    if name.is_empty() {
        bail!("empty property name in {:?}", excerpt(line));
    }
    let mut params = Vec::new();
    let mut rest = &line[name_end..];
    while let Some(after) = rest.strip_prefix(';') {
        let eq = after
            .find('=')
            .with_context(|| format!("parameter without '=' in {:?}", excerpt(line)))?;
        let value_start = &after[eq + 1..];
        let end = param_value_end(value_start);
        params.push(Param {
            name: after[..eq].to_string(),
            value: value_start[..end].to_string(),
        });
        rest = &value_start[end..];
    }
    let value = rest
        .strip_prefix(':')
        .with_context(|| format!("missing ':' after parameters in {:?}", excerpt(line)))?;
    Ok(Property {
        name: name.to_string(),
        params,
        value: value.to_string(),
    })
}

/// The start of a line for an error message; a CR-only file is one line.
fn excerpt(line: &str) -> String {
    let mut short: String = line.chars().take(40).collect();
    if short.len() < line.len() {
        short.push_str("...");
    }
    short
}

/// Index of the first `;` or `:` outside double quotes, or the end.
fn param_value_end(s: &str) -> usize {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' | ':' if !in_quotes => return i,
            _ => {}
        }
    }
    s.len()
}

pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    write_component(&doc.root, doc.line_ending, &mut out);
    out
}

fn write_component(c: &Component, le: LineEnding, out: &mut String) {
    write_line(&format!("BEGIN:{}", c.name), le, out);
    for entry in &c.entries {
        match entry {
            Entry::Property(p) => write_line(&p.content_line(), le, out),
            Entry::Component(child) => write_component(child, le, out),
        }
    }
    write_line(&format!("END:{}", c.name), le, out);
}

/// Writes one content line, folded so no physical line exceeds 75 octets.
/// Continuation lines start with a space that counts toward their 75.
fn write_line(line: &str, le: LineEnding, out: &mut String) {
    let mut rest = line;
    let mut first = true;
    loop {
        let limit = if first {
            MAX_LINE_OCTETS
        } else {
            out.push(' ');
            MAX_LINE_OCTETS - 1
        };
        if rest.len() <= limit {
            out.push_str(rest);
            out.push_str(le.as_str());
            return;
        }
        let cut = char_boundary_at_or_before(rest, limit);
        out.push_str(&rest[..cut]);
        out.push_str(le.as_str());
        rest = &rest[cut..];
        first = false;
    }
}

fn char_boundary_at_or_before(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
