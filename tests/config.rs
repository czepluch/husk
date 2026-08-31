mod common;

use std::fs;
use std::path::Path;

use husk::config::{Config, expand_home};

#[test]
fn defaults_match_the_spec() {
    let config = Config::default();
    assert!(config.vdir.ends_with(".local/share/vdirsyncer/tasks"));
    assert!(!config.vdir.starts_with("~"), "home is expanded");
    assert_eq!(config.default_project, None);
    assert_eq!(config.sync_command, ["vdirsyncer", "sync"]);
    assert_eq!(config.date_format, "%Y-%m-%d");
    assert_eq!(config.time_format, "%H:%M");
    assert_eq!(config.default_alarm_leads, ["1d", "1h", "0m"]);
    assert_eq!(config.theme, "phosphor");
}

#[test]
fn a_partial_file_keeps_the_other_defaults() {
    let config = Config::parse("theme = \"ansi\"\nvdir = \"~/tasks\"\n").unwrap();
    assert_eq!(config.theme, "ansi");
    assert!(config.vdir.ends_with("tasks") && !config.vdir.starts_with("~"));
    assert_eq!(config.date_format, "%Y-%m-%d");
    assert_eq!(config.sync_command, ["vdirsyncer", "sync"]);
}

#[test]
fn unknown_keys_and_wrong_types_are_errors() {
    assert!(Config::parse("colour = \"red\"\n").is_err());
    assert!(Config::parse("vdir = 3\n").is_err());
    assert!(Config::parse("sync_command = \"vdirsyncer sync\"\n").is_err());
    assert!(Config::parse("").is_ok());
}

#[test]
fn files_are_read_and_missing_ones_reported() {
    let dir = common::TempDir::new();
    let path = dir.path().join("config.toml");
    fs::write(&path, "date_format = \"%d/%m/%Y\"\n").unwrap();
    assert_eq!(Config::from_file(&path).unwrap().date_format, "%d/%m/%Y");
    let err = Config::from_file(&dir.path().join("nope.toml")).unwrap_err();
    assert!(format!("{err:#}").contains("nope.toml"), "{err:#}");
}

#[test]
fn expand_home_only_touches_a_leading_tilde() {
    assert_eq!(expand_home(Path::new("/abs/~x")), Path::new("/abs/~x"));
    assert_eq!(expand_home(Path::new("rel/~")), Path::new("rel/~"));
    let home = expand_home(Path::new("~/x"));
    assert!(!home.starts_with("~") && home.ends_with("x"));
}
