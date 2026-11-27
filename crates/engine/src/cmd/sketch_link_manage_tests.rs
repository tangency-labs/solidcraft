//! Managing projections: whole face boundaries against single edges, construction and profile
//! status, removing members and whole links, hiding, freezing, pending moves, repair and linked
//! design files.

use crate::Session;
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, LinkGeom, Sketch};

#[test]
fn projection_member_distance_stays_on_the_segment_and_handles_collapsed_edges() {
    let a = Vec2::ZERO;
    let b = Vec2::new(4.0, 0.0);
    assert_eq!(super::seg_dist(Vec2::new(2.0, 3.0), a, b), 3.0);
    assert_eq!(super::seg_dist(Vec2::new(7.0, 4.0), a, b), 5.0);
    assert_eq!(super::seg_dist(Vec2::new(-3.0, 4.0), a, b), 5.0);
    assert_eq!(super::seg_dist(Vec2::new(3.0, 4.0), a, a), 5.0);
    assert_eq!(super::seg_dist(Vec2::new(f64::NAN, 0.0), a, b), f64::INFINITY);
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e}"))
}

/// Why `sketch.delete` (its result `r`) broke off what remained of `link` (empty: it did not).
fn broken_because(r: &Value, link: &str) -> String {
    let broken = r["broken_links"].as_array().cloned().unwrap_or_default();
    broken.iter().find(|b| b["link"] == link).and_then(|b| b["why"].as_str()).unwrap_or_default().to_string()
}

/// A 40 × 40 × 10 plate with a Ø10 hole through its middle (part A, placed twice: A:2 at
/// x = 100), and part B with a sketch on its XY plane. Returns (session, a1, a2, sketch).
fn holed_parts() -> (Session, u64, u64, u64) {
    let mut s = Session::default();
    let a1 = run(&mut s, "component.create", json!({"name":"A"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":10,"base":[20,20,0],"operation":"cut","targets":["Plate"]}));
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[100,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Receiver"}))["sketch"].as_u64().unwrap();
    (s, a1, a2, sketch)
}

fn top_face_of_second(s: &mut Session, a2: u64) -> String {
    let r = run(s, "sketch.project", json!({"refs":[{"face":[105,5,10],"body":"Plate","path":[a2]}]}));
    r["links"][0]["link"].as_str().unwrap().to_string()
}

/// The kinds of a link's curves in the evaluated sketch (lines, rounds).
fn shape(s: &Session, sketch: u64, link: &str) -> (usize, usize) {
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    let curves = sk.link_curves(link);
    let lines = curves.iter().filter(|c| matches!(sk.curves[**c].kind, CurveKind::Line { .. })).count();
    let rounds = curves.iter().filter(|c| matches!(sk.curves[**c].kind, CurveKind::Circle { .. } | CurveKind::Arc { .. })).count();
    (lines, rounds)
}

fn profiles(s: &Session, sketch: u64) -> usize {
    s.model.state().sketch(sketch).unwrap().profiles.len()
}

fn hole_centre(s: &Session, sketch: u64, link: &str) -> Vec2 {
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    sk.link_curves(link)
        .into_iter()
        .find_map(|c| match sk.curves[c].kind {
            CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => sk.point(c),
            _ => None,
        })
        .unwrap()
}

/// A script reaches a copy by naming its body ("Plate (A:2)") as well as by `path`, and
/// follows that copy when it moves.
#[test]
fn a_copy_is_projected_by_its_body_name() {
    let (mut s, _, a2, sketch) = holed_parts();
    let r = run(&mut s, "sketch.project", json!({"refs":[{"face":[105,5,10],"body":"Plate (A:2)"}]}));
    let face = r["links"][0]["link"].as_str().unwrap().to_string();
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6);
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,7,0]}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 27.0)).len() < 1e-6);
}

#[test]
fn a_face_brings_every_boundary_loop_and_an_edge_only_itself() {
    let (mut s, _, a2, sketch) = holed_parts();
    // The edge first: projected after the face, it would already be in the sketch and be refused.
    let edge = run(&mut s, "sketch.project", json!({"refs":[{"edge":[110,0,10],"body":"Plate","path":[a2]}]}))["links"][0]["link"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(shape(&s, sketch, &edge), (1, 0), "an edge brings only itself");
    let face = top_face_of_second(&mut s, a2);
    let (lines, rounds) = shape(&s, sketch, &face);
    assert_eq!(lines, 4, "the outer boundary's four edges");
    assert!(rounds >= 1, "and the hole's loop");
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6);
    // The plate's inner face and outer loop make a profile with the hole as an island.
    assert!(profiles(&s, sketch) >= 2, "the plate ring and the hole");
    let e = s.execute("sketch.project", &json!({"refs":[{"edge":[110,0,10],"body":"Plate","path":[a2]}]})).unwrap_err().to_string();
    assert!(e.contains("already projected"), "a placed edge already shown is refused, as any other: {e}");
    // Both follow A:2; the face keeps its hole where A:2 puts it.
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,7,0]}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 27.0)).len() < 1e-6);
    assert_eq!(shape(&s, sketch, &face).0, 4);
}

#[test]
fn construction_takes_projections_out_of_profiles_and_survives_moves_and_reopen() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    let curves: Vec<String> = {
        let sk = s.doc.sketch(sketch).unwrap();
        sk.link_curves(&face).iter().map(|c| sk.curves[*c].id.clone()).collect()
    };
    assert!(profiles(&s, sketch) >= 1);
    run(&mut s, "sketch.construction", json!({"curves":curves,"value":true}));
    assert_eq!(profiles(&s, sketch), 0, "construction curves bound no profile");
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.link(&face).is_some(), "still linked");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,5,0]}));
    run(&mut s, "sketch.update_references", json!({}));
    let check = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        assert!(sk.link_curves(&face).iter().all(|c| sk.curves[*c].construction));
        assert!((hole_centre(s, sketch, &face) - Vec2::new(120.0, 25.0)).len() < 1e-6);
        assert_eq!(st.sketch(sketch).unwrap().profiles.len(), 0);
    };
    check(&s);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    check(&reopened);
    // Back to normal lines: profiles again, still linked.
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.construction", json!({"curves":curves,"value":false}));
    assert!(profiles(&s, sketch) >= 1);
}

