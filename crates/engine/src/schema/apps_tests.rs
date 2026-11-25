use serde_json::{Value, json};

use super::{AppsParams, AppsResult, SendToAppParams, SendToAppResult};
use crate::Session;
use crate::apps::{self, AppTarget};
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SendToAppParams>(), &["bodies", "copy_overwrite", "copy_to", "format", "target", "wait"]);
    assert_keys(input_schema_for::<AppsParams>(), &[]);
}

#[test]
fn no_command_adds_a_program() {
    for c in crate::cmd::command_specs() {
        assert!(!c.id.contains("add_app") && !c.id.starts_with("file.app_"), "{} would let a script add a program", c.id);
    }
    let schema = input_schema_for::<SendToAppParams>();
    let text = schema.as_value().to_string();
    assert!(!text.contains("\"program\""), "a program path is not a parameter: {text}");
}

fn reply_matches(reply: &Value) {
    let schema = output_schema_for::<SendToAppResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), reply), "{reply} doesn't match {schema:?}");
}

#[test]
fn the_list_reply_matches_its_schema_and_says_which_program_is_there() {
    let _turn = apps::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    apps::set_targets(&[]);
    let empty = s.execute("file.apps", &json!({})).unwrap();
    assert_eq!(empty, json!({"apps": []}));
    let t = AppTarget {
        id: "viewer".into(),
        name: "Viewer".into(),
        program: "/nonexistent/viewer".into(),
        formats: vec!["3mf".into(), "sc".into()],
        ..AppTarget::default()
    };
    apps::set_targets(&[t]);
    let listed = s.execute("file.apps", &json!({})).unwrap();
    apps::set_targets(&[]);
    let schema = output_schema_for::<AppsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &listed), "{listed}");
    assert_eq!(
        listed,
        json!({"apps": [{"id": "viewer", "name": "Viewer", "kind": "app", "program": "/nonexistent/viewer", "args": ["{file}"],
                         "formats": ["3mf", "sc"], "default_format": "3mf", "found": false}]})
    );
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("file.apps", &json!({"target": "x"})).is_err());
}

#[test]
fn a_send_to_an_app_nobody_added_is_refused_and_writes_nothing() {
    let _turn = apps::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    apps::set_targets(&[]);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
    for target in ["/bin/ls", "nothing", ""] {
        let e = s.execute("file.send_to_app", &json!({"target": target, "bodies": ["Block"]})).unwrap_err().to_string();
        assert!(e.contains("file.apps"), "{target}: {e}");
    }
    assert!(s.execute("file.send_to_app", &json!({})).is_err(), "target is needed");
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let e = strict.execute("file.send_to_app", &json!({"target": "x", "program": "/bin/ls"})).unwrap_err().to_string();
    assert!(e.contains("program"), "{e}");
}

/// The program is a script that writes its arguments, one to a line, and the folder it ran in.
#[cfg(unix)]
fn fake_app(tag: &str, args: &[&str], formats: &[&str]) -> (std::path::PathBuf, std::path::PathBuf, AppTarget) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("solidcraft-send-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (script, seen) = (dir.join("fakeapp"), dir.join("seen"));
    let body = format!("#!/bin/sh\npwd >> {seen}\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> {seen}; done\n", seen = seen.display());
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let target = AppTarget {
        id: "fakeapp".into(),
        name: "Fake App".into(),
        program: script.to_string_lossy().into_owned(),
        args: args.iter().map(|a| a.to_string()).collect(),
        formats: formats.iter().map(|f| f.to_string()).collect(),
        default_format: formats.first().map_or("3mf", |f| *f).to_string(),
    };
    (dir, seen, target)
}

