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
    assert!(app.session.doc.welds[0].symbol.is_some(), "the engine builds the symbol after evaluating 6 mm");
    assert!(!event(&mut app, &mut d, WeldPanelEvent::Cancel));
    assert_eq!(app.session.doc.welds.len(), 1);
}

#[test]
fn weld_dialog_left_new_bead_target_saves_and_keeps_the_tool_open() {
    let mut h = crate::scenario::Harness::new();
    h.app.start("weld.create");
    h.frames(3);
    let apply = crate::scenario::rect_of("dialog:apply").expect("left New weld bead target");
    let cancel = crate::scenario::rect_of("dialog:cancel").unwrap();
    assert!(apply.right() <= cancel.left());
    h.step(&json!({"click":[apply.center().x, apply.center().y]})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.welds.len(), 1);
    let dialog = h.app.dialog.as_ref().expect("New weld bead keeps the tool open");
    let Kind::Weld(state) = &dialog.kind else { panic!("Weld dialog remains active") };
    assert_eq!(state.panel.number, "W2");
    let cancel = crate::scenario::rect_of("dialog:cancel").unwrap();
    h.step(&json!({"click":[cancel.center().x, cancel.center().y]})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.welds.len(), 1);
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
    let apply = crate::scenario::rect_of("dialog:apply").expect("New weld bead");
    let t = Tokens::get();
    let vp = crate::agent_chat::tool_space(&h.app, crate::shell::content_rect(&h.app, h.app.viewport.rect.unwrap()));
    let drag_bounds = crate::shell::tool_drag_rect(&h.app, vp);
    let gap = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Feature).gap();
    assert!((card.bottom() - (drag_bounds.bottom() - gap)).abs() <= 1.0);
    assert!((header.top() - card.top()).abs() <= 1.0);
    assert!((footer.bottom() - card.bottom()).abs() <= 1.0);
    assert!(header.bottom() <= body.top());
    assert!(body.bottom() <= footer.top() + 0.5);
    for button in [apply, cancel, ok] {
        assert!(footer.contains_rect(button));
    }
    assert!(define.bottom() <= footer.top() + 0.5, "the symbol row is above the Cancel / OK bar");
    for button in [cancel, ok] {
        assert!((button.height() - t.form_button_height).abs() <= 0.5);
    }
    assert!(!define.intersects(cancel));
    assert!(apply.right() <= cancel.left(), "New weld bead is at the left of the normal Cancel / OK pair");
    assert!(!cancel.intersects(ok));
    println!(
        "live dialog headless host: card {card:?}; header {header:?}; body {body:?}; footer {footer:?}; Define {define:?}; Cancel {cancel:?}; OK {ok:?}"
    );
}

