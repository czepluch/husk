mod common;

use std::fs;

use husk::cli::{self, Leftover};
use husk::config::{Caldav, Config};
use husk::model::{NewTask, ProjectId};
use husk::store::{Store, VdirStore};

fn config(dir: &common::TempDir, url: &str) -> Config {
    Config {
        vdir: dir.path().to_path_buf(),
        sync_command: vec![],
        caldav: Some(Caldav {
            url: url.to_string(),
            username: "jacob".to_string(),
            password_command: vec!["echo".to_string(), "s3cret".to_string()],
        }),
        ..Config::default()
    }
}

#[test]
fn a_list_is_created_on_the_server_with_its_name_then_locally() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("201 Created");
    let config = config(&dir, &server.url);

    let project = cli::add_project(&store, &config, "  Chores ", Some("#FFCC00")).unwrap();
    let request = server.request();

    let first = request.lines().next().unwrap();
    assert!(
        first.starts_with(&format!("MKCOL /jacob/{}/ HTTP/1.1", project.id.as_str())),
        "{first}"
    );
    assert!(
        request.contains("authorization: Basic amFjb2I6czNjcmV0"),
        "{request}"
    );
    assert!(request.contains("<D:displayname>Chores</D:displayname>"));
    assert!(request.contains("<I:calendar-color>#FFCC00</I:calendar-color>"));
    assert!(request.contains("<C:comp name=\"VTODO\"/>"));

    let id = project.id.as_str();
    assert_eq!(id, id.to_uppercase(), "Apple-style id");
    let local = dir.path().join(id);
    assert_eq!(
        fs::read_to_string(local.join("displayname")).unwrap(),
        "Chores"
    );
    assert_eq!(fs::read_to_string(local.join("color")).unwrap(), "#FFCC00");
    assert!(store.projects().unwrap().iter().any(|p| p.name == "Chores"));
}

#[test]
fn a_refused_request_creates_nothing_locally() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let before = store.projects().unwrap().len();
    let server = common::FakeServer::new("401 Unauthorized");
    let config = config(&dir, &server.url);

    let error = cli::add_project(&store, &config, "Chores", None).unwrap_err();
    assert!(format!("{error:#}").contains("credentials"), "{error:#}");
    assert_eq!(store.projects().unwrap().len(), before);
}

#[test]
fn bad_input_is_refused_before_any_request() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let config = config(&dir, "http://127.0.0.1:9/");
    for (name, color, hint) in [
        ("", None, "needs a name"),
        ("Life", None, "already exists"),
        ("Chores", Some("red"), "#RRGGBB"),
    ] {
        let error = cli::add_project(&store, &config, name, color).unwrap_err();
        assert!(format!("{error:#}").contains(hint), "{name}: {error:#}");
    }
    let without = Config {
        caldav: None,
        ..config
    };
    let error = cli::add_project(&store, &without, "Chores", None).unwrap_err();
    assert!(format!("{error:#}").contains("[caldav]"), "{error:#}");
}

#[test]
fn a_failing_password_command_stops_before_any_request() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("201 Created");
    let mut config = config(&dir, &server.url);
    config.caldav.as_mut().unwrap().password_command =
        vec!["sh".to_string(), "-c".to_string(), "exit 3".to_string()];
    let error = cli::add_project(&store, &config, "Chores", None).unwrap_err();
    assert!(format!("{error:#}").contains("sh failed"), "{error:#}");
    assert!(!dir.path().read_dir().unwrap().any(|e| {
        let name = e.unwrap().file_name();
        name.len() == 36 && name.to_string_lossy().contains('-')
    }));
    drop(server);
}

#[test]
fn an_empty_list_is_removed_on_the_server_then_locally() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("204 No Content");
    let config = config(&dir, &server.url);

    let project = cli::remove_project(
        &store,
        &config,
        &ProjectId::new("solidity"),
        &Leftover::Keep,
    )
    .unwrap();
    let request = server.request();
    assert!(
        request.starts_with("DELETE /jacob/solidity/ HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains("authorization: Basic "), "{request}");
    assert_eq!(project.id.as_str(), "solidity");
    assert!(!dir.path().join("solidity").exists());
    assert!(!store.projects().unwrap().iter().any(|p| p.id == project.id));
}

#[test]
fn a_list_already_gone_from_the_server_is_still_removed_locally() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::answering(&["404 Not Found", "207 Multi-Status"]);
    let config = config(&dir, &server.url);
    cli::remove_project(
        &store,
        &config,
        &ProjectId::new("solidity"),
        &Leftover::Keep,
    )
    .unwrap();
    let requests = server.request();
    assert!(
        requests.contains("\nPROPFIND /jacob/ HTTP/1.1"),
        "the home set is checked before a 404 counts: {requests}"
    );
    assert!(!dir.path().join("solidity").exists());
}

#[test]
fn a_404_from_a_wrong_url_removes_nothing() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::answering(&["404 Not Found", "404 Not Found"]);
    let config = config(&dir, &server.url);
    let error = cli::remove_project(
        &store,
        &config,
        &ProjectId::new("solidity"),
        &Leftover::Keep,
    )
    .unwrap_err();
    drop(server.request());
    assert!(format!("{error:#}").contains("[caldav]"), "{error:#}");
    assert!(dir.path().join("solidity").is_dir());
}

