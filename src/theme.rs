#![allow(clippy::disallowed_types)]
//! Theme files, their layering and their resolution into ratatui styles.
//! The only place colors live: the UI refers to `Theme` slots, never to a
//! color. A built-in flavor is the base; `~/.config/husk/theme.toml` sets
//! individual slots on top of it.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use serde::Deserialize;

const PHOSPHOR: &str = include_str!("themes/phosphor.toml");
const ANSI: &str = include_str!("themes/ansi.toml");

pub const COLOR_SLOTS: [&str; 13] = [
    "bg",
    "fg",
    "muted",
    "accent",
    "border",
    "border_active",
    "overdue",
    "due_today",
    "due_soon",
    "done",
    "tag",
    "project",
    "recurring",
];
/// Priority is a text weight by default (bold, normal, dim) rather than a
/// hue, so it never competes with the due-date colors; a theme can still
/// give the `pri_*` styles a color.
pub const STYLE_SLOTS: [&str; 7] = [
    "selected",
    "title",
    "status_bar",
    "help_key",
    "pri_high",
    "pri_medium",
    "pri_low",
];

/// The contents of one theme file; every key optional so files can be layered.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    pub colors: BTreeMap<String, String>,
    pub styles: BTreeMap<String, StyleSpec>,
    pub symbols: SymbolsSpec,
    pub borders: BordersSpec,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StyleSpec {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub reverse: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SymbolsSpec {
    pub set: Option<String>,
    pub recurring: Option<String>,
    pub overdue: Option<String>,
    pub done: Option<String>,
    pub subtask: Option<String>,
    pub alarm: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct BordersSpec {
    pub style: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbols {
    pub recurring: String,
    pub overdue: String,
    pub done: String,
    pub subtask: String,
    pub alarm: String,
}

impl Symbols {
    fn set(name: &str) -> Result<Self> {
        let (recurring, overdue, done, subtask, alarm) = match name {
            "unicode" => ("↻", "◂", "✓", "└", "◷"),
            "ascii" => ("@", "<", "x", "-", "*"),
            other => bail!("unknown symbol set {other:?}; sets are unicode and ascii"),
        };
        Ok(Self {
            recurring: recurring.to_string(),
            overdue: overdue.to_string(),
            done: done.to_string(),
            subtask: subtask.to_string(),
            alarm: alarm.to_string(),
        })
    }
}

/// Everything the UI needs, fully resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub base: Style,
    pub muted: Style,
    pub accent: Style,
    pub border: Style,
    pub border_active: Style,
    pub overdue: Style,
    pub due_today: Style,
    pub due_soon: Style,
    pub done: Style,
    pub pri_high: Style,
    pub pri_medium: Style,
    pub pri_low: Style,
    pub tag: Style,
    pub project: Style,
    pub recurring: Style,
    pub selected: Style,
    pub title: Style,
    pub status_bar: Style,
    pub help_key: Style,
    pub symbols: Symbols,
    /// `None` draws no borders at all.
    pub border_type: Option<BorderType>,
}

impl Theme {
    /// A built-in flavor with the user's theme file, if any, laid on top.
    pub fn load(flavor: &str, user_file: Option<&Path>) -> Result<Self> {
        let mut file = ThemeFile::parse(builtin(flavor)?)
            .with_context(|| format!("built-in theme {flavor}"))?;
        if let Some(path) = user_file.filter(|p| p.is_file()) {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))?;
            let over =
                ThemeFile::parse(&text).with_context(|| format!("parse {}", path.display()))?;
            file.merge(over);
        }
        file.resolve()
    }
}

/// The source of a built-in flavor.
pub fn builtin(name: &str) -> Result<&'static str> {
    match name {
        "phosphor" => Ok(PHOSPHOR),
        "ansi" => Ok(ANSI),
        other => bail!("unknown theme {other:?}; built-in themes are phosphor and ansi"),
    }
}