#[test]
fn deleted_members_stay_deleted_and_shared_points_stay() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    // One outer edge of the face goes; its neighbours keep their shared corners.
    let (victim, kept_points) = {
        let sk = s.doc.sketch(sketch).unwrap();
        let line = sk.link_curves(&face).into_iter().find(|c| matches!(sk.curves[*c].kind, CurveKind::Line { .. })).unwrap();
        (sk.curves[line].id.clone(), sk.link_points(&face).len())
    };
    run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":[victim]}));
    let after = |s: &Session| shape(s, sketch, &face);
    assert_eq!(after(&s).0, 3);
    assert_eq!(s.doc.sketch(sketch).unwrap().link_points(&face).len(), kept_points, "the corners are still used by the neighbours");
    // An update, a move of A:2 and a reopen do not bring it back.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,3,0]}));
    run(&mut s, "sketch.update_references", json!({}));
    assert_eq!(after(&s).0, 3);
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 23.0)).len() < 1e-6);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    assert_eq!(after(&reopened).0, 3);
    // Undo brings the member back.
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(after(&s).0, 4);
}

#[test]
fn removing_a_link_removes_its_geometry_and_constraints_but_break_keeps_it() {
    let (mut s, _, a2, sketch) = holed_parts();
    let edge = run(&mut s, "sketch.project", json!({"refs":[{"edge":[110,40,10],"body":"Plate","path":[a2]}]}))["links"][0]["link"]
        .as_str()
        .unwrap()
        .to_string();
    let face = top_face_of_second(&mut s, a2);
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[30,0]]}))["curves"][0].as_str().unwrap().to_string();
    let projected = {
        let sk = s.doc.sketch(sketch).unwrap();
        sk.curves[sk.link_curves(&face)[0]].id.clone()
    };
    run(&mut s, "sketch.constraint.parallel", json!({"a":line,"b":projected}));
    let before = s.doc.sketch(sketch).unwrap().constraints.len();
    let r = run(&mut s, "sketch.remove_link", json!({"link":face}));
    assert_eq!(r["removed"], json!([face]));
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.link(&face).is_none());
    assert!(sk.curves.iter().all(|c| c.link.as_deref() != Some(face.as_str())));
    assert!(sk.points.iter().all(|p| p.link.as_deref() != Some(face.as_str())));
    assert!(sk.constraints.len() < before, "the constraint on it went");
    assert!(sk.curve_index(&line).is_some(), "other geometry stays");
    assert!(sk.link(&edge).is_some(), "other links stay");
    // Break Link keeps the geometry as normal curves.
    let n = s.doc.sketch(sketch).unwrap().link_curves(&edge).len();
    run(&mut s, "sketch.break_link", json!({"link":edge}));
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.link(&edge).is_none());
    assert_eq!(sk.curves.iter().filter(|c| c.link.is_none()).count(), 1 + n);
    // Updating references brings neither back.
    run(&mut s, "sketch.update_references", json!({}));
    assert!(s.doc.sketch(sketch).unwrap().links.is_empty());
}

#[test]
fn hiding_a_link_changes_only_what_is_shown() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    let n = profiles(&s, sketch);
    let r = run(&mut s, "sketch.link_visibility", json!({"link":face}));
    assert_eq!(r["visible"], false);
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    assert!(sk.link_curves(&face).iter().all(|c| !crate::view::sketch_curve_visible(sk, *c)));
    let points = crate::view::SketchPointVisibility::new(sk);
    assert!(sk.link_points(&face).iter().all(|p| !points.takes(&sk.points[*p])));
    assert_eq!(profiles(&s, sketch), n, "profiles are untouched");
    let refs = run(&mut s, "sketch.references", json!({}));
    assert_eq!(refs["references"][0]["hidden"], true);
    // It still follows its source while hidden.
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,2,0]}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 22.0)).len() < 1e-6);
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.link_visibility", json!({"link":face,"visible":true}));
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    assert!(sk.link_curves(&face).iter().all(|c| crate::view::sketch_curve_visible(sk, *c)));
}

#[test]
fn freeze_holds_the_geometry_and_resume_follows_again() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    run(&mut s, "sketch.freeze_link", json!({"link":face}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,9,0]}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6, "frozen");
    let refs = run(&mut s, "sketch.references", json!({"sketch":sketch}));
    assert_eq!(refs["references"][0]["status"], "frozen");
    let saved = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert!(saved.sketch(sketch).unwrap().link(&face).unwrap().frozen);
    run(&mut s, "sketch.resume_link", json!({"sketch":sketch,"link":face}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 29.0)).len() < 1e-6, "follows again");
    run(&mut s, "edit.undo", json!({}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6, "undo freezes it again");
}

/// A frozen projection reads nothing, so moving the copy it was read from leaves its sketch's
/// result as it was (taken from the cache), where a live one is evaluated again.
#[test]
fn moving_the_source_of_a_frozen_projection_does_not_evaluate_its_sketch_again() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    run(&mut s, "sketch.finish", json!({}));
    let result = |s: &Session| s.model.result(sketch).map(|r| r.state.clone()).unwrap();
    let live = result(&s);
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,9,0]}));
    assert!(!std::sync::Arc::ptr_eq(&live, &result(&s)), "premise: a live projection is evaluated again");
    run(&mut s, "sketch.freeze_link", json!({"sketch":sketch,"link":face}));
    let frozen = result(&s);
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,9,0]}));
    assert!(std::sync::Arc::ptr_eq(&frozen, &result(&s)), "the frozen projection's sketch is not evaluated again");
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 29.0)).len() < 1e-6, "where it was frozen");
}

