//! Issue #489: a sketch or an offset plane on the top face of an extrude that was itself
//! sketched on a face. Sketching a profile over the whole face draws curves on top of the face's
//! projected boundary; the extrude's faces must still carry an exact, certified planar source.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::FaceAttachmentStatus;
use solidcraft_geom::prelude::*;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn profile(s: &mut Session, circle: bool, full: bool) {
    if circle {
        run(s, "sketch.circle.center", json!({"center": [20, 20], "diameter": if full { 40 } else { 16 }}));
    } else if full {
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]}));
    } else {
        run(s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]}));
    }
}

/// A 40 mm base (square or round) 10 mm tall, then a second extrude 10 mm up from a sketch on
/// its top face (or on a plane at that height), joined or as a new body.
fn two_extrudes(circle: bool, full: bool, on_face: bool, operation: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    profile(&mut s, circle, true);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "name": "First"}));
    let plane = if on_face { json!({"face": [20, 20, 10]}) } else { json!("XY") };
    let mut params = json!({"plane": plane, "name": "Upper"});
    if !on_face {
        params["offset"] = json!(10);
    }
    run(&mut s, "sketch.create", params);
    profile(&mut s, circle, full);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "operation": operation, "name": "Second"}));
    s
}

fn healthy(s: &Session, sketch: &str) -> solidcore_record::Plane {
    let id = s.doc.find_feature(sketch).map(|f| f.id).unwrap();
    match s.placed_document().sketch_face_attachment(id, &s.model) {
        FaceAttachmentStatus::Healthy { plane, .. } => plane,
        other => panic!("{sketch}: {other:?}"),
    }
}

fn no_errors(s: &Session) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

#[test]
fn sketch_and_offset_plane_on_the_top_face_of_a_face_sketched_extrude() {
    for operation in ["join", "new"] {
        for circle in [false, true] {
            for full in [true, false] {
                for on_face in [true, false] {
                    let case = format!("{operation} circle={circle} full={full} on_face={on_face}");
                    let mut s = two_extrudes(circle, full, on_face, operation);
                    let top = [20., 20., 20.];
                    s.execute("sketch.create", &json!({"plane": {"face": top}, "name": "Top"})).unwrap_or_else(|e| panic!("{case}: {e}"));
                    run(&mut s, "sketch.finish", json!({}));
                    let plane = healthy(&s, "Top");
                    assert!((plane.origin.z - 20.).abs() < 1e-9 && (plane.normal().z - 1.).abs() < 1e-9, "{case}: {plane:?}");
                    s.execute("construct.plane.offset", &json!({"base": {"face": top}, "offset": 5, "name": "Above"}))
                        .unwrap_or_else(|e| panic!("{case}: {e}"));
                    // The second extrude's flat side attaches too, also where a full-face Join
                    // merged it with the first extrude's side (#491): that face is known by its
                    // exact history name.
                    if !circle {
                        let side = if full { [40., 20., 15.] } else { [30., 20., 15.] };
                        s.execute("sketch.create", &json!({"plane": {"face": side}, "name": "Side"})).unwrap_or_else(|e| panic!("{case} side: {e}"));
                        assert!((healthy(&s, "Side").normal().x - 1.).abs() < 1e-9, "{case}");
                        run(&mut s, "sketch.finish", json!({}));
                    }
                    s.refresh();
                    no_errors(&s);
                }
            }
        }
    }
}

/// Three extrudes, each sketched over the whole top face of the one before; editing the first
/// one's height moves every later sketch with its face.
#[test]
fn three_stacked_extrudes_follow_an_edit_of_the_first() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "name": "E1"}));
    for (k, top) in [(2, 10.), (3, 20.)] {
        run(&mut s, "sketch.create", json!({"plane": {"face": [20, 20, top]}, "name": format!("S{k}")}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 40]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 10, "operation": "join", "name": format!("E{k}")}));
    }
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 20, 30]}, "name": "S4"}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 20], "diameter": 10}));
    run(&mut s, "sketch.finish", json!({}));
    s.refresh();
    no_errors(&s);
    assert_eq!(s.model.state().bodies.len(), 1);
    for (name, z) in [("S2", 10.), ("S3", 20.), ("S4", 30.)] {
        assert!((healthy(&s, name).origin.z - z).abs() < 1e-9, "{name}");
    }

    run(&mut s, "timeline.edit", json!({"feature": "E1", "set": {"extent": {"distance": "25"}}}));
    s.refresh();
    no_errors(&s);
    for (name, z) in [("S2", 25.), ("S3", 35.), ("S4", 45.)] {
        assert!((healthy(&s, name).origin.z - z).abs() < 1e-9, "{name} after edit");
    }
    let volume: f64 = s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
    assert!((volume - 40. * 40. * 45.).abs() < 1e-6, "{volume}");

    // Saved and reopened, every attachment still resolves.
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    reopened.refresh();
    no_errors(&reopened);
    assert!((healthy(&reopened, "S4").origin.z - 45.).abs() < 1e-9);
}