impl ThemeFile {
    pub fn parse(text: &str) -> Result<Self> {
        let file: Self = toml::from_str(text)?;
        if let Some(slot) = file
            .colors
            .keys()
            .find(|slot| !COLOR_SLOTS.contains(&slot.as_str()))
        {
            bail!("unknown color slot {slot:?}");
        }
        if let Some(slot) = file
            .styles
            .keys()
            .find(|slot| !STYLE_SLOTS.contains(&slot.as_str()))
        {
            bail!("unknown style slot {slot:?}");
        }
        Ok(file)
    }

    /// Lays `over` on top of this file: every key it sets wins.
    pub fn merge(&mut self, over: Self) {
        self.colors.extend(over.colors);
        self.styles.extend(over.styles);
        let s = &mut self.symbols;
        let o = over.symbols;
        s.set = o.set.or(s.set.take());
        s.recurring = o.recurring.or(s.recurring.take());
        s.overdue = o.overdue.or(s.overdue.take());
        s.done = o.done.or(s.done.take());
        s.subtask = o.subtask.or(s.subtask.take());
        s.alarm = o.alarm.or(s.alarm.take());
        self.borders.style = over.borders.style.or(self.borders.style.take());
    }

    pub fn resolve(&self) -> Result<Theme> {
        let fg = |slot: &str| -> Result<Style> { Ok(Style::new().fg(self.color(slot)?)) };
        let style = |slot: &str, fallback: Style| -> Result<Style> {
            match self.styles.get(slot) {
                Some(spec) => self.style(spec),
                None => Ok(fallback),
            }
        };
        let accent = fg("accent")?;
        let muted = fg("muted")?;
        Ok(Theme {
            base: Style::new().fg(self.color("fg")?).bg(self.color("bg")?),
            muted,
            accent,
            border: fg("border")?,
            border_active: fg("border_active")?,
            overdue: fg("overdue")?,
            due_today: fg("due_today")?,
            due_soon: fg("due_soon")?,
            done: fg("done")?,
            pri_high: style("pri_high", Style::new().add_modifier(Modifier::BOLD))?,
            pri_medium: style("pri_medium", Style::new())?,
            pri_low: style("pri_low", Style::new().add_modifier(Modifier::DIM))?,
            tag: fg("tag")?,
            project: fg("project")?,
            recurring: fg("recurring")?,
            selected: style("selected", Style::new().add_modifier(Modifier::REVERSED))?,
            title: style("title", accent.add_modifier(Modifier::BOLD))?,
            status_bar: style("status_bar", muted)?,
            help_key: style("help_key", accent.add_modifier(Modifier::BOLD))?,
            symbols: self.symbols()?,
            border_type: self.border_type()?,
        })
    }

    fn color(&self, slot: &str) -> Result<Color> {
        let value = self
            .colors
            .get(slot)
            .with_context(|| format!("color slot {slot} is not set"))?;
        parse_color(value, &self.colors, 0).with_context(|| format!("color slot {slot}"))
    }

    fn style(&self, spec: &StyleSpec) -> Result<Style> {
        let mut style = Style::new();
        if let Some(fg) = &spec.fg {
            style = style.fg(parse_color(fg, &self.colors, 0)?);
        }
        if let Some(bg) = &spec.bg {
            style = style.bg(parse_color(bg, &self.colors, 0)?);
        }
        for (on, modifier) in [
            (spec.bold, Modifier::BOLD),
            (spec.italic, Modifier::ITALIC),
            (spec.underline, Modifier::UNDERLINED),
            (spec.dim, Modifier::DIM),
            (spec.reverse, Modifier::REVERSED),
        ] {
            if on {
                style = style.add_modifier(modifier);
            }
        }
        Ok(style)
    }