#[test]
fn a_name_two_lists_share_is_refused_with_their_ids() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    for id in ["chores-1", "chores-2"] {
        store
            .create_project(&ProjectId::new(id), "Chores", None)
            .unwrap();
    }
    let error = cli::project_named(&store, "chores").unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("2 lists are named \"chores\"")
            && text.contains("chores-1")
            && text.contains("chores-2"),
        "{text}"
    );
    assert_eq!(
        cli::project_named(&store, "chores-2").unwrap().as_str(),
        "chores-2",
        "the id still names one"
    );
    assert_eq!(
        cli::project_named(&store, "LIFE").unwrap().as_str(),
        "life",
        "a unique name matches regardless of case"
    );
}

#[test]
fn a_file_husk_cannot_read_counts_as_a_task_and_goes_only_with_delete_tasks() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    fs::write(dir.path().join("solidity/odd.ics"), "not a calendar\n").unwrap();
    let config = config(&dir, "http://127.0.0.1:9/");
    let solidity = ProjectId::new("solidity");

    let error = cli::remove_project(&store, &config, &solidity, &Leftover::Keep).unwrap_err();
    assert!(format!("{error:#}").contains("holds 1 tasks"), "{error:#}");
    let error = cli::remove_project(
        &store,
        &config,
        &solidity,
        &Leftover::MoveTo("Life".to_string()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("odd.ics"), "{error:#}");
    assert!(dir.path().join("solidity/odd.ics").exists());

    let server = common::FakeServer::new("204 No Content");
    let reachable = Config {
        caldav: Some(Caldav {
            url: server.url.clone(),
            ..config.caldav.clone().unwrap()
        }),
        ..config
    };
    cli::remove_project(&store, &reachable, &solidity, &Leftover::Delete).unwrap();
    drop(server.request());
    assert!(!dir.path().join("solidity").exists());
}

#[test]
fn a_list_with_tasks_is_kept_unless_told_where_they_go() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let config = config(&dir, "http://127.0.0.1:9/");
    let (life_count, argot) = common::vdir_task_counts();
    let life = cli::project_named(&store, "Life").unwrap();
    assert_eq!(life.as_str(), "life");

    let error = cli::remove_project(&store, &config, &life, &Leftover::Keep).unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains(&format!("holds {life_count} tasks")) && text.contains("--move-to"),
        "{text}"
    );
    let error = cli::remove_project(
        &store,
        &config,
        &life,
        &Leftover::MoveTo("Nope".to_string()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("no list named \"Nope\""));
    let error = cli::remove_project(
        &store,
        &config,
        &life,
        &Leftover::MoveTo("life".to_string()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("into itself"));
    let error = cli::project_named(&store, "Nope").unwrap_err();
    assert!(format!("{error:#}").contains("no list named \"Nope\""));
    assert!(dir.path().join("life").is_dir());
    assert_eq!(store.tasks(None).unwrap().len(), life_count + argot);
}

#[test]
fn tasks_move_to_another_list_before_removal() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("204 No Content");
    let config = config(&dir, &server.url);
    let (life, argot) = common::vdir_task_counts();
    let chores = ProjectId::new("chores");
    store.create_project(&chores, "Chores", None).unwrap();
    for summary in ["Vacuum", "Dishes"] {
        store
            .create(
                &chores,
                NewTask {
                    summary: summary.to_string(),
                    ..NewTask::default()
                },
            )
            .unwrap();
    }

    cli::remove_project(
        &store,
        &config,
        &chores,
        &Leftover::MoveTo("Life".to_string()),
    )
    .unwrap();
    drop(server.request());
    assert!(!dir.path().join("chores").exists());
    let tasks = store.tasks(None).unwrap();
    assert_eq!(tasks.len(), life + argot + 2);
    let moved: Vec<&str> = tasks
        .iter()
        .filter(|t| t.summary == "Vacuum" || t.summary == "Dishes")
        .map(|t| t.project.as_str())
        .collect();
    assert_eq!(moved, ["life", "life"]);
}

#[test]
fn tasks_are_deleted_with_the_list_when_asked() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("200 OK");
    let config = config(&dir, &server.url);
    let (life, _) = common::vdir_task_counts();

    cli::remove_project(&store, &config, &ProjectId::new("argot"), &Leftover::Delete).unwrap();
    drop(server.request());
    assert!(!dir.path().join("argot").exists());
    assert_eq!(store.tasks(None).unwrap().len(), life);
}

#[test]
fn a_refused_removal_changes_nothing() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("403 Forbidden");
    let config = config(&dir, &server.url);
    let (life, argot) = common::vdir_task_counts();

    let error = cli::remove_project(&store, &config, &ProjectId::new("argot"), &Leftover::Delete)
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("does not allow jacob to delete"),
        "{error:#}"
    );
    assert!(dir.path().join("argot").is_dir());
    assert_eq!(store.tasks(None).unwrap().len(), life + argot);
}

#[test]
fn a_server_error_names_the_status_without_the_xml_body() {
    let dir = common::fixture_vdir();
    let store = VdirStore::new(dir.path());
    let server = common::FakeServer::new("500 Internal Server Error");
    let config = config(&dir, &server.url);
    let error = cli::add_project(&store, &config, "Chores", None).unwrap_err();
    let text = format!("{error:#}");
    assert!(text.contains("500 Internal Server Error"), "{text}");
    assert!(!text.contains("s3cret"), "{text}");
}
