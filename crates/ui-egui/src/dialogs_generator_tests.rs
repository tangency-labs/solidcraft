use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::generator::Package;
use solidcraft_engine::generator::installed::Installed;

use super::*;
use crate::Services;
use crate::dialogs::{Dialog, apply_commands};
use crate::test_support::run_all;

/// A small generator package in a folder of its own: a plate whose `length` the code gives.
fn example() -> (PathBuf, Installed) {
    let dir = std::env::temp_dir().join(format!("solidcraft-generator-dialog-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut part = Session::default();
    part.execute("parameters.add", &json!({"name": "length", "expression": "40 mm"})).unwrap();
    part.execute("solid.box", &json!({"length": "length", "width": 7, "height": 3, "body_name": "Nut"})).unwrap();
    part.execute("file.save", &json!({"path": dir.join("tnut.sc")})).unwrap();
    std::fs::write(
        dir.join("generator.toml"),
        "format = \"solidcraft-generator/2\"\nid = \"metric-t-nut\"\nname = \"Metric T-nut\"\nmodel = \"tnut.sc\"\n\
         [parameters]\nlength = { type = \"int\", min = 20, max = 100, step = 10, unit = \"mm\" }\n\
         [[template]]\npattern = \"TN-{length}\"\n",
    )
    .unwrap();
    let package = Arc::new(Package::load(&dir).unwrap());
    (dir.clone(), Installed { path: dir, package })
}

/// The dialog as the Insert tab opens it, with the example package installed.
fn opened() -> (PathBuf, SolveApp, Dialog) {
    let (dir, installed) = example();
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.start("generator.insert");
    let mut d = app.dialog.clone().expect("the dialog opens");
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.packages = Some(vec![installed]);
    (dir, app, d)
}

fn set_code(d: &mut Dialog, code: &str) {
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.code = code.into();
}

#[test]
fn insert_opens_the_ordering_code_dialog_with_no_selection_needed() {
    let (_dir, _, d) = opened();
    assert!(d.inputs.is_empty());
    assert!(!d.previews(), "building a part is not a live preview");
}

#[test]
fn a_code_the_package_accepts_inserts_the_part_through_the_command() {
    let (_dir, mut app, mut d) = opened();
    set_code(&mut d, " TN-40 ");
    let commands = apply_commands(&app, &d).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, "generator.insert");
    assert_eq!(commands[0].1["code"], "TN-40");
    run_all(&mut app, commands);
    let state = app.session.model.state();
    assert_eq!(state.bodies.len(), 1);
}

#[test]
fn a_refused_or_empty_code_gives_the_generators_reason_and_changes_nothing() {
    let (_dir, app, mut d) = opened();
    for (code, words) in [("TN-45", "ordering code"), ("XX-40", "ordering code"), ("", "")] {
        set_code(&mut d, code);
        let e = apply_commands(&app, &d).unwrap_err();
        assert!(e.contains(words), "{code}: {e}");
    }
    assert!(app.session.model.state().bodies.is_empty());
}

#[test]
fn hostile_codes_never_panic_the_dialog() {
    let (_dir, app, mut d) = opened();
    let big = "9".repeat(100_000);
    for code in ["\0", "TN-40\u{202e}", big.as_str(), "${1/0}", "TN-\u{1F600}", "   "] {
        set_code(&mut d, code);
        assert!(apply_commands(&app, &d).is_err(), "{code:?}");
    }
}

#[test]
fn with_no_package_installed_there_is_nothing_to_insert() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.start("generator.insert");
    let mut d = app.dialog.clone().unwrap();
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.packages = Some(Vec::new());
    g.code = "TN-40".into();
    assert!(apply_commands(&app, &d).is_err());
}
