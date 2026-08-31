mod common;

use std::fs;

use chrono::{DateTime, TimeZone, Utc};
use husk::model::{Due, NewTask, Priority, ProjectId};
use husk::store::{Store, VdirStore};

fn clock() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 31, 14, 0, 0).unwrap()
}

fn store(dir: &common::TempDir) -> VdirStore {
    VdirStore::new(dir.path()).with_clock(clock)
}

fn life() -> ProjectId {
    ProjectId::new("life")
}

fn argot() -> ProjectId {
    ProjectId::new("argot")
}

const TEST_TASK: &str = "BD215969-28EE-4474-A2E4-61575C3C49E7";

fn tmp_files(dir: &common::TempDir) -> Vec<String> {
    let mut found = Vec::new();
    for project in fs::read_dir(dir.path()).unwrap() {
        for entry in fs::read_dir(project.unwrap().path()).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.ends_with(".tmp") {
                found.push(name);
            }
        }
    }
    found
}

#[test]
fn projects_come_from_directory_names_and_metadata_files() {
    let dir = common::fixture_vdir();
    let projects = store(&dir).projects().unwrap();
    let summary: Vec<(String, String, Option<String>)> = projects
        .into_iter()
        .map(|p| (p.id.as_str().to_string(), p.name, p.color))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("argot".into(), "Argot".into(), None),
            ("life".into(), "Life".into(), Some("#83D754".into())),
            ("solidity".into(), "solidity".into(), None),
        ]
    );
}

#[test]
fn tasks_lists_all_projects_or_one() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let (life_n, argot_n) = common::vdir_task_counts();
    let all = store.tasks(None).unwrap();
    assert_eq!(all.len(), life_n + argot_n);
    assert!(
        all.iter()
            .all(|t| t.project == life() || t.project == argot())
    );

    assert_eq!(store.tasks(Some(&life())).unwrap().len(), life_n);
    assert_eq!(store.tasks(Some(&argot())).unwrap().len(), argot_n);
    assert_eq!(
        store
            .tasks(Some(&ProjectId::new("solidity")))
            .unwrap()
            .len(),
        0
    );
    assert!(store.tasks(Some(&ProjectId::new("nope"))).is_err());
}

#[test]
fn unparsable_and_foreign_files_are_skipped() {
    let dir = common::fixture_vdir();
    fs::write(dir.path().join("life/garbage.ics"), "not a calendar\n").unwrap();
    fs::write(dir.path().join("life/notes.txt"), "BEGIN:VCALENDAR").unwrap();
    fs::write(dir.path().join("life/leftover.ics.tmp"), "BEGIN:VCALENDAR").unwrap();
    assert_eq!(
        store(&dir).tasks(Some(&life())).unwrap().len(),
        common::vdir_task_counts().0
    );
}

#[test]
fn get_finds_a_uid_even_when_the_file_name_differs() {
    let dir = common::fixture_vdir();
    let task = store(&dir).get("200671310813144693").unwrap();
    assert_eq!(task.summary, "Android task");
    assert_eq!(task.project, argot());
    assert!(store(&dir).get("missing").is_err());
}

#[test]
fn create_writes_a_uid_named_file_and_returns_the_task() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let new = NewTask {
        summary: "Created here".to_string(),
        due: Some(Due::DateTime(clock())),
        priority: Priority::Medium,
        tags: vec!["home".to_string()],
        ..NewTask::default()
    };
    let task = store.create(&life(), new.clone()).unwrap();
    assert_eq!(task.summary, new.summary);
    assert_eq!(task.due, new.due);
    assert_eq!(task.priority, Priority::Medium);
    assert_eq!(task.tags, vec!["home"]);
    assert_eq!(task.project, life());

    let path = dir.path().join(format!("life/{}.ics", task.uid));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("SUMMARY:Created here\r\n"), "{text}");
    assert!(text.contains("DUE:20260831T140000Z\r\n"), "{text}");
    assert_eq!(
        store.tasks(Some(&life())).unwrap().len(),
        common::vdir_task_counts().0 + 1
    );
    assert_eq!(store.get(&task.uid).unwrap(), task);
    assert!(tmp_files(&dir).is_empty());

    assert!(
        store
            .create(&ProjectId::new("nope"), NewTask::default())
            .is_err()
    );
}

#[test]
fn saving_an_unchanged_task_leaves_the_file_alone() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let path = dir.path().join("life/timed-alarm.ics");
    let before = fs::read(&path).unwrap();
    let mut task = store.get("18137A41-46DD-4CC3-AD7B-B606DB130741").unwrap();
    store.save(&mut task).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!fs::read_to_string(&path).unwrap().contains("SEQUENCE"));
}

