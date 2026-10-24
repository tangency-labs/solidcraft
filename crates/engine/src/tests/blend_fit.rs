//! #1080: a fillet or chamfer too large for the geometry beside its edge is refused, naming the
//! largest size that fits, and one that fits still builds.

use std::path::Path;

use serde_json::json;

use super::{run, volume};
use crate::Session;

fn open_fixture() -> Session {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/cubeish-chamfer-too-large.solidcraft");
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    s
}

fn chamfer_error(s: &Session) -> Option<String> {
    s.model.results.iter().find(|r| r.name == "Chamfer1").and_then(|r| r.error.clone())
}

/// The report's design: a 20 mm cube with a 10 mm square opening into each of three faces,
/// then a 9 mm chamfer on the top edge over one side opening. The openings are 5 mm from the
/// edge, so the chamfer would cut through their rims. It is refused, naming 5 mm, and the part
/// is left as it was.
#[test]
fn chamfer_through_the_openings_is_refused() {
    let mut s = open_fixture();
    let err = chamfer_error(&s).expect("the 9 mm chamfer cuts through two openings: it must be refused");
    assert!(err.contains("runs past the faces beside the edge"), "{err}");
    assert!(err.contains("the largest size that fits is 5 mm"), "{err}");
    assert!((volume(&mut s) - 4000.0).abs() < 1e-6, "the part stays as it was without the chamfer");
}

/// Sizes that stop short of the openings build, and take away exactly the chamfer's triangle
/// along the 20 mm edge.
#[test]
fn chamfer_that_fits_still_builds() {
    let mut s = open_fixture();
    for d in [4.0, 5.0] {
        run(&mut s, "timeline.edit", json!({"feature": "Chamfer1", "set": {"distance": format!("{d} mm")}}));
        assert_eq!(chamfer_error(&s), None, "{d} mm fits");
        let want = 4000.0 - 0.5 * d * d * 20.0;
        assert!((volume(&mut s) - want).abs() < 1e-6, "{d} mm: {} (want {want})", volume(&mut s));
    }
}

/// Edges of the part as written to STL that are not shared by exactly two triangles (keyed by
/// their exact f32 ends, as #784's test does): none when the part is closed.
fn open_edges(s: &Session) -> usize {
    let state = s.model.state();
    let name = state.bodies[0].name.clone();
    let bytes = solidcraft_io::export(&state, std::slice::from_ref(&name), solidcraft_io::Format::StlBinary, &name).unwrap();
    let mut edges = std::collections::HashMap::new();
    for t in &solidcraft_io::read_stl(&bytes).unwrap() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (a.map(f32::to_bits), b.map(f32::to_bits));
            *edges.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
        }
    }
    edges.values().filter(|n| **n != 2).count()
}
/// A T: a 100 × 20 × 15 bar on a pin. A chamfer on the bar's bottom long edge that reaches past
/// the pin's side used to leave the part open where it met the pin (the pin's top under the
/// chamfer had no face), and larger ones were refused. Each now takes away exactly the chamfer's
/// triangle along the bar, keeps the pin whole, and leaves the part closed. (At 4 mm on the 8 mm
/// pin it still leaves one seam on the pin doubled, though its volume is right.)
#[test]
fn a_chamfer_past_a_joined_pin_keeps_the_pin_and_the_part_closed() {
    for (radius, sizes) in [(8.0, vec![2.5, 3.0, 5.0]), (6.0, vec![3.0, 5.0])] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": radius, "height": 30}));
        run(
            &mut s,
            "solid.box",
            json!({"length": 100, "width": 20, "height": 15, "corner": [-50, -10, 30], "operation": "join", "targets": ["Body1"]}),
        );
        let before = volume(&mut s);
        for d in sizes {
            run(&mut s, "solid.chamfer", json!({"edges": [[-30, -10, 30]], "distance": d}));
            let want = before - 0.5 * d * d * 100.0;
            // The pin is faceted where the chamfer crosses it, so allow a little.
            assert!((volume(&mut s) - want).abs() < 0.5, "pin r{radius}, {d} mm: {} (want {want})", volume(&mut s));
            assert_eq!(open_edges(&s), 0, "pin r{radius}, {d} mm: the part is closed");
            run(&mut s, "edit.undo", json!({}));
        }
    }
}
