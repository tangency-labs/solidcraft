//! The Join/New Body test agrees with what the Join makes: one body, or a body in pieces.

use serde_json::{Value, json};

use super::{Addition, addition};
use crate::Session;

fn run(s: &mut Session, id: &str, params: Value) {
    if let Err(e) = s.execute(id, &params) {
        panic!("{id} {params}: {e}");
    }
}

/// Sketch `circles` (centre, radius) and `rects` (corners) on a plane at height `z`.
fn sketch(s: &mut Session, z: f64, name: &str, circles: &[([f64; 2], f64)], rects: &[([f64; 2], [f64; 2])]) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": name}));
    for (c, r) in circles {
        run(s, "sketch.circle.center", json!({"center": c, "radius": r}));
    }
    for (p0, p1) in rects {
        run(s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
    }
    run(s, "sketch.finish", json!({}));
}

fn extrude(sketch: &str, point: [f64; 2], distance: f64) -> Vec<(String, Value)> {
    vec![("solid.extrude".to_string(), json!({"sketch": sketch, "profiles": [{"point": point}], "distance": distance, "operation": "new"}))]
}

/// The pieces of every body after running `cmds` as a Join for real.
fn pieces_after_join(s: &Session, cmds: &[(String, Value)]) -> Vec<usize> {
    let mut t = s.scratch();
    for (id, p) in cmds {
        let mut p = p.clone();
        p["operation"] = json!("join");
        run(&mut t, id, p);
    }
    t.model.state().bodies.iter().map(|b| b.body.lumps().map_or(1, |l| l.len())).collect()
}

/// The verdict (does the material join?), checked against the Join's own result: one body in one
/// piece, or not.
fn verdict(label: &str, s: &Session, cmds: &[(String, Value)]) -> bool {
    let one = addition(s, cmds, false).unwrap_or_else(|| panic!("{label}: no verdict")) != Addition::Apart;
    let before: Vec<usize> = s.model.state().bodies.iter().map(|b| b.body.lumps().map_or(1, |l| l.len())).collect();
    let after = pieces_after_join(s, cmds);
    // Joined: no new body, and no body in more pieces than before (a bridge leaves fewer).
    let joined = after.len() == before.len() && after.iter().zip(&before).all(|(a, b)| a <= b);
    assert_eq!(one, joined, "{label}: verdict {one}, pieces {before:?} -> {after:?}");
    one
}

/// A 40 × 40 × 10 block centred on the origin with a 5 mm radius hole through it.
fn holed_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Base"}));
    sketch(&mut s, 0.0, "Hole", &[([0.0, 0.0], 5.0)], &[]);
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "profiles": [{"point": [0, 0]}], "distance": 10, "operation": "cut"}));
    s
}

fn cube() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    s
}

#[test]
fn material_on_a_body_away_from_its_centre_joins() {
    // A ring round a hole: its centre is over the hole, its material on the face.
    let mut s = holed_block();
    sketch(&mut s, 10.0, "Ring", &[([0.0, 0.0], 5.0), ([0.0, 0.0], 10.0)], &[]);
    assert!(verdict("ring round a hole", &s, &extrude("Ring", [7.5, 0.0], 5.0)));
    // A pin filling the bore: it meets the body only on the bore's round wall.
    sketch(&mut s, 0.0, "Pin", &[([0.0, 0.0], 5.0)], &[]);
    assert!(verdict("pin filling the bore", &s, &extrude("Pin", [0.0, 0.0], 10.0)));
    // A block against a side wall, all of it or half of it, and one standing past an edge.
    let mut s = cube();
    sketch(&mut s, 0.0, "Side", &[], &[([10.0, 0.0], [20.0, 10.0]), ([10.0, 15.0], [20.0, 25.0])]);
    assert!(verdict("block against a side wall", &s, &extrude("Side", [15.0, 5.0], 10.0)));
    sketch(&mut s, 0.0, "Half", &[], &[([10.0, 5.0], [20.0, 15.0])]);
    assert!(verdict("block half against a side wall", &s, &extrude("Half", [15.0, 10.0], 5.0)));
    sketch(&mut s, 10.0, "Ledge", &[], &[([8.0, 0.0], [30.0, 10.0])]);
    assert!(verdict("block past an edge", &s, &extrude("Ledge", [20.0, 5.0], 5.0)));
}

#[test]
fn material_apart_stays_a_new_body() {
    let mut s = holed_block();
    // A pin in the bore with room round it: inside the block's box, touching nothing.
    sketch(&mut s, 0.0, "Thin", &[([0.0, 0.0], 4.0)], &[]);
    assert!(!verdict("pin clear in the bore", &s, &extrude("Thin", [0.0, 0.0], 10.0)));
    let mut s = cube();
    sketch(&mut s, 0.0, "Far", &[], &[([30.0, 0.0], [40.0, 10.0])]);
    assert!(!verdict("block away from the cube", &s, &extrude("Far", [35.0, 5.0], 10.0)));
    sketch(&mut s, 0.0, "Gap", &[], &[([10.1, 0.0], [20.0, 10.0])]);
    assert!(!verdict("block 0.1 mm off a side wall", &s, &extrude("Gap", [15.0, 5.0], 10.0)));
}