#[test]
fn saving_a_change_bumps_sequence_and_stamps_and_patches_only_that_change() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let path = dir.path().join("life/no-due.ics");
    let before = fs::read_to_string(&path).unwrap();

    let mut task = store.get(TEST_TASK).unwrap();
    task.summary = "Renamed".to_string();
    store.save(&mut task).unwrap();

    let after = fs::read_to_string(&path).unwrap();
    let mut added: Vec<&str> = after.lines().filter(|l| !before.contains(l)).collect();
    added.sort();
    assert_eq!(
        added,
        vec![
            "DTSTAMP:20260831T140000Z",
            "LAST-MODIFIED:20260831T140000Z",
            "SEQUENCE:1",
            "SUMMARY:Renamed",
        ]
    );
    assert_eq!(
        after.lines().count(),
        before.lines().count() + 1,
        "only SEQUENCE is new"
    );
    assert!(!after.contains('\r'), "LF file stays LF");
    assert!(tmp_files(&dir).is_empty());

    // The same Task value can be edited and saved again without a re-read.
    store.save(&mut task).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        after,
        "second save is a no-op"
    );
    task.summary = "Renamed twice".to_string();
    store.save(&mut task).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("SEQUENCE:2\n"), "{text}");
    assert!(text.contains("SUMMARY:Renamed twice\n"), "{text}");
    assert_eq!(store.get(TEST_TASK).unwrap(), task);
}

#[test]
fn saving_works_on_files_laid_out_differently_from_husk() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    // Folded at 74 octets (python icalendar's width), no trailing newline.
    let long = format!("DESCRIPTION:{}\n {}", "x".repeat(62), "y".repeat(10));
    let text = common::fixture("apple/no-due.ics")
        .replace("UID:BD215969-28EE-4474-A2E4-61575C3C49E7", "UID:odd-layout")
        .replace("SUMMARY:Test task", &format!("{long}\nSUMMARY:Odd"));
    let path = dir.path().join("life/odd.ics");
    fs::write(&path, text.trim_end()).unwrap();

    let mut task = store.get("odd-layout").unwrap();
    assert_eq!(
        task.description.as_deref(),
        Some(&*format!("{}{}", "x".repeat(62), "y".repeat(10)))
    );
    task.summary = "Odd, renamed".to_string();
    store.save(&mut task).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("SUMMARY:Odd\\, renamed\n"), "{after}");
    assert!(after.contains("SEQUENCE:1\n"), "{after}");
    assert_eq!(store.get("odd-layout").unwrap().summary, "Odd, renamed");
}

#[test]
fn saving_refuses_when_the_file_changed_on_disk() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let path = dir.path().join("life/no-due.ics");
    let mut task = store.get(TEST_TASK).unwrap();

    let edited_elsewhere = fs::read_to_string(&path)
        .unwrap()
        .replace("SUMMARY:Test task", "SUMMARY:Edited on the phone");
    fs::write(&path, &edited_elsewhere).unwrap();

    task.summary = "Edited here".to_string();
    let err = store.save(&mut task).unwrap_err();
    assert!(err.to_string().contains("changed on disk"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), edited_elsewhere);
}

#[test]
fn delete_removes_the_file() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    store.delete("200671310813144693").unwrap();
    assert!(!dir.path().join("argot/priority-low.ics").exists());
    assert!(store.get("200671310813144693").is_err());
    assert!(store.delete("200671310813144693").is_err());
}

#[test]
fn move_to_writes_the_new_file_before_removing_the_old_one() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let old = dir.path().join("life/no-due.ics");
    let bytes = fs::read(&old).unwrap();

    store.move_to(TEST_TASK, &argot()).unwrap();
    assert!(!old.exists());
    let new = dir.path().join("argot/no-due.ics");
    assert_eq!(fs::read(&new).unwrap(), bytes, "same bytes, same UID");
    assert_eq!(store.get(TEST_TASK).unwrap().project, argot());
    let (life_n, argot_n) = common::vdir_task_counts();
    assert_eq!(store.tasks(None).unwrap().len(), life_n + argot_n);
    assert!(tmp_files(&dir).is_empty());

    store.move_to(TEST_TASK, &argot()).unwrap();
    assert!(new.exists(), "moving to the same project is a no-op");

    assert!(store.move_to(TEST_TASK, &ProjectId::new("nope")).is_err());
    assert!(new.exists(), "a failed move leaves the original in place");
}

#[test]
fn move_to_refuses_a_target_that_already_holds_the_uid() {
    let dir = common::fixture_vdir();
    let store = store(&dir);
    let original = dir.path().join("life/no-due.ics");
    fs::copy(&original, dir.path().join("solidity/copy.ics")).unwrap();

    let err = store
        .move_to(TEST_TASK, &ProjectId::new("solidity"))
        .unwrap_err();
    assert!(err.to_string().contains("already holds"), "{err}");
    assert!(original.exists());
}
