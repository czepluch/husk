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
    assert_eq!(config.default_alarm_leads, ["0m"]);
    assert_eq!(config.theme, "phosphor");
    assert_eq!(config.caldav, None);
}

#[test]
fn the_caldav_section_is_optional_and_strict() {
    let config = Config::parse(
        "[caldav]\nurl = \"https://dav.example/\"\nusername = \"me\"\npassword_command = [\"cat\", \"~/.config/pw\"]\n",
    )
    .unwrap();
    let caldav = config.caldav.unwrap();
    assert_eq!(caldav.url, "https://dav.example/");
    assert_eq!(caldav.username, "me");
    assert_eq!(caldav.password_command[0], "cat");
    assert!(
        !caldav.password_command[1].starts_with('~'),
        "home is expanded in the password command"
    );
    assert!(
        Config::parse("[caldav]\nurl = \"x\"\n").is_err(),
        "all three keys are required"
    );
    assert!(
        Config::parse(
            "[caldav]\nurl = \"x\"\nusername = \"u\"\npassword_command = []\npassword = \"p\"\n"
        )
        .is_err()
    );
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

#[test]
fn the_collections_file_defaults_to_vdirsyncers_and_expands_home() {
    let config = Config::default();
    assert!(
        config
            .sync_collections
            .ends_with(".local/share/vdirsyncer/status/tasks.collections")
            && !config.sync_collections.starts_with("~")
    );
    let config = Config::parse("sync_collections = \"~/st/pair.collections\"\n").unwrap();
    assert!(
        config.sync_collections.ends_with("st/pair.collections")
            && !config.sync_collections.starts_with("~")
    );
}
