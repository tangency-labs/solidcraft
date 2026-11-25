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

#[test]
fn weld_dialog_host_fills_available_tool_height_with_pinned_actions() {
    let mut h = crate::scenario::Harness::new();
    h.app.start("weld.create");
    h.frames(3);
    let card = h.app.viewport.dialog_rect.expect("visible weld card");
    let header = crate::scenario::rect_of("dialog:header").expect("shared header");
    let body = crate::scenario::rect_of("dialog:body:viewport").expect("body viewport");
    let footer = crate::scenario::rect_of("dialog:footer").expect("shared footer");
    let define = crate::scenario::rect_of("weld:define_symbol").expect("Define symbol");
    let ok = crate::scenario::rect_of("dialog:ok").expect("OK");
    let cancel = crate::scenario::rect_of("dialog:cancel").expect("Cancel");
    let t = Tokens::get();
    let vp = crate::agent_chat::tool_space(&h.app, crate::shell::content_rect(&h.app, h.app.viewport.rect.unwrap()));
    let drag_bounds = crate::shell::tool_drag_rect(&h.app, vp);
    let gap = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Feature).gap();
    assert!((card.bottom() - (drag_bounds.bottom() - gap)).abs() <= 1.0);
    assert!((header.top() - card.top()).abs() <= 1.0);
    assert!((footer.bottom() - card.bottom()).abs() <= 1.0);
    assert!(header.bottom() <= body.top());
    assert!(body.bottom() <= footer.top() + 0.5);
    for button in [cancel, ok] {
        assert!(footer.contains_rect(button));
    }
    assert!(define.bottom() <= footer.top() + 0.5, "the symbol row is above the Cancel / OK bar");
    for button in [cancel, ok] {
        assert!((button.height() - t.form_button_height).abs() <= 0.5);
    }
    assert!(!define.intersects(cancel));
    assert!(!cancel.intersects(ok));
    println!(
        "live dialog headless host: card {card:?}; header {header:?}; body {body:?}; footer {footer:?}; Define {define:?}; Cancel {cancel:?}; OK {ok:?}"
    );
}
