use super::*;
use crate::{Services, dialogs::Dialog};
use solidcraft_engine::Session;

fn app() -> SolveApp {
    SolveApp::new(Session::default(), Services::default())
}

#[test]
fn weld_dialog_done_creates_one_graphic_and_undo_removes_it() {
    let mut h = crate::scenario::Harness::new();
    h.app.start("weld.create");
    h.frames(2);
    let d = h.app.dialog.as_mut().expect("Weld starts as a real dialog");
    assert!(matches!(d.kind, Kind::Weld(_)));
    d.confirm = true;
    h.frames(2);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.welds.len(), 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.welds.is_empty());
}

#[test]
fn weld_new_bead_preserves_settings_and_cancel_does_not_run() {
    let mut app = app();
    let mut d = Dialog::for_command(&app, "weld.create").unwrap();
    let Kind::Weld(state) = &mut d.kind else { panic!() };
    state.panel.size.a = "6 mm".into();
    assert!(event(&mut app, &mut d, WeldPanelEvent::NewBead));
    let Kind::Weld(state) = &d.kind else { panic!() };
    assert_eq!(state.panel.number, "W2");
    assert_eq!(state.panel.size.a, "6 mm");
    assert_eq!(app.session.doc.welds.len(), 1);
    assert!(!event(&mut app, &mut d, WeldPanelEvent::Cancel));
    assert_eq!(app.session.doc.welds.len(), 1);
}

#[test]
fn weld_done_errors_stay_in_the_dialog_and_programmatic_calls_do_not_open_it() {
    let mut app = app();
    let mut d = Dialog::for_command(&app, "weld.create").unwrap();
    let Kind::Weld(state) = &mut d.kind else { panic!() };
    state.panel.size.a = "-1 mm".into();
    assert!(event(&mut app, &mut d, WeldPanelEvent::Done));
    assert!(d.error.is_some());
    assert!(app.session.doc.welds.is_empty());
    let Kind::Weld(state) = &mut d.kind else { panic!() };
    state.panel.size.a = "3 mm".into();
    state.panel.picks.line.push("local edge label".into());
    assert!(event(&mut app, &mut d, WeldPanelEvent::Done));
    assert!(d.error.is_some());
    assert!(app.session.doc.welds.is_empty());
    app.session.execute("weld.create", &json!({"weld_type":"fillet"})).unwrap();
    assert!(app.dialog.is_none());
}

#[test]
fn weld_panel_type_ids_match_the_engine_contract() {
    let app = app();
    let mut state = WeldDialog::default();
    for kind in crate::weld_panel::TYPES {
        state.panel.weld_type = kind.weld_type;
        let p = params(&state).unwrap();
        assert!(solidcraft_engine::doc::weld::TYPES.contains(&p["weld_type"].as_str().unwrap()));
    }
    assert!(start(&app, "weld.create").is_some());
}
