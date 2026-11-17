#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcore_record::brep::Curve;
use solidcraft_geom::prelude::*;

use crate::{EngineError, Session};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {params}: {e}"))
}

fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 30, "height": 10, "corner": [0,0,0], "body_name": "Board"}));
    s
}

fn check(s: &mut Session, body: &str, direction: [f64; 3], length: f64) {
    let before = s.doc.clone();
    let undo = s.undo.len();
    let reply = run(s, "wood.grain.query", json!({"body": body}));
    let got: [f64; 3] = serde_json::from_value(reply["direction"].clone()).unwrap();
    assert!(Vec3::from(got).dist(Vec3::from(direction)) < 1e-7, "{reply}");
    assert!((reply["stock_length"].as_f64().unwrap() - length).abs() < 1e-6, "{reply}");
    assert_eq!(s.doc, before);
    assert_eq!(s.undo.len(), undo);
}

fn reopen(s: &Session) -> Session {
    let bytes = solidcraft_io::write_design(&s.doc);
    Session::new(solidcraft_io::read_design(&bytes).unwrap())
}

#[test]
fn edge_grain_survives_save_reopen_and_body_rotation() {
    let mut s = block();
    let state = s.world_state().unwrap();
    let body = state.body("Board").unwrap();
    let edge = body
        .body
        .record()
        .edges
        .iter()
        .position(|edge| matches!(edge.curve, Curve::Line { start, end } if Vec3::from(end) - Vec3::from(start) == Vec3::new(100., 0., 0.)))
        .unwrap();
    run(&mut s, "wood.grain", json!({"body": "Board", "direction": {"edge": edge}}));
    check(&mut s, "Board", [1., 0., 0.], 100.);
    let mut s = reopen(&s);
    check(&mut s, "Board", [1., 0., 0.], 100.);
    run(&mut s, "solid.move", json!({"bodies": ["Board"], "translate": [20,30,40], "axis": [0,0,1], "angle": "90 deg"}));
    check(&mut s, "Board", [0., 1., 0.], 100.);
    let mut s = reopen(&s);
    check(&mut s, "Board", [0., 1., 0.], 100.);
}

#[test]
fn a_vector_set_after_rotation_is_saved_in_construction_coordinates() {
    let mut s = block();
    run(&mut s, "solid.move", json!({"bodies": ["Board"], "axis": [0,0,1], "angle": "90 deg"}));
    run(&mut s, "wood.grain", json!({"body": "Board", "direction": [0,8,0]}));
    check(&mut s, "Board", [0., 1., 0.], 100.);
    assert!(s.doc.wood_grain["Board"].direction.unwrap()[0] > 0.999999);
    run(&mut s, "solid.move", json!({"bodies": ["Board"], "axis": [1,0,0], "angle": "90 deg"}));
    check(&mut s, "Board", [0., 0., 1.], 100.);
    check(&mut reopen(&s), "Board", [0., 0., 1.], 100.);
}

#[test]
fn stock_axis_names_use_body_axes_after_rotation() {
    let mut s = block();
    run(&mut s, "solid.move", json!({"bodies": ["Board"], "axis": [0,0,1], "angle": "90 deg"}));
    let stock = crate::kernel::stock_box(&s.model.state().body("Board").unwrap().body).unwrap();
    for ((axis, vector, length), stock_axis) in
        [("length", [0., 1., 0.], 100.), ("width", [-1., 0., 0.], 30.), ("thickness", [0., 0., 1.], 10.)].into_iter().zip(stock.axes)
    {
        run(&mut s, "wood.grain", json!({"body": "Board", "direction": axis}));
        check(&mut s, "Board", stock_axis, length);
        assert!((Vec3::from(stock_axis).dot(Vec3::from(vector)).abs() - 1.0).abs() < 1e-7);
    }
}

