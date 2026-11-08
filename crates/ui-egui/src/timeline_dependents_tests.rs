//! The Dependents button of INSPECT ▸ HISTORY asks the engine for the dependents of ONE
//! feature. The button used to run `timeline.dependents` with no `feature` at all, and the
//! engine answered "`feature` must be an id or name" (#12007). With nothing selected it now
//! says what to pick, and with a feature selected it asks about that feature.

use serde_json::json;

use crate::scenario::Harness;

/// A design with a body and a feature on it, so the timeline has something to ask about.
fn design() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "S1"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 5})).unwrap();
    h.frames(2);
    h
}

/// With nothing selected the button says what to pick, and the engine is never asked.
#[test]
fn the_dependents_button_with_nothing_selected_says_what_to_pick() {
    let mut h = design();
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(2);

    // What the ribbon button does: `app.start(id)` with no parameters.
    h.app.start("timeline.dependents");
    h.frames(2);

    let (text, _, error) = h.app.status.clone().expect("the button says what to pick");
    assert!(!error, "the button must not report a failure: {text}");
    assert_eq!(text, h.app.tr("ui.dependents_select_first"));
    assert!(!text.contains("must be an id or name"), "{text}");
}

/// With a feature selected the button asks the engine about that feature, and the engine
/// answers instead of refusing the missing `feature`.
#[test]
fn the_dependents_button_with_a_feature_selected_asks_the_engine() {
    let mut h = design();
    let id = h.app.session.doc.find_feature("S1").expect("the sketch").id;
    h.app.run("select.set", json!({"items": [{"type": "feature", "id": id}]})).unwrap();
    h.frames(2);

    h.app.start("timeline.dependents");
    h.frames(2);

    let log = h.app.session.log.join("\n");
    assert!(!log.contains("must be an id or name"), "the engine refused the feature: {log}");
    assert!(log.contains("Feature Dependents done"), "the engine never answered: {log}");
}
