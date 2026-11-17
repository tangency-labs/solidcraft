//! Router profiles measured against closed-form section areas and Pappus' theorem.

use crate::{EngineError, Session};
use serde_json::{Value, json};
use std::f64::consts::PI;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}
fn board(s: &mut Session) {
    run(s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Board"}));
}
fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < expected.abs().max(1.0) * 1e-6, "{actual} != {expected}");
}

#[test]
fn router_profiles_remove_exact_section_area_times_length() {
    let r = 6.0;
    let cases = [
        ("chamfer", json!({"width":3}), 3.0 * 3.0 / 2.0, 7),
        ("bevel", json!({"width":3,"angle":"30 deg"}), 3.0 * 3.0 / (2.0 * 3.0_f64.sqrt()), 7),
        ("roundover", json!({"radius":r}), r * r * (1.0 - PI / 4.0), 7),
        ("roundover", json!({"radius":r,"fillet":1}), r * r * (1.0 - PI / 4.0) + r + 1.0, 9),
        ("cove", json!({"radius":r}), PI * r * r / 4.0, 7),
        ("rabbet", json!({"width":12,"depth":12}), 12.0 * 12.0, 8),
        ("ogee", json!({"width":12,"depth":12}), 12.0 * 12.0 / 2.0, 8),
        ("bead", json!({"radius":r,"step":1}), r * r * (1.0 - PI / 4.0) + 2.0 * r + 3.0, 11),
    ];
    for (profile, sizes, area, faces) in cases {
        let mut s = Session::default();
        board(&mut s);
        let mut p = json!({"edges":[[50,0,20]],"profile":profile});
        p.as_object_mut().unwrap().extend(sizes.as_object().unwrap().clone());
        let reply = run(&mut s, "wood.edge_profile", p);
        close(volume(&mut s), 100000.0 - area * 100.0);
        close(reply["edges"][0]["length_mm"].as_f64().unwrap(), 100.0);
        let st = s.model.state();
        assert_eq!(st.body("Board").unwrap().body.face_count(), faces, "{profile}");
    }
}

#[test]
fn router_roundover_on_round_table_uses_pappus() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":200,"height":20,"body_name":"Table"}));
    let reply = run(&mut s, "wood.edge_profile", json!({"edges":[[200,0,20]],"profile":"roundover","radius":6,"tangent_chain":true}));
    let r: f64 = 6.0;
    let area = r * r * (1.0 - PI / 4.0);
    // First moment inward from the rim: square minus the retained quarter disk.
    let moment = r.powi(3) * (5.0 / 6.0 - PI / 4.0);
    let removed = 2.0 * PI * (200.0 * area - moment);
    close(volume(&mut s), PI * 200.0 * 200.0 * 20.0 - removed);
    close(reply["edges"].as_array().unwrap().iter().map(|e| e["length_mm"].as_f64().unwrap()).sum(), 400.0 * PI);
    assert_eq!(solidcraft_doc::kernel::measure(&s.model.state().body("Table").unwrap().body).unwrap().merged.faces, 4);
}

#[test]
fn router_edit_and_none_keep_one_feature_and_restore_stock() {
    let mut s = Session::default();
    board(&mut s);
    let first = run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,20]],"profile":"roundover"}));
    let id = first["feature"].as_u64().unwrap();
    let count = s.doc.features.len();
    let changed = run(&mut s, "wood.edge_profile", json!({"feature":id,"profile":"rabbet","width":4,"depth":2}));
    assert_eq!(changed["feature"], id);
    close(volume(&mut s), 100000.0 - 4.0 * 2.0 * 100.0);
    run(&mut s, "wood.edge_profile", json!({"feature":id,"profile":"none"}));
    close(volume(&mut s), 100000.0);
    assert_eq!(s.doc.features.len(), count);
    assert_eq!(s.model.state().body("Board").unwrap().body.face_count(), 6);
}

#[test]
fn router_refuses_hostile_input_without_changing_document() {
    let mut s = Session::default();
    board(&mut s);
    run(&mut s, "solid.box", json!({"length":100,"width":50,"height":20,"corner":[0,100,0],"body_name":"Other"}));
    for p in [
        json!({"edges":[[50,0,20]],"profile":"roundover","radius":21}),
        json!({"edges":[[50,0,20],[50,100,20]],"profile":"rabbet"}),
        json!({"edges":[[50,0,20]],"profile":"unknown"}),
        json!({"edges":[],"profile":"chamfer"}),
        json!({"edges":[{"body":"Board","index":u64::MAX}],"profile":"chamfer"}),
        json!({"edges":[[50,0,20]],"profile":"bevel","angle":90}),
        json!({"edges":[[50,0,20]],"profile":"bead","step":-1}),
    ] {
        let before = serde_json::to_value(&s.doc).unwrap();
        assert!(matches!(s.execute("wood.edge_profile", &p), Err(EngineError::BadParams { .. })), "{p}");
        assert_eq!(serde_json::to_value(&s.doc).unwrap(), before, "{p}");
    }
    let routed = run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,20]],"profile":"roundover"}));
    let before = serde_json::to_value(&s.doc).unwrap();
    let before_volume = volume(&mut s);
    let error = s.execute("wood.edge_profile", &json!({"feature":routed["feature"],"profile":"roundover","radius":21}));
    assert!(matches!(error, Err(EngineError::BadParams { .. })));
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
    close(volume(&mut s), before_volume);
}

