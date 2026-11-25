// Per-component History hiding (#10393) through the same commands the Browser menu runs.
use crate::Session;
use serde_json::json;

#[test]
fn hide_and_show_history_mark_the_component_flag_and_undo() {
    let mut s = Session::default();
    s.execute("component.create", &json!({ "name": "Copy" })).unwrap();
    s.execute("component.activate", &json!({ "component": "root" })).unwrap();
    assert_eq!(s.execute("component.hide_history", &json!({ "component": "Copy" })).unwrap()["history_hidden"], json!(true));
    assert!(s.doc.components.iter().find(|c| c.name == "Copy").unwrap().history_hidden);
    assert_eq!(s.execute("component.show_history", &json!({ "component": "Copy" })).unwrap()["history_hidden"], json!(false));
    assert!(!s.doc.components.iter().find(|c| c.name == "Copy").unwrap().history_hidden);
    // Undo of the show puts the flag back; undo of the hide takes it away.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.components.iter().find(|c| c.name == "Copy").unwrap().history_hidden);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.doc.components.iter().find(|c| c.name == "Copy").unwrap().history_hidden);
}

#[test]
fn hide_history_needs_a_real_component_and_survives_a_save_round_trip() {
    let mut s = Session::default();
    s.execute("component.create", &json!({ "name": "Copy" })).unwrap();
    assert!(s.execute("component.hide_history", &json!({ "component": "root" })).is_err());
    s.execute("component.hide_history", &json!({ "component": "Copy" })).unwrap();
    // The flag rides in the design JSON and comes back on read.
    let json = s.doc.to_json();
    let read = solidcraft_doc::Document::from_json(&json).unwrap();
    assert!(read.components.iter().find(|c| c.name == "Copy").unwrap().history_hidden);
    // A design without the flag reads as shown, as always.
    let plain = solidcraft_doc::Document::from_json(&Session::default().doc.to_json()).unwrap();
    assert!(!plain.components.iter().any(|c| c.history_hidden));
}