    fn symbols(&self) -> Result<Symbols> {
        let mut symbols = Symbols::set(self.symbols.set.as_deref().unwrap_or("unicode"))?;
        let spec = &self.symbols;
        for (slot, value) in [
            (&mut symbols.recurring, &spec.recurring),
            (&mut symbols.overdue, &spec.overdue),
            (&mut symbols.done, &spec.done),
            (&mut symbols.subtask, &spec.subtask),
            (&mut symbols.alarm, &spec.alarm),
        ] {
            if let Some(value) = value {
                *slot = value.clone();
            }
        }
        Ok(symbols)
    }

    fn border_type(&self) -> Result<Option<BorderType>> {
        Ok(match self.borders.style.as_deref().unwrap_or("plain") {
            "plain" => Some(BorderType::Plain),
            "rounded" => Some(BorderType::Rounded),
            "double" => Some(BorderType::Double),
            "thick" => Some(BorderType::Thick),
            "none" => None,
            other => bail!("unknown border style {other:?}"),
        })
    }
}

/// `default`, `#rrggbb`, an ANSI name, a 0 to 255 index, or the name of a
/// color slot (one level deep). `base0X` references need a Base16 scheme,
/// which is not supported yet.
fn parse_color(value: &str, slots: &BTreeMap<String, String>, depth: u8) -> Result<Color> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("default") {
        return Ok(Color::Reset);
    }
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex(hex).with_context(|| format!("bad hex color {value:?}"));
    }
    if let Some(color) = ansi(v) {
        return Ok(color);
    }
    if let Ok(index) = v.parse::<u8>() {
        return Ok(Color::Indexed(index));
    }
    if v.starts_with("base0") || v.starts_with("base1") {
        bail!("{value:?} is a Base16 reference, which needs a scheme file (not supported yet)");
    }
    if COLOR_SLOTS.contains(&v) {
        if depth > 0 {
            bail!("color slot {v:?} refers to another slot; only one level is allowed");
        }
        let referenced = slots
            .get(v)
            .with_context(|| format!("color slot {v:?} is not set"))?;
        return parse_color(referenced, slots, depth + 1);
    }
    bail!("unknown color {value:?}")
}

fn parse_hex(hex: &str) -> Option<Color> {
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::Rgb(
        (n >> 16) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ))
}