#[cfg(unix)]
#[test]
fn replies_match_the_output_schema_for_each_format_and_the_app_gets_one_argument_per_template_entry() {
    let _turn = apps::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (dir, seen, target) = fake_app("replies", &["--title", "{name} as {format}", "{file}"], &["3mf", "step", "sc"]);
    apps::set_targets(&[target]);
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
    let keep = dir.join("kept folder").to_string_lossy().into_owned();
    let mut replies = Vec::new();
    for p in [
        json!({"target": "fakeapp", "bodies": ["Block"], "wait": 3000}),
        json!({"target": "Fake App", "format": "STEP", "bodies": ["Block"], "copy_to": keep, "wait": 3000}),
        json!({"target": "fakeapp", "format": "sc", "wait": 3000}),
        json!({"target": "fakeapp", "format": "sc", "copy_to": keep, "wait": 3000}),
    ] {
        let r = s.execute("file.send_to_app", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        reply_matches(&r);
        assert_eq!(r["app"], "Fake App", "{r}");
        let command = r["command"].as_array().unwrap_or_else(|| panic!("the real command is in the reply: {r}"));
        assert_eq!((command.len(), command.last()), (4, Some(&r["path"])), "{r}");
        assert_eq!(r["exit_code"], 0, "{r}");
        assert!(r.get("app_error").is_none(), "{r}");
        replies.push(r);
    }
    apps::set_targets(&[]);
    assert_eq!(replies[0]["format"], "3mf");
    assert_eq!(replies[1]["format"], "step");
    assert_eq!(replies[2]["format"], "sc");
    assert_eq!(replies[3]["version"], 1);
    let kept = replies[1]["path"].as_str().unwrap();
    assert!(kept.starts_with(&keep) && kept.ends_with("-v1.step"), "{kept}");
    let sc = replies[2]["path"].as_str().unwrap();
    assert_eq!(std::fs::metadata(sc).map(|m| m.len()).ok(), replies[2]["bytes"].as_u64(), "the design file: {sc}");
    let text = std::fs::read_to_string(&seen).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // Each run: its folder, then `--title`, the title as one argument, and the file.
    let step_run = lines.iter().position(|l| l.ends_with("-v1.step")).unwrap();
    assert_eq!(lines[step_run - 2], "--title");
    assert!(lines[step_run - 1].ends_with(" as step") && lines[step_run - 1].contains(' '), "{lines:?}");
    // The folder it ran in holds the file.
    assert!(lines[step_run - 3].ends_with("kept folder"), "{lines:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_format_the_app_does_not_take_and_a_bad_wait_are_refused() {
    let _turn = apps::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (dir, _seen, target) = fake_app("refuse", &["{file}"], &["stl"]);
    apps::set_targets(&[target]);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
    let wrong = s.execute("file.send_to_app", &json!({"target": "fakeapp", "format": "sc"})).unwrap_err().to_string();
    let slow = s.execute("file.send_to_app", &json!({"target": "fakeapp", "bodies": ["Block"], "wait": 99999})).unwrap_err().to_string();
    let unknown = s.execute("file.send_to_app", &json!({"target": "fakeapp", "format": "dwg"})).unwrap_err().to_string();
    apps::set_targets(&[]);
    assert!(wrong.contains("does not take sc"), "{wrong}");
    assert!(slow.contains("wait"), "{slow}");
    assert!(unknown.contains("dwg"), "{unknown}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program that fails at once is reported with its code, and the file is still saved.
#[cfg(unix)]
#[test]
fn a_failing_app_is_reported_with_its_exit_code() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = apps::TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (dir, _seen, mut target) = fake_app("fails", &["{file}"], &["stl"]);
    let script = dir.join("failing");
    std::fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    target.program = script.to_string_lossy().into_owned();
    apps::set_targets(&[target]);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
    let r = s.execute("file.send_to_app", &json!({"target": "fakeapp", "bodies": ["Block"], "wait": 3000})).unwrap();
    apps::set_targets(&[]);
    reply_matches(&r);
    assert_eq!(r["exit_code"], 7, "{r}");
    assert_eq!(r["exited"], true);
    assert!(r["app_error"].as_str().is_some_and(|e| e.contains("code 7")), "{r}");
    assert!(std::path::Path::new(r["path"].as_str().unwrap()).exists(), "the file is saved either way");
    let _ = std::fs::remove_dir_all(&dir);
}