#[test]
fn hairline_gaps_follow_the_join() {
    // Material closer than the contact test's tolerance: whatever the Join makes of it, the
    // verdict says the same (`verdict` checks).
    let mut s = cube();
    for (i, gap) in [0.001, 0.01].into_iter().enumerate() {
        let name = format!("Gap{i}");
        sketch(&mut s, 0.0, &name, &[], &[([10.0 + gap, 0.0], [20.0, 10.0])]);
        verdict(&format!("block {gap} mm off a side wall"), &s, &extrude(&name, [15.0, 5.0], 10.0));
    }
    let mut s = holed_block();
    sketch(&mut s, 0.0, "Close", &[([0.0, 0.0], 4.99)], &[]);
    verdict("pin 0.01 mm clear in the bore", &s, &extrude("Close", [0.0, 0.0], 10.0));
}

#[test]
fn a_bridge_between_pieces_joins_them() {
    // Two pieces of one body (a Join apart, kept with its warning) and a block across them.
    let mut s = cube();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "join"}));
    assert_eq!(s.model.state().bodies.len(), 1);
    sketch(&mut s, 10.0, "Bridge", &[], &[([5.0, 0.0], [25.0, 10.0])]);
    assert!(verdict("bridge", &s, &extrude("Bridge", [15.0, 5.0], 5.0)));
}

#[test]
fn material_inside_a_body_adds_nothing() {
    let s = cube();
    let box_at = |corner: [f64; 3], size: f64| {
        vec![("solid.box".to_string(), json!({"length": size, "width": size, "height": size, "corner": corner, "operation": "new"}))]
    };
    assert_eq!(addition(&s, &box_at([2.0, 2.0, 2.0], 4.0), true), Some(Addition::Inside), "a box inside the cube");
    assert_eq!(addition(&s, &box_at([2.0, 2.0, 6.0], 4.0), true), Some(Addition::Inside), "a box sunk flush with the top");
    assert_eq!(addition(&s, &box_at([2.0, 2.0, 8.0], 4.0), true), Some(Addition::Joins), "a box standing out of the top");
    assert_eq!(addition(&s, &box_at([2.0, 2.0, 10.0], 4.0), true), Some(Addition::Joins), "a box on the top");
    assert_eq!(addition(&s, &box_at([20.0, 2.0, 2.0], 4.0), true), Some(Addition::Apart), "a box apart");
    // A revolve whose volume sits inside the cube: a ring round its middle, inside its walls.
    let mut s = cube();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 5, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [6, 4], "p1": [7, 6]}));
    run(&mut s, "sketch.finish", json!({}));
    let ring = |x: f64| {
        vec![(
            "solid.revolve".to_string(),
            json!({"sketch": "Section", "profiles": [0], "axis": {"origin": [x, 5, 0], "dir": [0, 0, 1]}, "angle": "360 deg", "operation": "new"}),
        )]
    };
    assert_eq!(addition(&s, &ring(5.0), true), Some(Addition::Inside), "a ring inside the cube");
}

#[test]
fn no_verdict_without_an_operation_or_a_body() {
    let s = Session::default();
    assert_eq!(addition(&s, &[("solid.box".into(), json!({"length": 1, "width": 1, "height": 1, "operation": "new"}))], false), None);
    let s = cube();
    assert_eq!(addition(&s, &[("solid.fillet".into(), json!({"edges": [[10, 0, 5]], "radius": 1}))], false), None);
    assert_eq!(addition(&s, &[("solid.combine".into(), json!({"target": "Base", "tools": [], "operation": "join"}))], false), None);
    // A cut keeps its operation: nothing to choose.
    assert_eq!(addition(&s, &[("solid.box".into(), json!({"length": 1, "width": 1, "height": 1, "operation": "cut"}))], false), None);
    // A primitive placed apart, whichever of New Body and Join it has.
    let apart = json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "operation": "join"});
    assert_eq!(addition(&s, &[("solid.box".into(), apart)], false), Some(Addition::Apart));
}

#[test]
fn hidden_bodies_are_out_of_scope() {
    let mut s = cube();
    assert_eq!(crate::shown_scope(&s), None, "all shown: nothing to narrow");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "operation": "new", "body_name": "Other"}));
    run(&mut s, "browser.visibility", json!({"items": ["Base"], "visible": false}));
    assert_eq!(crate::shown_scope(&s), Some(vec!["Other".to_string()]));
    // A block on the hidden cube, with the shown bodies as its targets: apart from all of them.
    let on_hidden = json!({"length": 4, "width": 4, "height": 4, "corner": [2, 2, 10], "operation": "new", "targets": ["Other"]});
    assert_eq!(addition(&s, &[("solid.box".into(), on_hidden)], false), Some(Addition::Apart));
    run(&mut s, "browser.visibility", json!({"items": ["Other"], "visible": false}));
    assert_eq!(crate::shown_scope(&s), Some(Vec::new()), "nothing shown");
}
