mod common;

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use husk::sync::Syncer;

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_burst_of_requests_becomes_one_run() {
    let dir = common::TempDir::new();
    let marker = dir.path().join("runs");
    let syncer = Syncer::new(vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("echo run >> {}", marker.display()),
    ]);
    for _ in 0..5 {
        syncer.request();
    }
    assert!(
        wait_for(|| syncer.state().runs == 1),
        "{:?}",
        syncer.state()
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);
    let state = syncer.state();
    assert!(
        state.last_ok.is_some() && state.last_error.is_none(),
        "{state:?}"
    );

    syncer.request();
    assert!(
        wait_for(|| syncer.state().runs == 2),
        "{:?}",
        syncer.state()
    );
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 2);
}

#[test]
fn failures_are_reported_and_an_empty_command_is_a_no_op() {
    let syncer = Syncer::new(vec![
        "sh".to_string(),
        "-c".to_string(),
        "echo first >&2; echo nothing synced >&2; exit 3".to_string(),
    ]);
    syncer.request();
    assert!(wait_for(|| syncer.state().runs == 1));
    let state = syncer.state();
    assert_eq!(
        state.last_error.as_deref(),
        Some("sh failed: nothing synced")
    );
    assert!(state.last_ok.is_none());

    let missing = Syncer::new(vec!["husk-no-such-command-xyz".to_string()]);
    missing.request();
    assert!(wait_for(|| missing.state().runs == 1));
    assert!(
        missing
            .state()
            .last_error
            .unwrap()
            .starts_with("husk-no-such-command-xyz:")
    );

    let disabled = Syncer::new(vec![]);
    disabled.request();
    thread::sleep(Duration::from_millis(800));
    assert_eq!(disabled.state().runs, 0);
}

#[test]
fn flush_waits_for_a_pending_run_and_runs_are_two_seconds_apart() {
    let dir = common::TempDir::new();
    let marker = dir.path().join("runs");
    let syncer = Syncer::new(vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("date +%s%N >> {}", marker.display()),
    ]);
    syncer.request();
    assert!(syncer.busy());
    assert!(syncer.flush(Duration::from_secs(8)));
    assert_eq!(syncer.state().runs, 1);
    assert!(!syncer.busy());

    thread::sleep(Duration::from_millis(700));
    syncer.request();
    assert!(syncer.flush(Duration::from_secs(8)));
    assert_eq!(syncer.state().runs, 2);
    let stamps: Vec<i128> = fs::read_to_string(&marker)
        .unwrap()
        .lines()
        .map(|l| l.trim().parse().unwrap())
        .collect();
    assert!(stamps[1] - stamps[0] >= 2_000_000_000, "{stamps:?}");

    let disabled = Syncer::new(vec![]);
    disabled.request();
    assert!(!disabled.busy(), "a disabled syncer is never busy");
}