#[test]
fn a_rotated_boards_grain_length_axis_matches_the_stock_measure() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":600,"width":90,"height":20,"body_name":"Board"}));
    run(&mut s, "solid.move", json!({"bodies":["Board"],"axis":[0,0,1],"angle":"30 deg"}));
    let stock = crate::kernel::stock_box(&s.model.state().body("Board").unwrap().body).unwrap();
    assert!((stock.dimensions[0] - 600.0).abs() < 1e-7);
    let grain = run(&mut s, "wood.grain", json!({"body":"Board","direction":"length"}));
    let direction: [f64; 3] = serde_json::from_value(grain["direction"].clone()).unwrap();
    assert!(Vec3::from(direction).dist(Vec3::from(stock.axes[0])) < 1e-7, "{grain}; {stock:?}");
    // A 30 degree rotation takes X to [sqrt(3)/2, 1/2, 0].
    let expected = Vec3::new(3.0_f64.sqrt() / 2.0, 0.5, 0.0);
    assert!((Vec3::from(direction).dot(expected).abs() - 1.0).abs() < 1e-7);
    assert!((grain["stock_length"].as_f64().unwrap() - stock.dimensions[0]).abs() < 1e-7);
}

#[test]
fn face_and_edge_define_an_in_plane_grain_axis() {
    let mut s = block();
    let state = s.world_state().unwrap();
    let body = state.body("Board").unwrap();
    let (index, edge) = body
        .body
        .record()
        .edges
        .iter()
        .enumerate()
        .find(|(_, edge)| matches!(edge.curve, Curve::Line { start, end } if Vec3::from(end) - Vec3::from(start) == Vec3::new(100., 0., 0.)))
        .unwrap();
    let face = edge.uses.first().unwrap().face;
    run(&mut s, "wood.grain", json!({"body": "Board", "direction": {"edge": index, "face": face}}));
    check(&mut s, "Board", [1., 0., 0.], 100.);
}

#[test]
fn an_unset_frame_member_follows_its_diagonal_sketch_line_and_none_overrides_it() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"name":"Path", "plane":"XY"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[300,400]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch":"Path", "profile":{"kind":"bar", "width":30, "height":10}, "body_name":"Rail"}));
    check(&mut s, "Rail", [0.6, 0.8, 0.], 500.);
    assert!(s.doc.wood_grain.is_empty());
    check(&mut reopen(&s), "Rail", [0.6, 0.8, 0.], 500.);
    run(&mut s, "solid.move", json!({"bodies":["Rail"], "axis":[0,0,1],"angle":"90 deg"}));
    check(&mut s, "Rail", [-0.8, 0.6, 0.], 500.);
    run(&mut s, "wood.grain", json!({"body":"Rail", "direction":"none"}));
    let reply = run(&mut reopen(&s), "wood.grain.query", json!({"body":"Rail"}));
    assert!(reply["direction"].is_null() && reply["stock_length"].is_null());
}

#[test]
fn ordinary_bodies_have_no_implicit_grain() {
    let mut s = block();
    let reply = run(&mut s, "wood.grain.query", json!({"body":"Board"}));
    assert!(reply["direction"].is_null() && reply["stock_length"].is_null());
    assert!(s.doc.wood_grain.is_empty());
}

#[test]
fn copied_mirrored_and_patterned_bodies_inherit_rotated_grain() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board", "direction":[1,0,0]}));
    let copy = run(&mut s, "solid.move", json!({"bodies":["Board"], "translate":[0,200,0], "axis":[0,0,1], "angle":"90 deg", "copy":true}));
    let copy_name = copy["created_bodies"][0].as_str().unwrap().to_string();
    check(&mut s, &copy_name, [0., 1., 0.], 100.);
    let mirror = run(&mut s, "solid.mirror", json!({"bodies":["Board"], "plane":"YZ"}));
    let mirror_name = mirror["created_bodies"][0].as_str().unwrap().to_string();
    check(&mut s, &mirror_name, [-1., 0., 0.], 100.);
    let pattern =
        run(&mut s, "solid.pattern.circular", json!({"bodies":["Board"], "axis":{"origin":[0,0,0], "dir":[0,0,1]}, "count":2, "angle":"180 deg"}));
    let pattern_name = pattern["created_bodies"][0].as_str().unwrap().to_string();
    check(&mut s, &pattern_name, [-1., 0., 0.], 100.);
    let mut s = reopen(&s);
    check(&mut s, &copy_name, [0., 1., 0.], 100.);
    check(&mut s, &mirror_name, [-1., 0., 0.], 100.);
    check(&mut s, &pattern_name, [-1., 0., 0.], 100.);
}

