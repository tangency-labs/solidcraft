use super::*;

#[test]
fn ground_shadow_preserves_legacy_preferences_and_validates_the_style() {
    for (saved, expected) in [(json!({}), "ellipse"), (json!({"ground_shadow":true}), "ellipse"), (json!({"ground_shadow":false}), "off")] {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.load_prefs(&saved.to_string());
        assert_eq!(ground_shadow_mode(&app), expected);
    }
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    assert!(set(&mut app, &json!({"ground_shadow_style":"height_bands"})).is_err());
    assert_eq!(ground_shadow_mode(&app), "ellipse");
    app.load_prefs(&json!({"ground_shadow":true,"preferences":{"ground_shadow_style":"unknown","msaa":1}}).to_string());
    assert_eq!(ground_shadow_mode(&app), "ellipse");
    assert_eq!(app.preferences.msaa, 1);
}

#[test]
fn ground_shadow_selector_is_shared_persistent_and_cancelable_without_changing_ao() {
    let mut h = crate::scenario::Harness::new();
    h.call("engine.execute", json!({"command":"solid.box","params":{"length":10,"width":10,"height":10}}));
    h.call("ui.inspect", json!({}));
    let camera = h.app.cam;
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for handle in ["nav:view", "view:ground_shadow_style", "view:ground_shadow:projected"] {
        let p = crate::scenario::handle_at(handle).unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
    }
    assert_eq!(ground_shadow_mode(&h.app), "projected");
    assert!(!h.app.preferences.ambient_occlusion);
    // Close the View menu before opening another window, as a normal click outside does.
    h.call("ui.key", json!({"key":"Escape"}));
    h.call("ui.prefs", json!({"open":"Graphics"}));
    reveal_preference(&mut h, "view:ground_shadow_selector");
    let p = crate::scenario::handle_at("view:ground_shadow_selector").unwrap();
    h.call("ui.click", json!({"x":p.x,"y":p.y}));
    let p = crate::scenario::handle_at("view:ground_shadow:off").unwrap();
    h.call("ui.click", json!({"x":p.x,"y":p.y}));
    assert_eq!(ground_shadow_mode(&h.app), "off");
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(ground_shadow_mode(&h.app), "projected");
    h.call("ui.prefs", json!({"open":"Graphics","button":"apply"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(ground_shadow_mode(&h.app), "ellipse");
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(ground_shadow_mode(&h.app), "projected");
    let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(ground_shadow_mode(&restored), "projected");
    for preset in ["quality", "performance"] {
        h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":preset}}));
        assert_eq!(ground_shadow_mode(&h.app), "projected");
    }
    h.call("engine.execute", json!({"command":"view.ground_shadow","params":{"mode":"off"}}));
    assert_eq!(ground_shadow_mode(&h.app), "off");
    h.call("engine.execute", json!({"command":"view.ground_shadow","params":{"mode":"invalid"}}));
    assert_eq!(ground_shadow_mode(&h.app), "off");
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.undo.len(), undo);
}
