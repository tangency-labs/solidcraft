use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::generator::installed::{Installed, load_file};

use super::*;
use crate::Services;
use crate::dialogs::{Dialog, apply_commands};
use crate::test_support::run_all;

fn example() -> Installed {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/generators/slot-nut-20.generator.json"));
    let generator = Arc::new(load_file(&path).unwrap());
    Installed { path, generator }
}

/// The dialog as the Insert tab opens it, with the example package installed.
fn opened() -> (SolveApp, Dialog) {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.start("generator.build");
    let mut d = app.dialog.clone().expect("the dialog opens");
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.packages = Some(vec![example()]);
    (app, d)
}

fn set_code(d: &mut Dialog, code: &str) {
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.code = code.into();
}

#[test]
fn insert_opens_the_ordering_code_dialog_with_no_selection_needed() {
    let (_, d) = opened();
    assert!(d.inputs.is_empty());
    assert!(!d.previews(), "building a part is not a live preview");
}

#[test]
fn a_code_the_package_accepts_inserts_the_part_through_the_command() {
    let (mut app, mut d) = opened();
    set_code(&mut d, " TN20-M4 ");
    let commands = apply_commands(&app, &d).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, "generator.build");
    assert_eq!(commands[0].1["code"], "TN20-M4");
    assert!(commands[0].1["package"].as_str().unwrap().ends_with("slot-nut-20.generator.json"));
    run_all(&mut app, commands);
    let state = app.session.model.state();
    assert_eq!(state.bodies.len(), 1);
    let volume = state.bodies[0].mesh().measure().volume;
    assert!((volume - 365.788).abs() < 0.5, "{volume}");
}

#[test]
fn a_refused_or_empty_code_gives_the_packages_reason_and_changes_nothing() {
    let (app, mut d) = opened();
    for (code, words) in [("TN20-M3-2", "not offered"), ("TN20-M6-2", "not modelled"), ("TN20-M9", "not a TN20"), ("", "")] {
        set_code(&mut d, code);
        let e = apply_commands(&app, &d).unwrap_err();
        assert!(e.contains(words), "{code}: {e}");
    }
    assert!(app.session.model.state().bodies.is_empty());
}

#[test]
fn hostile_codes_never_panic_the_dialog() {
    let (app, mut d) = opened();
    let big = "9".repeat(100_000);
    for code in ["\0", "TN20-M4\u{202e}", big.as_str(), "${1/0}", "TN20-M4-\u{1F600}", "   "] {
        set_code(&mut d, code);
        assert!(apply_commands(&app, &d).is_err(), "{code:?}");
    }
}

#[test]
fn with_no_package_installed_there_is_nothing_to_insert() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.start("generator.build");
    let mut d = app.dialog.clone().unwrap();
    let Kind::Generator(g) = &mut d.kind else { panic!("wrong dialog") };
    g.packages = Some(Vec::new());
    g.code = "TN20-M4".into();
    assert!(apply_commands(&app, &d).is_err());
}