#[test]
fn hostile_grain_inputs_are_bad_params_and_leave_the_document_unchanged() {
    let mut s = block();
    run(&mut s, "solid.box", json!({"body_name":"Other", "length":5,"width":5,"height":5,"corner":[200,0,0]}));
    let before = s.doc.clone();
    let undo = s.undo.len();
    for params in [
        json!({"body":"Missing", "direction":[1,0,0]}),
        json!({"body":"Board", "direction":[0,0,0]}),
        json!({"body":"Board", "direction":{"body":"Other", "edge":0}}),
        json!({"body":"Board", "direction":{"edge":999999}}),
        json!({"body":"Board", "direction":{"edge":0,"face":999999}}),
        json!({"body":"Board", "direction":[1e308,1e308,0]}),
        json!({"body":"Board", "direction":"longest"}),
        json!({"body":"Board"}),
    ] {
        let error = s.execute("wood.grain", &params).unwrap_err();
        assert!(matches!(error, EngineError::BadParams { .. }), "{params}: {error}");
        assert_eq!(s.doc, before, "{params}");
        assert_eq!(s.undo.len(), undo);
    }
}

#[test]
fn grain_changes_are_undoable_and_renaming_preserves_grain() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board", "direction":"length"}));
    run(&mut s, "edit.undo", json!({}));
    assert!(run(&mut s, "wood.grain.query", json!({"body":"Board"}))["direction"].is_null());
    run(&mut s, "edit.redo", json!({}));
    check(&mut s, "Board", [1., 0., 0.], 100.);
    run(&mut s, "body.rename", json!({"body":"Board","name":"Rail"}));
    check(&mut s, "Rail", [1., 0., 0.], 100.);
}

#[test]
fn cached_reopening_keeps_the_grain_frame_without_rebuilding_geometry() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board","direction":"length"}));
    run(&mut s, "solid.move", json!({"bodies":["Board"],"axis":[0,0,1],"angle":"90 deg"}));
    let cache = s.model.saved(&s.doc).unwrap();
    let mut opened = Session::opened((*s.doc).clone(), Some(&cache));
    check(&mut opened, "Board", [0., 1., 0.], 100.);
    assert_eq!(opened.evaluation_count(), 0);
}

#[test]
fn a_cache_from_before_grain_frames_rebuilds_frame_members_and_keeps_their_grain() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"name":"Path", "plane":"XY"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[300,400]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch":"Path", "profile":{"kind":"bar", "width":30, "height":10}, "body_name":"Rail"}));
    let mut cache = s.model.saved(&s.doc).unwrap();
    for b in cache.records["model_bodies"].as_array_mut().unwrap() {
        b.as_object_mut().unwrap().remove("grain_frame");
    }
    // The cache is refused, so opening evaluates the timeline again and the member keeps its grain.
    assert!(solidcraft_doc::Model::adopt(&s.doc, &cache).is_err());
    let mut opened = Session::opened((*s.doc).clone(), Some(&cache));
    check(&mut opened, "Rail", [0.6, 0.8, 0.], 500.);
}

#[test]
fn an_oblique_vector_measures_exact_projection_of_the_whole_blank() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board","direction":[3,4,0]}));
    // 100 * 3/5 + 30 * 4/5 = 84 mm.
    check(&mut s, "Board", [0.6, 0.8, 0.], 84.);
}

#[test]
fn a_pattern_of_a_board_feature_inherits_its_grain() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board","direction":"length"}));
    let feature = s.doc.features.first().unwrap().name.clone();
    let reply = run(&mut s, "solid.pattern.rectangular", json!({"features":[feature],"dir1":[0,1,0],"count1":2,"spacing1":100}));
    let name = reply["created_bodies"][0].as_str().unwrap().to_string();
    check(&mut s, &name, [1., 0., 0.], 100.);
    check(&mut reopen(&s), &name, [1., 0., 0.], 100.);
}

#[test]
fn renaming_a_source_preserves_inherited_grain_on_its_copies() {
    let mut s = block();
    run(&mut s, "wood.grain", json!({"body":"Board","direction":"length"}));
    let reply = run(&mut s, "solid.move", json!({"bodies":["Board"],"translate":[0,100,0],"copy":true}));
    let name = reply["created_bodies"][0].as_str().unwrap().to_string();
    run(&mut s, "body.rename", json!({"body":"Board","name":"Rail"}));
    check(&mut s, &name, [1., 0., 0.], 100.);
    check(&mut reopen(&s), &name, [1., 0., 0.], 100.);
}
