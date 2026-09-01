//! `~/.config/husk/config.toml`, with a default for every key.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub vdir: PathBuf,
    pub default_project: Option<String>,
    pub sync_command: Vec<String>,
    pub date_format: String,
    pub time_format: String,
    pub default_alarm_leads: Vec<String>,
    /// A built-in theme flavor: `phosphor` or `ansi`.
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vdir: expand_home(Path::new("~/.local/share/vdirsyncer/tasks")),
            default_project: None,
            sync_command: vec!["vdirsyncer".to_string(), "sync".to_string()],
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            default_alarm_leads: vec!["0m".to_string()],
            theme: "phosphor".to_string(),
        }
    }
}

impl Config {
    /// `~/.config/husk`, or `None` when there is no home directory.
    pub fn dir() -> Option<PathBuf> {
        BaseDirs::new().map(|base| base.config_dir().join("husk"))
    }

    /// The user's theme overrides, `~/.config/husk/theme.toml`.
    pub fn theme_file() -> Option<PathBuf> {
        Self::dir().map(|dir| dir.join("theme.toml"))
    }

    /// Reads `config.toml` from the config directory; no file means defaults.
    pub fn load() -> Result<Self> {
        match Self::dir().map(|dir| dir.join("config.toml")) {
            Some(path) if path.is_file() => Self::from_file(&path),
            _ => Ok(Self::default()),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut config: Self = toml::from_str(text)?;
        config.vdir = expand_home(&config.vdir);
        Ok(config)
    }
}

/// Replaces a leading `~` with the home directory.
pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), BaseDirs::new()) {
        (Ok(rest), Some(base)) => base.home_dir().join(rest),
        _ => path.to_path_buf(),
    }
}
