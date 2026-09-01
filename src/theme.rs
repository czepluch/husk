#![allow(clippy::disallowed_types)]
//! Theme files, their layering and their resolution into ratatui styles.
//! The only place colors live: the UI refers to `Theme` slots, never to a
//! color. The base is a built-in flavor or a Base16/Base24 scheme file
//! through a fixed mapping; `~/.config/husk/theme.toml` sets individual
//! slots on top of it.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use serde::{Deserialize, Serialize};

use crate::config::expand_home;

const PHOSPHOR: &str = include_str!("themes/phosphor.toml");
const ANSI: &str = include_str!("themes/ansi.toml");
/// The Base16 slot mapping; the palette comes from the scheme file.
const BASE16: &str = include_str!("themes/base16.toml");

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
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    pub colors: BTreeMap<String, String>,
    pub styles: BTreeMap<String, StyleSpec>,
    pub symbols: SymbolsSpec,
    pub borders: BordersSpec,
    /// `base00` to `base17` from a scheme file, what `baseXX` refers to.
    #[serde(skip)]
    pub palette: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dim: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SymbolsSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overdue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alarm: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BordersSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// The base named by `theme` in the config (a built-in flavor, or a
    /// Base16/Base24 scheme file through the fixed mapping) with the user's
    /// theme file, if any, laid on top.
    pub fn load(theme: &str, user_file: Option<&Path>) -> Result<Self> {
        let mut file = ThemeFile::base(theme)?;
        if let Some(path) = user_file.filter(|p| p.is_file()) {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))?;
            let over =
                ThemeFile::parse(&text).with_context(|| format!("parse {}", path.display()))?;
            file.merge(over);
        }
        file.resolve()
    }

    /// A foreground style from `#rrggbb` (an alpha suffix is ignored), for
    /// colors that come from data, such as the vdir `color` file, rather
    /// than from the theme.
    pub fn hex_style(&self, hex: &str) -> Option<Style> {
        let digits = hex.trim().trim_start_matches('#');
        let rgb = digits
            .get(..6)
            .filter(|_| digits.len() == 6 || digits.len() == 8)?;
        parse_hex(rgb).map(|color| Style::new().fg(color))
    }

    /// The resolved theme as a theme file, so a user can start from the
    /// current look. Colors come out as hex, ANSI names or `default`.
    pub fn dump(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self.to_file())?)
    }

    fn to_file(&self) -> ThemeFile {
        let color = |style: Style| color_name(style.fg.unwrap_or(Color::Reset));
        let colors = [
            ("bg", color_name(self.base.bg.unwrap_or(Color::Reset))),
            ("fg", color(self.base)),
            ("muted", color(self.muted)),
            ("accent", color(self.accent)),
            ("border", color(self.border)),
            ("border_active", color(self.border_active)),
            ("overdue", color(self.overdue)),
            ("due_today", color(self.due_today)),
            ("due_soon", color(self.due_soon)),
            ("done", color(self.done)),
            ("tag", color(self.tag)),
            ("project", color(self.project)),
            ("recurring", color(self.recurring)),
        ];
        let styles = [
            ("selected", self.selected),
            ("title", self.title),
            ("status_bar", self.status_bar),
            ("help_key", self.help_key),
            ("pri_high", self.pri_high),
            ("pri_medium", self.pri_medium),
            ("pri_low", self.pri_low),
        ];
        ThemeFile {
            colors: colors
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            styles: styles
                .into_iter()
                .map(|(k, v)| (k.to_string(), style_spec(v)))
                .collect(),
            symbols: SymbolsSpec {
                set: None,
                recurring: Some(self.symbols.recurring.clone()),
                overdue: Some(self.symbols.overdue.clone()),
                done: Some(self.symbols.done.clone()),
                subtask: Some(self.symbols.subtask.clone()),
                alarm: Some(self.symbols.alarm.clone()),
            },
            borders: BordersSpec {
                style: Some(
                    match self.border_type {
                        Some(BorderType::Rounded) => "rounded",
                        Some(BorderType::Double) => "double",
                        Some(BorderType::Thick) => "thick",
                        Some(_) => "plain",
                        None => "none",
                    }
                    .to_string(),
                ),
            },
            palette: BTreeMap::new(),
        }
    }
}

