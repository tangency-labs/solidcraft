use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

/// The Coincident tool takes two circles clicked by their edges and lays the second on the first.
#[test]
fn the_coincident_tool_lays_one_circle_on_another() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":10})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[30,0],"radius":6})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h.step(&json!({"start":"sketch.constraint.coincident"})).unwrap();
    h.step(&json!({"click":{"world":[0,10,0]}})).unwrap();
    h.step(&json!({"click":{"world":[30,6,0]}})).unwrap();
    let st = h.app.session.world_state();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    use solidcraft_engine::sketch::ConstraintKind;
    let laid = sk.constraints.iter().filter(|k| matches!(k.kind, ConstraintKind::Concentric { .. } | ConstraintKind::Equal { .. })).count();
    assert_eq!(laid, 2, "{:?} {:?}", sk.constraints, h.app.status);
    let c2 = sk.curve_index("c2").unwrap();
    let solidcraft_engine::sketch::CurveKind::Circle { c, r } = sk.curves[c2].kind else { panic!("c2 is a circle") };
    assert!(sk.point(c).unwrap().len() < 1e-6 && (r - 10.0).abs() < 1e-6);
}