#[test]
fn a_frozen_projection_keeps_its_size_when_the_source_is_resized_and_update_says_frozen() {
    let (mut s, _, sketch, face) = parametric_plate(false);
    run(&mut s, "sketch.freeze_link", json!({"link":face}));
    // Hiding and showing it again leaves it frozen.
    run(&mut s, "sketch.link_visibility", json!({"link":face,"visible":false}));
    run(&mut s, "sketch.link_visibility", json!({"link":face,"visible":true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name":"w","expression":"60 mm"}));
    // A:2's top runs from x = 100 to 100 + w: frozen, it still ends at 140, not 160.
    assert!(on_x(&link_lines(&s, sketch, &face, false), 140.0).is_some(), "frozen at w = 40");
    assert!(on_x(&link_lines(&s, sketch, &face, false), 160.0).is_none());
    let r = run(&mut s, "sketch.update_references", json!({"sketch":sketch}));
    assert_eq!(r["links"][0]["status"], "frozen", "{r}");
    assert_eq!(run(&mut s, "sketch.references", json!({"sketch":sketch}))["references"][0]["status"], "frozen");
    assert!(on_x(&link_lines(&s, sketch, &face, true), 140.0).is_some(), "update leaves it where it was");
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.resume_link", json!({"link":face}));
    assert!(on_x(&link_lines(&s, sketch, &face, false), 160.0).is_some(), "resumed, it follows w = 60");
}

#[test]
fn pending_moves_are_reported_and_captured_by_update() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,4,0],"capture":false}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6, "a pending move is not followed");
    let refs = run(&mut s, "sketch.references", json!({}));
    assert_eq!(refs["references"][0]["status"], "pending");
    assert_eq!(refs["references"][0]["occurrence"]["name"], "A:2");
    let r = run(&mut s, "sketch.update_references", json!({"capture":true}));
    assert_eq!(r["captured"], json!([a2]));
    assert_eq!(r["ok"], true, "{r}");
    assert!(s.pending_moves.is_empty());
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 24.0)).len() < 1e-6);
    // The stored sketch has it too: current, not stale.
    let refs = run(&mut s, "sketch.references", json!({}));
    assert_eq!(refs["references"][0]["status"], "current");
    run(&mut s, "edit.undo", json!({}));
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6);
}

/// An exploded view places the copy only while it is shown: the projection reports no pending
/// move, and Update References with capture writes nothing into the design.
#[test]
fn an_exploded_view_is_not_a_pending_move_and_is_never_captured() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.ground", json!({"occurrence":a2,"grounded":false}));
    let saved = s.doc.occurrences.iter().find(|o| o.id == a2).unwrap().transform;
    let view = run(&mut s, "explode.create", json!({"moves":[{"occurrence":a2,"translate":[0,50,0]}]}))["view"].as_u64().unwrap();
    run(&mut s, "explode.show", json!({"view":view}));
    assert!(!s.pending_moves.is_empty(), "premise: the exploded view places A:2");
    assert_eq!(run(&mut s, "sketch.references", json!({}))["references"][0]["status"], "current");
    let r = run(&mut s, "sketch.update_references", json!({"capture":true}));
    assert_eq!(r["captured"], json!([]), "{r}");
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == a2).unwrap().transform, saved, "the design keeps A:2's placement");
    assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 20.0)).len() < 1e-6);
}

/// A sketch on a face projects that face's border on its own (an auto-projected link, which
/// resolves through the attachment). Repaired onto another body's face, it is an ordinary link
/// of that face: it no longer looks for the attachment, so it is found again on evaluation.
#[test]
fn a_repaired_auto_projected_border_follows_its_new_source() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":10,"base":[60,0,0],"body_name":"Post"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":[20,20,10]}}))["sketch"].as_u64().unwrap();
    let auto = s.doc.sketch(sketch).unwrap().links.iter().find(|l| l.auto_face).map(|l| l.id.clone()).expect("premise: the border is auto-projected");
    let r = run(&mut s, "sketch.repair_link", json!({"link":auto,"ref":{"face":[60,0,10],"body":"Post"}}));
    assert!(r["rebuilt"].as_bool().is_some(), "{r}");
    run(&mut s, "sketch.finish", json!({}));
    let stored = s.doc.sketch(sketch).unwrap().link(&auto).cloned().unwrap();
    assert!(!stored.auto_face && !stored.lost, "{stored:?}");
    let state = s.model.state();
    let shown = state.sketch(sketch).unwrap().sketch.link(&auto).cloned().unwrap();
    assert!(!shown.lost, "found again on its new source: {shown:?}");
    let (lines, rounds) = shape(&s, sketch, &auto);
    assert!(lines == 0 && rounds > 0, "the post's circle, not the plate's square: {lines} lines, {rounds} rounds");
}

