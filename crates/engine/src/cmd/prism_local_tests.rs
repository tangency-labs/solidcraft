//! A cut whose walls lie on the target's, in a target that is a prism along the cut only round
//! it (a chamfer or a round elsewhere): the 2D route in a column cut round the tool
//! (`solidcraft_kernel`'s `prism_local`). The B-rep boolean finds no result on these.

use serde_json::{Value, json};
use std::f64::consts::PI;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

/// A cut of `params`: the volume it takes, after checking it built.
fn cut(s: &mut Session, params: Value) -> f64 {
    let before = volume(s);
    checked_cut(s, params);
    before - volume(s)
}

fn checked_cut(s: &mut Session, params: Value) {
    let id = run(s, "solid.extrude", params)["feature"].as_u64().unwrap();
    let r = s.model.result(id).unwrap();
    assert!(r.error.is_none(), "{r:?}");
}

/// An 80 x 50 x 10 plate with a 10 mm round pocket 2 mm deep in its top; `edge` dressed first.
fn plate(edge: impl FnOnce(&mut Session)) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[80,50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":10}));
    edge(&mut s);
    pocket(&mut s, 2.0);
    s
}

/// The round pocket cut `depth` down from the plate's top.
fn pocket(s: &mut Session, depth: f64) -> f64 {
    run(s, "sketch.create", json!({"plane":"XY","offset":10}));
    run(s, "sketch.circle.center", json!({"center":[40,25],"radius":5}));
    run(s, "sketch.finish", json!({}));
    cut(s, json!({"distance":-depth,"operation":"cut"}))
}

/// The pocket made deeper (2 to 5 mm): its wall lies on the pocket's. The plate is a prism
/// along the cut but for its chamfered rim, or its rounded bottom edges.
#[test]
fn a_round_pocket_made_deeper_beside_a_chamfer_or_a_round() {
    let chamfered = |s: &mut Session| {
        run(s, "solid.chamfer", json!({"edges":[[40,0,10],[40,50,10],[0,25,10],[80,25,10]],"distance":1}));
    };
    let rounded = |s: &mut Session| {
        run(s, "solid.fillet", json!({"edges":[[40,0,0],[40,50,0],[0,25,0],[80,25,0]],"radius":2}));
    };
    let want = PI * 25.0 * 3.0;
    for (what, mut s) in [("chamfered", plate(chamfered)), ("rounded", plate(rounded))] {
        let taken = pocket(&mut s, 5.0);
        assert!((taken - want).abs() < want * 1e-3, "{what}: {taken} taken, want {want}");
    }
}

/// #1360's ring cut round a hole, the hole counterbored: the ring's inner wall lies on the
/// hole's, in a disc whose top rim is chamfered. Without the chamfer the disc is a prism and
/// the 2D route takes it whole.
#[test]
fn a_ring_cut_round_a_counterbored_hole_in_a_chamfered_disc() {
    let ring = |chamfer: bool| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":40}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":10}));
        run(&mut s, "solid.hole", json!({"position":[20,0,10],"diameter":6,"type":"counterbore","cb_diameter":9,"cb_depth":1.5}));
        if chamfer {
            run(&mut s, "solid.chamfer", json!({"edges":[[40,0,10]],"distance":1}));
        }
        run(&mut s, "sketch.create", json!({"plane":{"face":[0,0,10]}}));
        run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":3}));
        run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":6}));
        run(&mut s, "sketch.finish", json!({}));
        let before = s.model.state().bodies[0].body.clone();
        // The ring between the counterbore's rim (projected from the face) and the 6 mm circle.
        checked_cut(&mut s, json!({"direction":"negative","distance":3,"operation":"cut","profiles":[{"point":[25.25,0]}]}));
        let after = s.model.state().bodies[0].body.clone();
        // Subtracting two ~49,800 mm³ measurements magnifies their meshing error.
        // Control the chord precision of this small cut independently of the part
        // size, and check convergence rather than loosening its analytic tolerance.
        [1e-4, 2e-5].map(|tol| {
            let volume = |b: &solidcraft_kernel::Body| b.tessellate_with(tol, true).unwrap().measure().volume;
            volume(&before) - volume(&after)
        })
    };
    let want = PI * (36.0 - 20.25) * 3.0;
    for chamfer in [false, true] {
        let measured = ring(chamfer);
        for taken in measured {
            assert!((taken - want).abs() < want * 1e-3, "chamfer {chamfer}: {taken} taken, want {want}");
        }
        assert!((measured[0] - measured[1]).abs() < want * 1e-4, "chamfer {chamfer}: removal must converge as the mesh is refined: {measured:?}");
    }
}