/// ANSI names as terminals and Base16 use them; ratatui's `Gray` is ANSI 7
/// (white) and its `White` is ANSI 15 (bright white).
fn ansi(name: &str) -> Option<Color> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::Gray,
        "bright_black" | "gray" | "grey" => Color::DarkGray,
        "bright_red" => Color::LightRed,
        "bright_green" => Color::LightGreen,
        "bright_yellow" => Color::LightYellow,
        "bright_blue" => Color::LightBlue,
        "bright_magenta" => Color::LightMagenta,
        "bright_cyan" => Color::LightCyan,
        "bright_white" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_flavors_resolve() {
        let phosphor = ThemeFile::parse(PHOSPHOR).unwrap().resolve().unwrap();
        assert_eq!(
            phosphor.overdue,
            Style::new().fg(Color::Rgb(0xff, 0x40, 0x40))
        );
        assert_eq!(
            phosphor.title,
            Style::new()
                .fg(Color::Rgb(0x39, 0xff, 0x14))
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(phosphor.symbols.recurring, "↻");
        assert_eq!(phosphor.pri_high, Style::new().add_modifier(Modifier::BOLD));
        assert_eq!(phosphor.pri_medium, Style::new());
        assert_eq!(phosphor.pri_low, Style::new().add_modifier(Modifier::DIM));
        assert_eq!(phosphor.border_type, Some(BorderType::Plain));

        let ansi = ThemeFile::parse(ANSI).unwrap().resolve().unwrap();
        assert_eq!(ansi.base, Style::new().fg(Color::Reset).bg(Color::Reset));
        assert_eq!(ansi.muted, Style::new().fg(Color::DarkGray));
        assert_eq!(ansi.selected, Style::new().add_modifier(Modifier::REVERSED));
    }

    #[test]
    fn user_file_overrides_single_slots() {
        let mut base = ThemeFile::parse(PHOSPHOR).unwrap();
        let over = ThemeFile::parse(
            "[colors]\noverdue = \"magenta\"\n[symbols]\nset = \"ascii\"\nrecurring = \"R\"\n[borders]\nstyle = \"rounded\"\n",
        )
        .unwrap();
        base.merge(over);
        let theme = base.resolve().unwrap();
        assert_eq!(theme.overdue, Style::new().fg(Color::Magenta));
        assert_eq!(
            theme.accent,
            Style::new().fg(Color::Rgb(0x39, 0xff, 0x14)),
            "untouched"
        );
        assert_eq!(theme.symbols.recurring, "R");
        assert_eq!(theme.symbols.done, "x", "ascii set with one override");
        assert_eq!(theme.border_type, Some(BorderType::Rounded));
    }

    #[test]
    fn unknown_slots_and_colors_are_errors() {
        assert!(ThemeFile::parse("[colors]\nbackground = \"red\"\n").is_err());
        assert!(ThemeFile::parse("[styles]\nheader = { bold = true }\n").is_err());
        assert!(ThemeFile::parse("[colors]\nfg = { r = 1 }\n").is_err());
        let mut file = ThemeFile::parse(PHOSPHOR).unwrap();
        file.colors.insert("fg".into(), "base05".into());
        let err = format!("{:#}", file.resolve().unwrap_err());
        assert!(err.contains("Base16"), "{err}");
        file.colors.insert("fg".into(), "chartreuse".into());
        assert!(file.resolve().is_err());
        file.colors.insert("fg".into(), "#12345".into());
        assert!(file.resolve().is_err());
    }

    #[test]
    fn colors_can_reference_a_slot_one_level_deep() {
        let mut file = ThemeFile::parse(PHOSPHOR).unwrap();
        file.colors.insert("border_active".into(), "accent".into());
        assert_eq!(
            file.resolve().unwrap().border_active,
            file.resolve().unwrap().accent
        );
        file.colors.insert("accent".into(), "fg".into());
        assert!(file.resolve().is_err(), "two levels");
        file.colors.insert("accent".into(), "accent".into());
        assert!(file.resolve().is_err(), "self reference");
    }

    #[test]
    fn indexed_and_named_colors_parse() {
        let none = BTreeMap::new();
        assert_eq!(parse_color("208", &none, 0).unwrap(), Color::Indexed(208));
        assert_eq!(
            parse_color("Bright_Red", &none, 0).unwrap(),
            Color::LightRed
        );
        assert_eq!(parse_color("white", &none, 0).unwrap(), Color::Gray);
        assert_eq!(parse_color("DEFAULT", &none, 0).unwrap(), Color::Reset);
    }

    #[test]
    fn user_file_is_layered_on_the_flavor() {
        let dir = std::env::temp_dir().join(format!("husk-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("theme.toml");
        std::fs::write(
            &path,
            "[styles]\ntitle = { underline = true }\n[symbols]\nset = \"nope\"\n",
        )
        .unwrap();
        assert!(
            Theme::load("phosphor", Some(&path)).is_err(),
            "unknown symbol set"
        );
        std::fs::write(
            &path,
            "[styles]\ntitle = { underline = true }\n[symbols]\nset = \"ascii\"\n",
        )
        .unwrap();
        let theme = Theme::load("phosphor", Some(&path)).unwrap();
        assert_eq!(
            theme.title,
            Style::new().add_modifier(Modifier::UNDERLINED),
            "a style slot is replaced whole"
        );
        assert_eq!(theme.symbols.done, "x");
        assert_eq!(theme.symbols.overdue, "<");
        assert_eq!(
            theme.accent,
            Style::new().fg(Color::Rgb(0x39, 0xff, 0x14)),
            "untouched"
        );
        let missing = dir.join("missing.toml");
        assert!(
            Theme::load("phosphor", Some(&missing)).is_ok(),
            "no file, no overrides"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_flavor_is_an_error() {
        assert!(builtin("solarized").is_err());
        assert!(Theme::load("phosphor", None).is_ok());
    }
}