#[test]
fn stale_then_lost_keeps_the_updated_geometry_and_repair_reconnects() {
    for another_copy in [false, true] {
        let (mut s, a1, a2, sketch) = holed_parts();
        let face = top_face_of_second(&mut s, a2);
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,6,0]}));
        assert_eq!(run(&mut s, "sketch.references", json!({}))["references"][0]["status"], "stale");
        let r = run(&mut s, "sketch.update_references", json!({}));
        assert_eq!(r["links"][0]["status"], "changed", "{r}");
        run(&mut s, "selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]}));
        let r = run(&mut s, "sketch.update_references", json!({}));
        assert_eq!(r["ok"], false);
        assert!((hole_centre(&s, sketch, &face) - Vec2::new(120.0, 26.0)).len() < 1e-6, "last good geometry");
        let ids: Vec<String> =
            s.doc.sketch(sketch).unwrap().link_curves(&face).iter().map(|c| s.doc.sketch(sketch).unwrap().curves[*c].id.clone()).collect();
        // Repair onto either the only remaining placement or one of several placements.
        if another_copy {
            let a3 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
            run(&mut s, "occurrence.move", json!({"occurrence":a3,"translate":[0,100,0]}));
        }
        // Repaired onto A:1's top face: same shape, same ids.
        run(&mut s, "sketch.edit", json!({"sketch":sketch}));
        let r = run(&mut s, "sketch.repair_link", json!({"link":face,"ref":{"face":[5,5,10],"body":"Plate","path":[a1]}}));
        assert_eq!(r["rebuilt"], false, "{r}");
        let sk = s.doc.sketch(sketch).unwrap();
        assert!(!sk.link(&face).unwrap().lost);
        if another_copy {
            assert_eq!(sk.link(&face).unwrap().placement.as_ref().unwrap().source, vec![a1]);
        } else {
            assert!(sk.link(&face).unwrap().placement.is_none(), "a single placement keeps ordinary pending-move following after repair");
        }
        let now: Vec<String> = sk.link_curves(&face).iter().map(|c| sk.curves[*c].id.clone()).collect();
        assert_eq!(now, ids);
        assert!((hole_centre(&s, sketch, &face) - Vec2::new(20.0, 20.0)).len() < 1e-6);
    }
}

#[test]
fn update_with_files_reports_an_unavailable_design_and_keeps_geometry() {
    let dir = std::env::temp_dir().join(format!("solidcraft-placed-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("bar.solidcraft");
    let save = |length: f64| {
        let mut p = Session::default();
        run(&mut p, "solid.box", json!({"length":length,"width":5,"height":5,"body_name":"Bar"}));
        run(&mut p, "file.save", json!({"path":part}));
    };
    save(10.0);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path":part}));
    let first = inserted["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[0,50,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    let body = inserted["bodies"][0].as_str().unwrap().to_string();
    let link = run(&mut s, "sketch.project", json!({"refs":[{"edge":[5,50,5],"body":body,"path":[second]}]}))["links"][0]["link"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "sketch.finish", json!({}));
    let length = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        let CurveKind::Line { a, b } = sk.curves[sk.link_curves(&link)[0]].kind else { panic!() };
        sk.point(a).unwrap().dist(sk.point(b).unwrap())
    };
    assert!((length(&s) - 10.0).abs() < 1e-6);
    save(16.0);
    let r = run(&mut s, "sketch.update_references", json!({"files":true}));
    assert_eq!(r["ok"], true, "{r}");
    assert!((length(&s) - 16.0).abs() < 1e-6, "the edited design's edge, at the second placement");
    // A host that refuses linked-design updates refuses them here too: nothing is read or changed.
    save(22.0);
    s.set_command_check(Some(std::sync::Arc::new(|id, _, _| if id == "component.update_links" { Err("no file updates".into()) } else { Ok(()) })));
    let before = s.doc.clone();
    let e = s.execute("sketch.update_references", &json!({"files":true})).unwrap_err().to_string();
    assert!(e.contains("no file updates"), "{e}");
    assert!(*s.doc == *before, "the design is unchanged");
    assert!((length(&s) - 16.0).abs() < 1e-6);
    s.set_command_check(None);
    std::fs::remove_file(&part).unwrap();
    let r = run(&mut s, "sketch.update_references", json!({"files":true}));
    assert_eq!(r["ok"], false);
    assert_eq!(r["files"]["unavailable"].as_array().unwrap().len(), 1, "{r}");
    assert!((length(&s) - 16.0).abs() < 1e-6, "the last bodies stay usable");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn management_commands_refuse_bad_input_without_changes() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    let before = s.doc.to_json();
    for (cmd, p) in [
        ("sketch.remove_link", json!({"link":"nope"})),
        ("sketch.freeze_link", json!({})),
        ("sketch.resume_link", json!({"entities":["nope"]})),
        ("sketch.link_visibility", json!({"links":[face, 7]})),
        ("sketch.repair_link", json!({"link":face})),
        ("sketch.repair_link", json!({"link":face,"ref":{"edge":[1e300,0,0],"body":"Plate","path":[a2, a2]}})),
        ("sketch.references", json!({"sketch":"no such"})),
        ("sketch.update_references", json!({"sketch":sketch + 1000})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        assert_eq!(s.doc.to_json(), before, "{cmd}");
    }
}

#[test]
fn a_turned_source_keeps_its_face_projection_and_ids() {
    let (mut s, _, a2, sketch) = holed_parts();
    let face = top_face_of_second(&mut s, a2);
    let ids = |s: &Session, stored: bool| -> Vec<String> {
        let st = s.model.state();
        let sk = if stored { s.doc.sketch(sketch).unwrap().clone() } else { st.sketch(sketch).unwrap().sketch.clone() };
        sk.link_curves(&face).iter().map(|c| sk.curves[*c].id.clone()).collect()
    };
    let before = ids(&s, true);
    run(&mut s, "sketch.finish", json!({}));
    // A:2 turned 30 degrees about its corner at (100, 0): the hole centre (120, 20) turns with it.
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"space":"world","axis":[0,0,1],"angle":"30 deg","origin":[100,0,0]}));
    let (c, n) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let turned = Vec2::new(100.0 + 20.0 * c - 20.0 * n, 20.0 * n + 20.0 * c);
    assert!(!s.model.state().sketch(sketch).unwrap().sketch.link(&face).unwrap().lost);
    assert!((hole_centre(&s, sketch, &face) - turned).len() < 1e-6, "{:?}", hole_centre(&s, sketch, &face));
    assert_eq!(ids(&s, false), before, "the same curves, turned");
    let r = run(&mut s, "sketch.update_references", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(ids(&s, true), before);
    // Turned again, saved and reopened: still the same curves.
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"space":"world","axis":[0,0,1],"angle":"95 deg","origin":[100,0,0]}));
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    assert!(!reopened.model.state().sketch(sketch).unwrap().sketch.link(&face).unwrap().lost);
    assert_eq!(ids(&reopened, false), before);
}