#[test]
fn router_multiple_edges_and_expressions_rebuild() {
    let mut s = Session::default();
    board(&mut s);
    run(&mut s, "parameters.change", json!({"name":"router_size","expression":"2 mm"}));
    let reply = run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,20],[50,50,20]],"profile":"chamfer","width":"router_size"}));
    assert_eq!(reply["edges"].as_array().unwrap().len(), 2);
    close(volume(&mut s), 100000.0 - 2.0 * 2.0 / 2.0 * 200.0);
    run(&mut s, "parameters.change", json!({"name":"router_size","expression":"3 mm"}));
    close(volume(&mut s), 100000.0 - 3.0 * 3.0 / 2.0 * 200.0);
}

#[test]
fn router_duplicate_picks_and_record_indices_route_one_edge() {
    let mut source = Session::default();
    board(&mut source);
    let state = source.model.state();
    let record = state.body("Board").unwrap().body.record();
    let index = record
        .edges
        .iter()
        .position(|edge| {
            let a = record.vertices[edge.vertices[0] as usize].point;
            let b = record.vertices[edge.vertices[1] as usize].point;
            (a == [0.0, 0.0, 20.0] && b == [100.0, 0.0, 20.0]) || (b == [0.0, 0.0, 20.0] && a == [100.0, 0.0, 20.0])
        })
        .unwrap();
    for edges in [json!([[25, 0, 20], [75, 0, 20]]), json!([{"body":"Board","index":index}])] {
        let mut s = Session::default();
        board(&mut s);
        let reply = run(&mut s, "wood.edge_profile", json!({"edges":edges,"profile":"chamfer","width":3}));
        close(volume(&mut s), 100000.0 - 3.0 * 3.0 / 2.0 * 100.0);
        assert_eq!(reply["edges"].as_array().unwrap().len(), 1);
        close(reply["edges"][0]["length_mm"].as_f64().unwrap(), 100.0);
        assert_eq!(s.model.state().body("Board").unwrap().body.face_count(), 7);
    }
}

#[test]
fn router_saved_design_rebuilds_the_same_feature() {
    let mut s = Session::default();
    board(&mut s);
    let reply = run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,20]],"profile":"roundover","radius":6,"fillet":1}));
    let cached = s.model.saved(&s.doc).unwrap();
    let pieces: Vec<_> = cached.bodies.iter().enumerate().map(|(i, _)| vec![(i, 1)]).collect();
    let (thinned, _, _) = cached.thinned(&pieces, 0).unwrap();
    let state = thinned.final_state().unwrap();
    assert_eq!(state.routed_lengths[&reply["feature"].as_u64().unwrap()], vec![100.0]);
    let adopted = solidcraft_doc::Model::adopt(&s.doc, &thinned).unwrap();
    assert_eq!(adopted.state().routed_lengths[&reply["feature"].as_u64().unwrap()], vec![100.0]);
    let saved = serde_json::to_value(&s.doc).unwrap();
    let mut reopened = Session::default();
    *reopened.doc_mut() = serde_json::from_value(saved).unwrap();
    reopened.refresh();
    close(volume(&mut reopened), 100000.0 - (36.0 * (1.0 - PI / 4.0) + 7.0) * 100.0);
    let id = reply["feature"].as_u64().unwrap();
    run(&mut reopened, "wood.edge_profile", json!({"feature":id,"profile":"none"}));
    close(volume(&mut reopened), 100000.0);
}

#[test]
fn router_refuses_non_square_support_faces() {
    let mut s = Session::default();
    run(&mut s, "solid.cone", json!({"radius":20,"top_radius":10,"height":20}));
    let before = serde_json::to_value(&s.doc).unwrap();
    let error = s.execute("wood.edge_profile", &json!({"edges":[[10,0,20]],"profile":"roundover","radius":2})).unwrap_err();
    assert!(matches!(error, EngineError::BadParams { .. }));
    assert!(error.to_string().contains("90"), "{error}");
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
}

#[test]
fn router_roundover_follows_a_quarter_circle_edge_exactly() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Outline"}));
    run(&mut s, "sketch.arc.center_point", json!({"center":[0,0],"start":[200,0],"sweep":90,"id":"rim"}));
    run(&mut s, "sketch.line", json!({"points":["rim.end",[0,0],"rim.start"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Outline","distance":20,"body_name":"ArcBoard"}));
    let q = 200.0 * std::f64::consts::FRAC_1_SQRT_2;
    let reply = run(&mut s, "wood.edge_profile", json!({"edges":[[q,q,20]],"profile":"roundover","radius":6}));
    let area = 36.0 * (1.0 - PI / 4.0);
    let moment = 216.0 * (5.0 / 6.0 - PI / 4.0);
    let removed = PI / 2.0 * (200.0 * area - moment);
    close(volume(&mut s), PI * 200.0 * 200.0 * 20.0 / 4.0 - removed);
    close(reply["edges"][0]["length_mm"].as_f64().unwrap(), 100.0 * PI);
    assert_eq!(solidcraft_doc::kernel::measure(&s.model.state().body("ArcBoard").unwrap().body).unwrap().merged.faces, 6);
}

#[test]
fn router_feature_is_redefined_in_place_from_its_dialog_command() {
    let mut s = Session::default();
    board(&mut s);
    let id = run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,20]],"profile":"chamfer","width":3}))["feature"].as_u64().unwrap();
    let count = s.doc.features.len();
    let edit = json!({"feature": id, "command": "wood.edge_profile", "params": {"edges":[[50,0,20]],"profile":"roundover","radius":6}});
    run(&mut s, "timeline.redefine", edit);
    assert_eq!(s.doc.features.len(), count, "the edit replaces the feature, it adds none");
    assert!(matches!(&s.doc.feature(id).unwrap().kind, solidcraft_doc::FeatureKind::WoodEdgeProfile { profile, .. } if profile == "roundover"));
    close(volume(&mut s), 100000.0 - 6.0 * 6.0 * (1.0 - PI / 4.0) * 100.0);
}
