use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn bottom_rounds() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name":"corner_radius", "expression":"2 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Rectangle"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[25,25]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":"10 mm", "body_name":"Block"}));
    run(&mut s, "solid.fillet", json!({"edges":[[12.5,0,0],[25,12.5,0],[12.5,25,0],[0,12.5,0]], "radius":"2 mm"}));
    s
}

fn volume(s: &mut Session) -> f64 {
    let value = run(s, "inspect.measure", json!({}));
    assert_eq!(value["body_count"], 1);
    let state = s.model.state();
    let body = &state.body("Block").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    value["total"]["volume_mm3"].as_f64().unwrap()
}

#[test]
fn issue1557_fillet_recomputes_and_survives_undo_and_reload() {
    for edges in [json!([[25, 0, 6]]), json!([[25, 0, 6], [24.287266336668306, 0.7127336633316916, 0.48266373325510686]])] {
        let mut s = bottom_rounds();
        let before = volume(&mut s);
        let out = run(&mut s, "solid.fillet", json!({"edges":edges, "radius":"corner_radius"}));
        let id = out["feature"].as_u64().unwrap();
        let after = volume(&mut s);
        assert!(after < before - 1.0);
        assert_eq!(s.doc.features.len(), 4);
        run(&mut s, "edit.undo", json!({}));
        assert!((volume(&mut s) - before).abs() < 1e-6);
        run(&mut s, "edit.redo", json!({}));
        assert!((volume(&mut s) - after).abs() < 1e-6);
        run(&mut s, "parameters.change", json!({"name":"corner_radius", "expression":"1 mm"}));
        let smaller = volume(&mut s);
        assert!(smaller > after && smaller < before);
        assert!(run(&mut s, "timeline.inspect", json!({"feature":id}))["error"].is_null());
        let bytes = crate::io::write_design_container(&s.doc).unwrap();
        let mut reopened = Session::new(crate::io::read_design(&bytes).unwrap());
        assert!((volume(&mut reopened) - smaller).abs() < 1e-6);
        assert_eq!(reopened.doc.features.len(), 4);
    }
}

#[test]
fn issue1557_failed_radius_keeps_the_existing_rounds() {
    let mut s = bottom_rounds();
    let before = s.doc.to_json();
    let v = volume(&mut s);
    let undo = s.history.len();
    assert!(s.execute("solid.fillet", &json!({"edges":[[25,0,6]], "radius":"1000 mm"})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.history.len(), undo);
    assert!((volume(&mut s) - v).abs() < 1e-6);
}