#[test]
fn a_tilted_source_changes_shape_visibly_and_never_panics() {
    let (mut s, _, a2, sketch) = holed_parts();
    let edge = run(&mut s, "sketch.project", json!({"refs":[{"edge":[110,0,10],"body":"Plate","path":[a2]}]}))["links"][0]["link"]
        .as_str()
        .unwrap()
        .to_string();
    let face = top_face_of_second(&mut s, a2);
    run(&mut s, "sketch.finish", json!({}));
    for angle in ["20 deg", "70 deg", "90 deg"] {
        run(&mut s, "occurrence.move", json!({"occurrence":a2,"space":"world","axis":[1,0,0],"angle":angle,"origin":[100,0,0]}));
        let r = run(&mut s, "sketch.update_references", json!({}));
        let refs = run(&mut s, "sketch.references", json!({}));
        for entry in refs["references"].as_array().unwrap() {
            assert!(["current", "lost"].contains(&entry["status"].as_str().unwrap()), "{angle}: {entry} {r}");
        }
    }
    // The edge stays a line (its projection shortens or collapses to a point, which is lost).
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    assert!(sk.link(&edge).is_some() && sk.link(&face).is_some());
}

/// Part A: a `w` × 40 × 10 plate (`w` a parameter, 40 mm), optionally with a Ø10 hole at
/// (20, 20), placed twice (A:2 at x = 100); part B's sketch projects A:2's top face.
/// Returns (session, A:2, sketch, face link).
fn parametric_plate(hole: bool) -> (Session, u64, u64, String) {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"w","expression":"40 mm"}));
    let a1 = run(&mut s, "component.create", json!({"name":"A"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":"w","width":40,"height":10,"body_name":"Plate"}));
    if hole {
        run(&mut s, "solid.cylinder", json!({"radius":5,"height":10,"base":[20,20,0],"operation":"cut","targets":["Plate"]}));
    }
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[100,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Receiver"}))["sketch"].as_u64().unwrap();
    let face = top_face_of_second(&mut s, a2);
    (s, a2, sketch, face)
}

/// The link's lines in the evaluated (or stored) sketch: id and end points.
fn link_lines(s: &Session, sketch: u64, link: &str, stored: bool) -> Vec<(String, Vec2, Vec2)> {
    let st = s.model.state();
    let sk = if stored { s.doc.sketch(sketch).unwrap().clone() } else { st.sketch(sketch).unwrap().sketch.clone() };
    sk.link_curves(link)
        .into_iter()
        .filter_map(|c| match sk.curves[c].kind {
            CurveKind::Line { a, b } => Some((sk.curves[c].id.clone(), sk.point(a)?, sk.point(b)?)),
            _ => None,
        })
        .collect()
}

/// A line along x = `x` (both ends there).
fn on_x(lines: &[(String, Vec2, Vec2)], x: f64) -> Option<String> {
    lines.iter().find(|(_, a, b)| (a.x - x).abs() < 1e-6 && (b.x - x).abs() < 1e-6).map(|(id, ..)| id.clone())
}

#[test]
fn a_deleted_edge_stays_that_edge_when_the_source_is_resized() {
    let (mut s, _, sketch, face) = parametric_plate(false);
    let lines = link_lines(&s, sketch, &face, true);
    assert_eq!(lines.len(), 4);
    let right = on_x(&lines, 140.0).expect("the right edge at x = 140");
    let left = on_x(&lines, 100.0).expect("the left edge at x = 100");
    // A line of the sketch's own, held parallel to the left edge: its constraint must survive.
    let own = run(&mut s, "sketch.line", json!({"points":[[90,0],[90,30]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.parallel", json!({"a":own,"b":left}));
    run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":[right]}));
    let constraints = s.doc.sketch(sketch).unwrap().constraints.len();
    run(&mut s, "sketch.finish", json!({}));
    // Ten times as wide: the right edge moves to x = 500, the left stays at x = 100.
    run(&mut s, "parameters.change", json!({"name":"w","expression":"400 mm"}));
    run(&mut s, "sketch.update_references", json!({}));
    let check = |s: &Session, stored: bool| {
        let lines = link_lines(s, sketch, &face, stored);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(on_x(&lines, 500.0).is_none(), "the deleted right edge stays deleted: {lines:?}");
        assert_eq!(on_x(&lines, 100.0), Some(left.clone()), "the left edge stays, with its id: {lines:?}");
        assert!(lines.iter().any(|(_, a, b)| (a.y - b.y).abs() < 1e-6 && (a.x - b.x).abs() > 399.0), "top and bottom span the new width");
        assert!(!s.model.state().sketch(sketch).unwrap().sketch.link(&face).unwrap().lost);
        assert_eq!(s.doc.sketch(sketch).unwrap().constraints.len(), constraints, "the constraint on the left edge survives");
    };
    check(&s, false);
    check(&s, true);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    check(&reopened, false);
    // Back to 40 mm and wider again: still the same edge left out.
    run(&mut s, "parameters.change", json!({"name":"w","expression":"25 mm"}));
    let lines = link_lines(&s, sketch, &face, false);
    assert!(on_x(&lines, 125.0).is_none() && on_x(&lines, 100.0).is_some(), "{lines:?}");
}

/// The link's round curves (the hole's boundary) in the evaluated sketch: centres.
fn rounds(s: &Session, sketch: u64, link: &str) -> Vec<(String, Vec2)> {
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    sk.link_curves(link)
        .into_iter()
        .filter_map(|c| match sk.curves[c].kind {
            CurveKind::Circle { c: centre, .. } | CurveKind::Arc { c: centre, .. } => Some((sk.curves[c].id.clone(), sk.point(centre)?)),
            _ => None,
        })
        .collect()
}

/// [`parametric_plate`] with its hole, as a design saved by a build that projected a closed
/// circular edge as its two halves (before #17175) and opened again: the stored link holds the
/// hole's rim as two arcs, which it keeps when the edge is read again as one circle (#18250).
/// The sketch is open. Returns (session, sketch, face link).
fn plate_saved_with_a_halved_hole() -> (Session, u64, String) {
    let (s, _, sketch, face) = parametric_plate(true);
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(sketch).unwrap();
    let halved: Vec<LinkGeom> = sk
        .link_curves(&face)
        .into_iter()
        .filter_map(|c| sk.curve_link_geom(c))
        .flat_map(|g| match g {
            LinkGeom::Circle(c, r) => {
                let (east, west) = (Vec2::new(c.x + r, c.y), Vec2::new(c.x - r, c.y));
                vec![LinkGeom::Arc { c, a: east, b: west }, LinkGeom::Arc { c, a: west, b: east }]
            }
            g => vec![g],
        })
        .collect();
    assert_eq!(halved.iter().filter(|g| matches!(g, LinkGeom::Arc { .. })).count(), 2, "premise: the fresh hole was one circle");
    assert!(sk.update_link(&face, &halved).unwrap(), "premise: the stored rim is rebuilt as two halves");
    let mut s = Session { doc: solidcraft_doc::format::read(&doc.to_json()).unwrap().into(), ..Default::default() };
    s.refresh();
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    (s, sketch, face)
}

/// Deletes all of `hole` (the link's round curves): kept out by its wall's family, however far
/// the plate grows, in the session and in the design saved and opened again.
fn the_whole_hole_stays_deleted_through_a_resize(mut s: Session, sketch: u64, face: &str, hole: Vec<String>) {
    let outer: Vec<String> = link_lines(&s, sketch, face, true).into_iter().map(|(id, ..)| id).collect();
    run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":hole}));
    assert_eq!(s.doc.sketch(sketch).unwrap().link(face).unwrap().omit_edges.len(), 1, "one family");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name":"w","expression":"400 mm"}));
    let r = run(&mut s, "sketch.update_references", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    let check = |s: &Session| {
        assert!(rounds(s, sketch, face).is_empty(), "the hole stays deleted");
        let mut ids: Vec<String> = link_lines(s, sketch, face, false).into_iter().map(|(id, ..)| id).collect();
        ids.sort();
        let mut want = outer.clone();
        want.sort();
        assert_eq!(ids, want, "the four outer edges stay, with their ids");
        assert!(on_x(&link_lines(s, sketch, face, false), 500.0).is_some(), "the right edge moved with the width");
    };
    check(&s);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    check(&reopened);
}

/// A design saved with the hole in two halves (#18250). One half: no source edge gives just
/// that half (a one-edge wall gives both, a split wall's pieces are told apart only by number),
/// so the rest is broken off. The whole hole stays deleted through a resize.
#[test]
fn half_a_hole_breaks_the_rest_off_and_the_whole_hole_stays_deleted_through_a_resize() {
    let (mut s, sketch, face) = plate_saved_with_a_halved_hole();
    let hole: Vec<String> = rounds(&s, sketch, &face).into_iter().map(|(id, _)| id).collect();
    assert_eq!(hole.len(), 2, "the hole comes as two halves");
    let half = &hole[0];
    let r = run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":[half]}));
    let why = broken_because(&r, &face);
    assert!(why.contains("more curves than the ones deleted") || why.contains("told apart only by number"), "{r}");
    assert!(s.doc.sketch(sketch).unwrap().link(&face).is_none());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(rounds(&s, sketch, &face).len(), 2, "undo brings the link and both halves back");
    the_whole_hole_stays_deleted_through_a_resize(s, sketch, &face, hole);
}

/// A fresh projection gives the hole as one circle (#17175): deleting it leaves the whole hole
/// out, through a resize and a reopen.
#[test]
fn a_freshly_projected_hole_is_one_circle_and_stays_deleted_through_a_resize() {
    let (s, _, sketch, face) = parametric_plate(true);
    let hole: Vec<String> = rounds(&s, sketch, &face).into_iter().map(|(id, _)| id).collect();
    assert_eq!(hole.len(), 1, "the hole comes as one circle");
    {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        assert!(matches!(sk.curves[sk.curve_index(&hole[0]).unwrap()].kind, CurveKind::Circle { .. }), "a circle, not an arc");
    }
    the_whole_hole_stays_deleted_through_a_resize(s, sketch, &face, hole);
}

/// Deleted arcs are all of a projected circle only when their spans close the turn (#18250): a
/// 350° arc whose 10° gap falls between points sampled every 45° is not, nor are halves 1°
/// apart or an arc about another centre; arcs that meet across 0° are. A circle with nothing
/// deleted on it is not covered, nor is one no larger than the tolerance by any arcs.
#[test]
fn only_arcs_on_the_circle_that_close_the_turn_are_all_of_it() {
    let mut sk = Sketch::new();
    let at = |centre: Vec2, deg: f64| centre + Vec2::new(deg.to_radians().cos(), deg.to_radians().sin()) * 5.0;
    let mut arc = |centre: Vec2, from: f64, to: f64| sk.add_arc(centre, at(centre, from), at(centre, to), [None; 3], None).unwrap();
    let near_full = arc(Vec2::ZERO, 27.5, 17.5);
    let across = arc(Vec2::ZERO, 300.0, 60.0);
    let rest = arc(Vec2::ZERO, 60.0, 300.0);
    let upper = arc(Vec2::ZERO, 0.0, 180.0);
    let lower_short = arc(Vec2::ZERO, 181.0, 360.0);
    let elsewhere = arc(Vec2::new(0.5, 0.0), 180.0, 0.0);
    let parts = |deleted: &[usize]| super::circle_parts(&sk, deleted, Vec2::ZERO, 5.0, 1e-7);
    assert_eq!(parts(&[near_full]), (vec![near_full], false), "a 350° arc leaves its 10° gap");
    assert_eq!(parts(&[across, rest]), (vec![across, rest], true), "complementary arcs across 0° close the turn");
    assert_eq!(parts(&[across]), (vec![across], false));
    assert_eq!(parts(&[upper, lower_short]), (vec![upper, lower_short], false), "halves 1° apart");
    assert_eq!(parts(&[upper, elsewhere]), (vec![upper], false), "an arc about another centre is not on the circle");
    // With nothing deleted on it, a circle is not covered, however small.
    assert_eq!(parts(&[]), (vec![], false));
    let tiny = |deleted: &[usize]| super::circle_parts(&sk, deleted, Vec2::ZERO, 1e-9, 1e-7);
    assert_eq!(tiny(&[]), (vec![], false), "a circle within the tolerance, nothing deleted on it");
    assert_eq!(tiny(&[elsewhere]), (vec![], false), "a circle within the tolerance, an unrelated arc deleted");
    // Spans with a slack of a whole turn (a rim no longer than the tolerance) tell nothing.
    let (half, turn) = (std::f64::consts::PI, std::f64::consts::TAU);
    assert!(!super::covers_turn(&[], 1e-9), "no spans");
    assert!(!super::covers_turn(&[(0.0, half), (half, half)], turn), "a slack of a whole turn");
    assert!(super::covers_turn(&[(0.0, half), (half, half)], 1e-9), "the two halves, with a sound slack");
}

#[test]
fn a_deleted_member_whose_source_edge_is_gone_makes_the_projection_lost_not_shorter() {
    let (mut s, _, sketch, face) = parametric_plate(true);
    let hole: Vec<String> = rounds(&s, sketch, &face).into_iter().map(|(id, _)| id).collect();
    run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":hole}));
    run(&mut s, "sketch.finish", json!({}));
    let kept = link_lines(&s, sketch, &face, false);
    // The hole is suppressed: its wall's edges, the deleted members, no longer exist.
    let cylinder = s.doc.features.iter().find(|f| f.name.starts_with("Cylinder")).unwrap().id;
    run(&mut s, "timeline.suppress", json!({"feature":cylinder,"suppressed":true}));
    let st = s.model.state();
    assert!(st.sketch(sketch).unwrap().sketch.link(&face).unwrap().lost, "lost, not quietly re-projected");
    assert_eq!(link_lines(&s, sketch, &face, false), kept, "the last geometry stays");
    let refs = run(&mut s, "sketch.references", json!({"sketch":sketch}));
    assert_eq!(refs["references"][0]["status"], "lost");
    assert!(refs["references"][0]["reason"].as_str().unwrap().contains("deleted projected edge"), "{refs}");
    // Back again: the projection is whole minus its hole once more.
    run(&mut s, "timeline.suppress", json!({"feature":cylinder,"suppressed":false}));
    assert!(!s.model.state().sketch(sketch).unwrap().sketch.link(&face).unwrap().lost);
    assert!(rounds(&s, sketch, &face).is_empty());
}

#[test]
fn part_of_a_projection_without_source_edges_breaks_off_the_rest() {
    let (mut s, a2, sketch, face) = parametric_plate(false);
    // Seen from above, the plate's silhouette is its top face, already shown: that goes first.
    run(&mut s, "sketch.remove_link", json!({"link":face}));
    // A body silhouette: no source edge per curve, so the curves left are broken off as they are.
    let r = run(&mut s, "sketch.project", json!({"refs":[{"body":"Plate","path":[a2]}]}));
    let link = r["links"][0]["link"].as_str().unwrap().to_string();
    let silhouette = r["links"][0]["curves"].as_array().unwrap().clone();
    assert!(silhouette.len() > 1);
    let r = run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":[silhouette[0]]}));
    assert!(broken_because(&r, &link).contains("only a projected face"), "{r}");
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.links.is_empty());
    assert_eq!(sk.curves.iter().filter(|c| c.link.is_none()).count(), silhouette.len() - 1, "the rest stays, unlinked");
    // Removing all its curves removes it, with nothing broken off.
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":silhouette}));
    assert!(r.get("broken_links").is_none(), "{r}");
    assert!(s.doc.sketch(sketch).unwrap().links.is_empty());
}

/// Part A: a 40 × 40 × 10 plate with a D-shaped pocket 1 mm into its top face (a line and a
/// cubic control-point spline), placed twice (A:2 at x = 100); part B's sketch projects A:2's
/// top face. The spline's edge comes in as its exact Bézier piece (#1508), and a deleted
/// member is matched against that same piece: it is found to be the spline's source edge. The
/// pocket's walls are named from the cut's exact history, so the deleted member is recorded and
/// the rest of the projection stays linked.
#[test]
fn a_deleted_exact_spline_member_is_matched_to_its_source_edge() {
    let mut s = Session::default();
    let a1 = run(&mut s, "component.create", json!({"name":"A"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Plate"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","offset":10}));
    run(&mut s, "sketch.line", json!({"points":[[5,5],[25,5]]}));
    run(&mut s, "sketch.spline.control_point", json!({"points":[[25,5],[25,15],[5,15],[5,5]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":-1,"operation":"cut"}));
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[100,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let b = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Receiver"}))["sketch"].as_u64().unwrap();
    let face = run(&mut s, "sketch.project", json!({"refs":[{"face":[138,38,10],"body":"Plate","path":[a2]}]}))["links"][0]["link"]
        .as_str()
        .unwrap()
        .to_string();
    let st = s.model.state();
    let sk = &st.sketch(b).unwrap().sketch;
    let exact: Vec<String> = sk
        .link_curves(&face)
        .into_iter()
        .filter(|c| matches!(sk.curves[*c].kind, CurveKind::Spline { control: true, .. }))
        .map(|c| sk.curves[c].id.clone())
        .collect();
    assert_eq!(exact.len(), 1, "the spline comes in as its exact piece");
    let r = run(&mut s, "sketch.delete", json!({"preserve_projection_link":true,"entities":exact}));
    assert!(r.get("broken_links").is_none(), "matched to its source edge: {r}");
    let link = s.doc.sketch(b).unwrap().link(&face).cloned().expect("still linked");
    assert!(!link.lost);
    assert_eq!(link.omit_edges.len(), 1, "the spline's wall is kept out by name");
    assert!(s.doc.sketch(b).unwrap().link_curves(&face).iter().all(|c| !exact.contains(&s.doc.sketch(b).unwrap().curves[*c].id)));
}

/// A solid's exact pieces (#1508) are matched on the curves they gave the sketch: a control-point
/// spline's and a conic's control points lie off the curve, so their points on the curve are
/// probed; a piece that is not on the curve is not matched.
#[test]
fn exact_spline_and_conic_pieces_lie_on_their_curves() {
    use solidcraft_sketch::LinkGeom;
    let mut s = Session::default();
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.spline.control_point", json!({"points":[[0,0],[0,12],[20,12],[20,0]],"connect":false}));
    run(&mut s, "sketch.conic", json!({"start":[30,0],"end":[40,0],"apex":[35,6],"rho":0.7}));
    let sk = s.doc.sketch(sketch).unwrap().clone();
    let spline = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Spline { .. })).unwrap();
    let conic = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Conic { .. })).unwrap();
    let pts = vec![Vec2::new(0., 0.), Vec2::new(0., 12.), Vec2::new(20., 12.), Vec2::new(20., 0.)];
    let piece = LinkGeom::Spline { pts: pts.clone(), control: true, degree: 3, periodic: false, nurbs: None };
    assert!(super::piece_on(&sk, spline, &piece, 1e-7));
    let lifted = LinkGeom::Spline { pts: pts.iter().map(|p| *p + Vec2::Y).collect(), control: true, degree: 3, periodic: false, nurbs: None };
    assert!(!super::piece_on(&sk, spline, &lifted, 1e-7));
    let piece = LinkGeom::Conic { a: Vec2::new(30., 0.), apex: Vec2::new(35., 6.), b: Vec2::new(40., 0.), rho: 0.7 };
    assert!(super::piece_on(&sk, conic, &piece, 1e-7));
    let flatter = LinkGeom::Conic { a: Vec2::new(30., 0.), apex: Vec2::new(35., 6.), b: Vec2::new(40., 0.), rho: 0.3 };
    assert!(!super::piece_on(&sk, conic, &flatter, 1e-7));
}

/// A face of the 40 mm plate has four straight edges and the circular hole. Removing one
/// straight edge leaves three and the hole. Only explicit preservation keeps those linked.
#[test]
fn partial_projection_delete_defaults_to_break_and_explicit_preservation_survives_reopen() {
    for batch in [false, true] {
        for policy in [None, Some(false), Some(true)] {
            let (mut s, _, a2, sketch) = holed_parts();
            let face = top_face_of_second(&mut s, a2);
            let sk = s.doc.sketch(sketch).unwrap();
            let curve = sk.link_curves(&face).into_iter().find(|c| matches!(sk.curves[*c].kind, CurveKind::Line { .. })).unwrap();
            let victim = sk.curves[curve].id.clone();
            let remaining: Vec<String> = sk.link_curves(&face).into_iter().filter(|c| *c != curve).map(|c| sk.curves[c].id.clone()).collect();
            let circle = remaining.iter().find(|id| matches!(sk.curves[sk.curve_index(id).unwrap()].kind, CurveKind::Circle { .. })).unwrap().clone();
            let mut params = if batch { json!({"items":[{"type":"sketch_curve","id":victim}]}) } else { json!({"entities":[victim]}) };
            if let Some(policy) = policy {
                params["preserve_projection_link"] = json!(policy);
            }
            let command = if batch { "selection.delete" } else { "sketch.delete" };
            if batch {
                let before = s.doc.to_json();
                let mut dry = params.clone();
                dry["dry_run"] = json!(true);
                run(&mut s, command, dry);
                assert_eq!(s.doc.to_json(), before, "dry run keeps geometry and the link");
            }
            run(&mut s, command, params);
            let preserve = policy == Some(true);
            let assert_link = |s: &Session, moved: bool| {
                let st = s.model.state();
                let sk = &st.sketch(sketch).unwrap().sketch;
                assert!(sk.curve_index(&victim).is_none());
                assert_eq!(sk.link(&face).is_some(), preserve, "{command}, {policy:?}");
                for id in &remaining {
                    let c = &sk.curves[sk.curve_index(id).unwrap()];
                    assert_eq!(c.link.as_deref(), preserve.then_some(face.as_str()));
                }
                let CurveKind::Circle { c, .. } = sk.curves[sk.curve_index(&circle).unwrap()].kind else { panic!("hole") };
                let expected_y = if preserve && moved { 23.0 } else { 20.0 };
                assert!((sk.point(c).unwrap() - Vec2::new(120.0, expected_y)).len() < 1e-6);
                if preserve {
                    assert_eq!(sk.link(&face).unwrap().omit_edges.len(), 1);
                    assert!(!sk.link(&face).unwrap().lost);
                }
            };
            assert_link(&s, false);
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,3,0]}));
            run(&mut s, "sketch.update_references", json!({}));
            assert_link(&s, true);
            let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
            reopened.refresh();
            assert_link(&reopened, true);
        }
    }
}
