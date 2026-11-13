//! The Parameters dialog's ▲ / ▼ buttons reorder user parameters (#1187).
use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

fn order(h: &mut Harness) -> Vec<String> {
    let v = h.app.session.execute("parameters.list", &json!({"filter": "user"})).unwrap();
    v["parameters"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn arrows_move_user_parameters_up_and_down() {
    let mut h = Harness::new();
    for (n, e) in [("a", "1 mm"), ("b", "2 mm"), ("c", "3 mm")] {
        h.app.session.execute("parameters.add", &json!({"name": n, "expression": e})).unwrap();
    }
    // The Browser's Parameters row starts this command; the ribbon shows only the active tab.
    h.call("ui.start", json!({"command":"parameters.change"}));
    h.frames(3);
    assert!(h.app.params.is_some());
    h.frames(3);
    h.run(&json!([{"click":{"handle":"param:after:a"}}])).unwrap();
    h.frames(2);
    assert_eq!(order(&mut h), ["b", "a", "c"]);
    h.run(&json!([{"click":{"handle":"param:before:c"}}])).unwrap();
    h.frames(2);
    assert_eq!(order(&mut h), ["b", "c", "a"]);
    // The first row cannot move up, the last cannot move down.
    h.run(&json!([{"click":{"handle":"param:before:b"}}, {"click":{"handle":"param:after:a"}}])).unwrap();
    h.frames(2);
    assert_eq!(order(&mut h), ["b", "c", "a"]);
    assert!(h.app.params.as_ref().unwrap().error.is_none());
}
