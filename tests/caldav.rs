mod common;

use std::fs;

use husk::cli;
use husk::config::{Caldav, Config};
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