fn style_spec(style: Style) -> StyleSpec {
    let has = |m: Modifier| style.add_modifier.contains(m);
    StyleSpec {
        fg: style.fg.map(color_name),
        bg: style.bg.map(color_name),
        bold: has(Modifier::BOLD),
        italic: has(Modifier::ITALIC),
        underline: has(Modifier::UNDERLINED),
        dim: has(Modifier::DIM),
        reverse: has(Modifier::REVERSED),
    }
}

/// The inverse of `parse_color`, for `husk theme dump`.
fn color_name(color: Color) -> String {
    match color {
        Color::Reset => "default".to_string(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(n) => n.to_string(),
        Color::Black => "black".to_string(),
        Color::Red => "red".to_string(),
        Color::Green => "green".to_string(),
        Color::Yellow => "yellow".to_string(),
        Color::Blue => "blue".to_string(),
        Color::Magenta => "magenta".to_string(),
        Color::Cyan => "cyan".to_string(),
        Color::Gray => "white".to_string(),
        Color::DarkGray => "bright_black".to_string(),
        Color::LightRed => "bright_red".to_string(),
        Color::LightGreen => "bright_green".to_string(),
        Color::LightYellow => "bright_yellow".to_string(),
        Color::LightBlue => "bright_blue".to_string(),
        Color::LightMagenta => "bright_magenta".to_string(),
        Color::LightCyan => "bright_cyan".to_string(),
        Color::White => "bright_white".to_string(),
    }
}

/// The `base00` to `base17` slots of a Base16 or Base24 scheme file, in
/// either layout (top-level `base00: "1d2021"` or under `palette:` with a
/// `#`). Only the palette is read; everything else in the file is ignored.
pub fn parse_scheme(text: &str) -> Result<BTreeMap<String, String>> {
    let mut palette = BTreeMap::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_ascii_lowercase();
        if key.len() != 6 || !key.starts_with("base") {
            continue;
        }
        // Quoted or bare, with or without a trailing comment.
        let raw = value.trim();
        let value = match raw.chars().next() {
            Some(quote @ ('"' | '\'')) => raw[1..].split(quote).next().unwrap_or(""),
            _ => raw.split_whitespace().next().unwrap_or(""),
        };
        let hex = value.trim_start_matches('#');
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("{key}: {value:?} is not a hex color");
        }
        palette.insert(key, hex.to_string());
    }
    for slot in (0..16).map(|n| format!("base{n:02x}")) {
        if !palette.contains_key(&slot) {
            bail!("scheme file has no {slot}");
        }
    }
    Ok(palette)
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
    /// A built-in flavor by name, or a scheme file by path through the
    /// Base16 mapping.
    pub fn base(theme: &str) -> Result<Self> {
        if let Ok(text) = builtin(theme) {
            return Self::parse(text).with_context(|| format!("built-in theme {theme}"));
        }
        let path = expand_home(Path::new(theme));
        if !path.is_file() {
            bail!(
                "theme {theme:?} is neither a built-in flavor (phosphor, ansi) nor a scheme file"
            );
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut file = Self::parse(BASE16).context("built-in base16 mapping")?;
        file.palette = parse_scheme(&text).with_context(|| format!("scheme {}", path.display()))?;
        Ok(file)
    }

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
        parse_color(value, self, 0).with_context(|| format!("color slot {slot}"))
    }

    fn style(&self, spec: &StyleSpec) -> Result<Style> {
        let mut style = Style::new();
        if let Some(fg) = &spec.fg {
            style = style.fg(parse_color(fg, self, 0)?);
        }
        if let Some(bg) = &spec.bg {
            style = style.bg(parse_color(bg, self, 0)?);
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

/// `default`, `#rrggbb`, an ANSI name, a 0 to 255 index, a `baseXX` slot of
/// the loaded scheme, or the name of a color slot (one level deep).
fn parse_color(value: &str, file: &ThemeFile, depth: u8) -> Result<Color> {
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
    let lower = v.to_ascii_lowercase();
    if lower.starts_with("base") && lower.len() == 6 {
        let hex = file.palette.get(&lower).with_context(|| {
            if file.palette.is_empty() {
                format!("{value:?} is a Base16 reference, but no scheme file is loaded")
            } else {
                format!("the scheme has no {value}")
            }
        })?;
        return parse_hex(hex).with_context(|| format!("bad hex color {hex:?} for {value}"));
    }
    if COLOR_SLOTS.contains(&v) {
        if depth > 0 {
            bail!("color slot {v:?} refers to another slot; only one level is allowed");
        }
        let referenced = file
            .colors
            .get(v)
            .with_context(|| format!("color slot {v:?} is not set"))?;
        return parse_color(referenced, file, depth + 1);
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
        let none = ThemeFile::default();
        assert_eq!(parse_color("208", &none, 0).unwrap(), Color::Indexed(208));
        assert_eq!(
            parse_color("Bright_Red", &none, 0).unwrap(),
            Color::LightRed
        );
        assert_eq!(parse_color("white", &none, 0).unwrap(), Color::Gray);
        assert_eq!(parse_color("DEFAULT", &none, 0).unwrap(), Color::Reset);
    }

    const CLASSIC_SCHEME: &str = "scheme: \"Test dark\"\nauthor: \"nobody\"\nbase00: \"1d2021\"\nbase01: \"3c3836\"\nbase02: \"504945\"\nbase03: \"665c54\"\nbase04: \"bdae93\"\nbase05: \"d5c4a1\"\nbase06: \"ebdbb2\"\nbase07: \"fbf1c7\"\nbase08: \"fb4934\"\nbase09: \"fe8019\"\nbase0A: \"fabd2f\"\nbase0B: \"b8bb26\"\nbase0C: \"8ec07c\"\nbase0D: \"83a598\"\nbase0E: \"d3869b\"\nbase0F: \"d65d0e\"\n";
    const NEW_SCHEME: &str = "system: \"base16\"\nname: \"Test\"\nvariant: \"dark\"\npalette:\n  base00: \"#1d2021\"\n  base01: \"#3c3836\"\n  base02: \"#504945\"\n  base03: \"#665c54\"\n  base04: \"#bdae93\"\n  base05: \"#d5c4a1\"\n  base06: \"#ebdbb2\"\n  base07: \"#fbf1c7\"\n  base08: \"#fb4934\"\n  base09: \"#fe8019\"\n  base0A: \"#fabd2f\"\n  base0B: \"#b8bb26\"\n  base0C: \"#8ec07c\"\n  base0D: \"#83a598\"\n  base0E: \"#d3869b\"\n  base0F: \"#d65d0e\"\n";

    #[test]
    fn scheme_files_parse_in_both_layouts() {
        let classic = parse_scheme(CLASSIC_SCHEME).unwrap();
        let newer = parse_scheme(NEW_SCHEME).unwrap();
        assert_eq!(classic, newer);
        assert_eq!(classic["base08"], "fb4934");
        let err = parse_scheme("base00: \"1d2021\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("base01"), "{err}");
        assert!(parse_scheme("base00: \"zzzzzz\"\n").is_err());
    }

    #[test]
    fn a_scheme_maps_onto_the_slots_and_can_be_overridden() {
        let dir = std::env::temp_dir().join(format!("husk-scheme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let scheme = dir.join("gruvbox.yaml");
        std::fs::write(&scheme, NEW_SCHEME).unwrap();
        let theme = Theme::load(scheme.to_str().unwrap(), None).unwrap();
        assert_eq!(
            theme.overdue,
            Style::new().fg(Color::Rgb(0xfb, 0x49, 0x34)),
            "base08"
        );
        assert_eq!(
            theme.accent,
            Style::new().fg(Color::Rgb(0xb8, 0xbb, 0x26)),
            "base0B"
        );
        assert_eq!(theme.base.bg, Some(Color::Rgb(0x1d, 0x20, 0x21)), "base00");
        assert_eq!(
            theme.tag,
            Style::new().fg(Color::Rgb(0xd3, 0x86, 0x9b)),
            "base0E"
        );

        let user = dir.join("theme.toml");
        std::fs::write(&user, "[colors]\naccent = \"base09\"\ntag = \"#ffffff\"\n").unwrap();
        let theme = Theme::load(scheme.to_str().unwrap(), Some(&user)).unwrap();
        assert_eq!(
            theme.accent,
            Style::new().fg(Color::Rgb(0xfe, 0x80, 0x19)),
            "the user file may use baseXX"
        );
        assert_eq!(theme.tag, Style::new().fg(Color::Rgb(0xff, 0xff, 0xff)));

        let err = Theme::load("phosphor", Some(&user)).unwrap_err();
        assert!(format!("{err:#}").contains("no scheme file"), "{err:#}");
        assert!(Theme::load(dir.join("missing.yaml").to_str().unwrap(), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scheme_files_tolerate_real_world_quirks() {
        let quirky = "\u{feff}# A comment line\r\nsystem: base24\r\npalette:\r\n  base00: '#1d2021' # background\r\n  base01: 3c3836\r\n  base02: \"#504945\"\r\n  base03: \"665c54\"\r\n  base04: bdae93\r\n  base05: d5c4a1\r\n  base06: ebdbb2\r\n  base07: fbf1c7\r\n  base08: fb4934\r\n  base09: fe8019\r\n  base0A: fabd2f\r\n  base0B: b8bb26\r\n  base0C: 8ec07c\r\n  base0D: 83a598\r\n  base0E: d3869b\r\n  base0F: d65d0e\r\n  base10: 000000\r\n  base17: ffffff\r\n";
        let palette = parse_scheme(quirky).unwrap();
        assert_eq!(
            palette["base00"], "1d2021",
            "quoted with a trailing comment"
        );
        assert_eq!(palette["base01"], "3c3836", "bare");
        assert_eq!(palette["base17"], "ffffff", "Base24 slots come along");
        assert!(parse_scheme("name: x\n").is_err(), "not a scheme");
    }

    #[test]
    fn a_scheme_with_overrides_dumps_and_reloads_identically() {
        let dir = std::env::temp_dir().join(format!("husk-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let scheme = dir.join("s.yaml");
        std::fs::write(&scheme, NEW_SCHEME).unwrap();
        let user = dir.join("theme.toml");
        std::fs::write(
            &user,
            "[colors]\nfg = \"default\"\nmuted = \"bright_black\"\ntag = \"208\"\n[styles]\ntitle = { fg = \"base09\", italic = true, underline = true }\n[symbols]\nset = \"ascii\"\n[borders]\nstyle = \"none\"\n",
        )
        .unwrap();
        let theme = Theme::load(scheme.to_str().unwrap(), Some(&user)).unwrap();
        let text = theme.dump().unwrap();
        let again = ThemeFile::parse(&text).unwrap().resolve().unwrap();
        assert_eq!(again, theme, "{text}");
        assert!(
            text.contains("style = \"none\"") && text.contains("tag = \"208\""),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dump_round_trips_through_the_parser() {
        for flavor in ["phosphor", "ansi"] {
            let theme = Theme::load(flavor, None).unwrap();
            let text = theme.dump().unwrap();
            assert!(
                text.contains("[colors]") && text.contains("[styles.selected]"),
                "{text}"
            );
            let again = ThemeFile::parse(&text).unwrap().resolve().unwrap();
            assert_eq!(again, theme, "{flavor}:\n{text}");
        }
    }

    #[test]
    fn data_colors_come_through_hex_style() {
        let theme = ThemeFile::parse(PHOSPHOR).unwrap().resolve().unwrap();
        let expected = Style::new().fg(Color::Rgb(0x83, 0xd7, 0x54));
        assert_eq!(theme.hex_style("#83D754"), Some(expected));
        assert_eq!(theme.hex_style("83d754"), Some(expected));
        assert_eq!(
            theme.hex_style("#83D754FF\n"),
            Some(expected),
            "alpha suffix ignored"
        );
        assert_eq!(theme.hex_style("#83D75"), None);
        assert_eq!(theme.hex_style("green"), None);
    }

    #[test]
    fn unknown_flavor_is_an_error() {
        assert!(builtin("solarized").is_err());
        assert!(Theme::load("phosphor", None).is_ok());
    }
}
