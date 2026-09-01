mod common;

use std::fs;
use std::process::Command;

use serde_json::Value;

struct Setup {
    dir: common::TempDir,
    config: std::path::PathBuf,
}

fn setup(sync_command: &str) -> Setup {
    let dir = common::fixture_vdir();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        format!(
            "vdir = \"{}\"\ndefault_project = \"Life\"\nsync_command = [{sync_command}]\n",
            dir.path().display()
        ),
    )
    .unwrap();
    Setup { dir, config }
}

fn husk(setup: &Setup, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_husk"))
        .arg("--config")
        .arg(&setup.config)
        .args(args)
        .output()
        .expect("run husk");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn list_prints_text_and_json_with_view_and_project_filters() {
    let s = setup("\"true\"");
    let (ok, out, err) = husk(&s, &["list", "--view", "all"]);
    assert!(ok, "{err}");
    assert!(out.contains("Remember the milk"), "{out}");
    assert!(out.contains("!!! Imp"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("◂ ") && l.contains("Imp")),
        "overdue rows are marked:\n{out}"
    );
    assert!(out.contains("[Life]") && out.contains("[Argot]"), "{out}");

    let (ok, out, _) = husk(&s, &["list", "--view", "all", "--json"]);
    assert!(ok);
    let rows: Vec<Value> = serde_json::from_str(&out).unwrap();
    let (life, argot) = common::vdir_task_counts();
    assert_eq!(rows.len(), life + argot);
    let milk = rows
        .iter()
        .find(|r| r["summary"] == "Remember the milk")
        .unwrap();
    assert_eq!(milk["project"], "Life");
    assert_eq!(milk["priority"], "none");
    assert!(milk.get("done").is_none(), "no always-false field");
    assert!(milk["due"].as_str().unwrap().starts_with("2026-08-31T"));
    let parent = rows.iter().find(|r| r["summary"] == "Parent").unwrap();
    assert_eq!(parent["tags"], serde_json::json!(["Testing"]));
    assert_eq!(parent["due"], "2026-09-01");
    assert_eq!(parent["priority"], "high");

    let (ok, out, _) = husk(
        &s,
        &["list", "--view", "all", "--project", "argot", "--json"],
    );
    assert!(ok);
    let rows: Vec<Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(rows.len(), argot);
    assert!(rows.iter().all(|r| r["project"] == "Argot"));

    let (ok, out, _) = husk(&s, &["list", "--view", "overdue", "--json"]);
    assert!(ok);
    let rows: Vec<Value> = serde_json::from_str(&out).unwrap();
    assert!(rows.iter().all(|r| r["overdue"] == true), "{out}");

    let (ok, _, err) = husk(&s, &["list", "--project", "nope"]);
    assert!(!ok);
    assert!(err.contains("nope"), "{err}");

    fs::create_dir_all(s.dir.path().join("faelles")).unwrap();
    fs::write(s.dir.path().join("faelles/displayname"), "Fælles & Børn\n").unwrap();
    let (ok, out, err) = husk(
        &s,
        &[
            "list",
            "--view",
            "all",
            "--project",
            "FÆLLES & BØRN",
            "--json",
        ],
    );
    assert!(ok, "case folding beyond ASCII: {err}");
    assert_eq!(out.trim(), "[]");
}

#[test]
fn add_creates_a_task_and_runs_the_sync_command() {
    let dir = common::TempDir::new();
    let marker = dir.path().join("marker");
    let s = setup(&format!(
        "\"sh\", \"-c\", \"echo synced > {}\"",
        marker.display()
    ));
    let (ok, out, err) = husk(
        &s,
        &[
            "add",
            "From",
            "the",
            "cli",
            "due:tomorrow",
            "09:00",
            "+shell",
            "pri:l",
        ],
    );
    assert!(ok, "{err}");
    assert_eq!(out.trim(), "Added: From the cli [Life]");
    let text = fs::read_dir(s.dir.path().join("life"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| fs::read_to_string(e.path()).unwrap_or_default())
        .find(|t| t.contains("SUMMARY:From the cli"))
        .expect("task file in life");
    assert!(text.contains("CATEGORIES:shell"), "{text}");
    assert!(text.contains("PRIORITY:9"), "{text}");
    assert_eq!(
        text.matches("BEGIN:VALARM").count(),
        3,
        "timed due gets the default alarms"
    );
    assert!(marker.exists(), "add waits for the sync command to finish");

    let (ok, _, err) = husk(&s, &["add", "Elsewhere", "@argot"]);
    assert!(ok, "{err}");
    assert!(
        fs::read_dir(s.dir.path().join("argot"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| fs::read_to_string(e.path())
                .unwrap_or_default()
                .contains("SUMMARY:Elsewhere"))
    );

    let (ok, _, err) = husk(&s, &["add", "Nowhere", "@nosuch"]);
    assert!(!ok);
    assert!(err.contains("nosuch"), "{err}");

    let (ok, _, err) = husk(&s, &["add"]);
    assert!(!ok);
    assert!(err.contains("title"), "{err}");
}

#[test]
fn notify_dry_run_lists_what_would_fire_and_writes_nothing() {
    let s = setup("\"true\"");
    let state = s.dir.path().join("state/notify.json");
    let path = state.to_str().unwrap();
    let (ok, out, err) = husk(&s, &["notify", "--dry-run", "--state", path]);
    assert!(ok, "{err}");
    assert!(out.starts_with("Alarms after "), "{out}");
    assert!(out.contains("(last run: never)"), "{out}");
    assert!(
        out.trim_end().ends_with("nothing to notify"),
        "first run, fixtures are old:\n{out}"
    );
    assert!(!state.exists(), "a dry run writes no state");

    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(
        &state,
        "{\"last_run\": \"2026-08-30T00:00:00Z\", \"fired\": []}",
    )
    .unwrap();
    let (ok, out, err) = husk(&s, &["notify", "--dry-run", "--state", path]);
    assert!(ok, "{err}");
    assert!(out.contains("Remember the milk"), "{out}");
    assert!(out.contains("Imp"), "{out}");
    assert!(
        !out.contains("Test task"),
        "no alarm, no due: silent\n{out}"
    );
    assert!(
        fs::read_to_string(&state)
            .unwrap()
            .contains("2026-08-30T00:00:00Z"),
        "untouched"
    );
}

#[test]
fn sync_runs_the_configured_command() {
    let s = setup("\"true\"");
    assert!(husk(&s, &["sync"]).0);
    let f = setup("\"sh\", \"-c\", \"echo broken >&2; exit 4\"");
    let (ok, _, err) = husk(&f, &["sync"]);
    assert!(!ok);
    assert!(err.contains("broken"), "{err}");
    let n = setup("");
    let (ok, _, err) = husk(&n, &["sync"]);
    assert!(!ok);
    assert!(err.contains("sync_command"), "{err}");
}

#[test]
fn version_and_help_work_without_a_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_husk"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("husk "));
    let output = Command::new(env!("CARGO_BIN_EXE_husk"))
        .args(["add", "--help"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("due:fri"));
}

#[test]
fn add_reports_a_failed_sync_but_keeps_the_task() {
    let s = setup("\"sh\", \"-c\", \"echo boom >&2; exit 7\"");
    let (ok, out, err) = husk(&s, &["add", "Survives", "the", "sync"]);
    assert!(!ok);
    assert!(out.contains("Added: Survives the sync"), "{out}");
    assert!(err.contains("boom"), "{err}");
    assert!(
        fs::read_dir(s.dir.path().join("life"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| fs::read_to_string(e.path())
                .unwrap_or_default()
                .contains("SUMMARY:Survives the sync"))
    );
}