#[test]
fn weld_params_save_type_sizes_names_optional_values_and_symbols() {
    crate::i18n::install();
    let mut s = Session::default();
    let mut dialog = WeldDialog::default();
    for info in crate::weld_panel::TYPES {
        dialog.panel.pick_type(info.weld_type);
        dialog.panel.ends = 1; // The real panel order is square, tapered, rounded.
        dialog.panel.spot_look = 3;
        let p = params(&dialog).unwrap();
        let typ = p["weld_type"].as_str().unwrap();
        let keys: Vec<_> = p["size"].as_object().unwrap().keys().map(String::as_str).collect();
        assert!(keys.iter().all(|key| solidcraft_engine::doc::weld::size_keys(typ).contains(key)));
        assert_eq!(p["ends"], "tapered");
        assert_eq!(p["spot_look"], "marks_pin");
        assert_eq!(p["reference_plane"], "sketch");
        assert_eq!(p["side"], "arrow");
        assert!(p.get("density").is_none());
        assert!(p.get("overlap").is_none());
        assert!(p.get("texture").is_none());
        assert!(p.get("tack").is_none());
        let supported = matches!(info.weld_type, crate::weld_panel::WeldType::Fillet | crate::weld_panel::WeldType::Butt);
        assert_eq!(!p["symbol"].is_null(), supported, "{typ}");
        if supported {
            assert_eq!(p["symbol"]["tail"], p["process"]);
        }
        if typ == "butt" {
            assert_eq!(keys, ["h"]);
            assert!(p["symbol"]["arrow_side"]["size"].is_null());
            assert_eq!(p["symbol"]["arrow_side"]["contour"], "flush");
        }
        s.execute("weld.create", &p).unwrap();
        assert_eq!(s.doc.welds.last().unwrap().symbol.is_some(), supported);
    }
    let back = solidcraft_engine::doc::format::read(&serde_json::to_string(&s.doc).unwrap()).unwrap();
    assert_eq!(back.welds, s.doc.welds);
    dialog.panel.pick_type(crate::weld_panel::WeldType::Fillet);
    assert_eq!(params(&dialog).unwrap()["size"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["a"]);
    dialog.panel.size_by_leg = true;
    assert_eq!(params(&dialog).unwrap()["size"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["z"]);
    dialog.panel.advanced_open = true;
    dialog.panel.size.z1 = "4".into();
    dialog.panel.size.z2 = "6".into();
    assert_eq!(params(&dialog).unwrap()["size"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["z", "z1", "z2"]);
    dialog.panel.spans = true;
    dialog.panel.size.z1.clear();
    dialog.panel.size.z2.clear();
    dialog.panel.span_count = "3".into();
    dialog.panel.span_length = "10".into();
    dialog.panel.span_pitch = "40".into();
    dialog.panel.density = "7850".into();
    dialog.panel.overlap = "2".into();
    let p = params(&dialog).unwrap();
    assert_eq!(p["spans"], json!({"count":3,"length_mm":"10","pitch_mm":"40"}));
    assert_eq!(p["symbol"]["arrow_side"]["length_mm"], 10.);
    assert_eq!(p["symbol"]["arrow_side"]["intermittent"], json!({"count":3,"pitch_mm":40.}));
    s.execute("weld.create", &p).unwrap();
    assert_eq!(s.doc.welds.last().unwrap().settings.density, Some(7850.));
    dialog.panel.span_count = "1.5".into();
    assert!(params(&dialog).is_err());
}

#[test]
fn weld_tack_modes_and_butt_choices_save_only_applicable_dimensions() {
    use crate::weld_panel::WeldType;
    let mut dialog = WeldDialog::default();
    dialog.panel.pick_type(WeldType::Tack);
    for (mode, keys) in [(0, vec!["e", "l"]), (1, vec!["l", "n"]), (2, vec!["l"])] {
        dialog.panel.tack_mode = mode;
        let p = params(&dialog).unwrap();
        assert_eq!(p["size"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), keys);
        assert_eq!(p["tack_mode"], ["pitch", "count", "points"][mode]);
        let mut app = app();
        app.run("weld.create", p).unwrap();
        assert_eq!(app.session.doc.welds.len(), 1);
    }
    dialog.panel.pick_type(WeldType::Butt);
    dialog.panel.size.s = "2".into();
    assert!(params(&dialog).unwrap()["size"].get("s").is_none());
    dialog.panel.partial_penetration = true;
    assert_eq!(params(&dialog).unwrap()["size"]["s"], "2");
    for (groove, supported) in (0..9).map(|i| (i, i < 2)) {
        dialog.panel.groove = groove;
        let p = params(&dialog).unwrap();
        assert_eq!(p["groove"], solidcraft_engine::doc::weld::GROOVES[groove]);
        assert_eq!(!p["symbol"].is_null(), supported);
    }
    dialog.panel.root = 2;
    dialog.panel.symbol_standard = "aws_a2_4_2020".into();
    let p = params(&dialog).unwrap();
    assert_eq!(p["root"], "backing_run");
    assert_eq!(p["symbol_standard"], "aws_a2_4_2020");
}

#[test]
fn weld_standard_preference_is_validated_reset_on_design_and_copied_at_start() {
    let mut app = app();
    assert_eq!(app.preferences.weld_symbol_standard, "iso2553_2019_a");
    for standard in solidcraft_engine::doc::weld::SYMBOL_STANDARDS {
        crate::prefs::control(&mut app, &json!({"weld_symbol_standard":standard})).unwrap();
        let (kind, _) = start(&app, "weld.create").unwrap();
        let Kind::Weld(dialog) = kind else { panic!() };
        assert_eq!(dialog.panel.symbol_standard, *standard);
        let p = params(&dialog).unwrap();
        assert_eq!(p["symbol_standard"], *standard);
        let saved = serde_json::to_value(&app.preferences).unwrap();
        let restored: crate::prefs::Prefs = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.weld_symbol_standard, *standard);
    }
    for invalid in [json!("jis"), json!(3), json!(null)] {
        let before = app.preferences.clone();
        assert!(crate::prefs::control(&mut app, &json!({"weld_symbol_standard":invalid})).is_err());
        assert_eq!(app.preferences, before);
    }
    crate::prefs::control(&mut app, &json!({"open":"Design","button":"defaults"})).unwrap();
    assert_eq!(app.preferences.weld_symbol_standard, "iso2553_2019_a");
}
