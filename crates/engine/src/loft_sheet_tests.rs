//! A loft between straight sections has planar side faces, so a lofted duct converts to sheet
//! metal (#13331).

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The face types of body `b`, as `inspect.measure` counts them.
fn face_types(s: &mut Session, b: &str) -> Value {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0]["face_types"].clone()
}

/// A square of half-width `half` sketched on the plane `z` up.
fn square(s: &mut Session, name: &str, half: f64, z: f64) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": name}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-half, -half], "p1": [half, half]}));
    run(s, "sketch.finish", json!({}));
}

/// #13331: a 110 mm square lofted to a 200 mm square 120 mm above it is a hopper whose four
/// sides each join two parallel lines, so they are planes; shelled 1.5 mm with both ends open
/// and converted to sheet metal with three corner bends, it converts.
#[test]
fn a_loft_between_parallel_squares_converts_to_sheet_metal() {
    let mut s = Session::default();
    run(
        &mut s,
        "sheet.manage_rules",
        json!({"name": "Steel 1.5mm", "thickness": "1.5 mm", "bend_radius": "1.5 mm", "k_factor": "0.4", "active": true}),
    );
    square(&mut s, "Bottom", 55.0, 0.0);
    square(&mut s, "Top", 100.0, 120.0);
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Bottom"}, {"sketch": "Top"}], "body_name": "Hopper"}));
    let types = face_types(&mut s, "Hopper");
    assert_eq!(types["plane"].as_u64(), Some(6), "{types}");
    run(&mut s, "solid.shell", json!({"body": "Hopper", "faces": [[0, 0, 120], [0, 0, 0]], "thickness": "1.5 mm", "direction": "inside"}));
    let types = face_types(&mut s, "Hopper");
    assert_eq!(types["plane"].as_u64(), Some(10), "{types}");
    run(
        &mut s,
        "sheet.convert",
        json!({"body": "Hopper", "face": [0, -77.5, 60], "bends": [[77.5, 77.5, 60], [77.5, -77.5, 60], [-77.5, -77.5, 60]]}),
    );
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

/// Two equal squares loft to a square prism: six planes.
#[test]
fn a_loft_between_equal_squares_is_a_prism_of_planes() {
    let mut s = Session::default();
    square(&mut s, "Bottom", 55.0, 0.0);
    square(&mut s, "Top", 55.0, 120.0);
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Bottom"}, {"sketch": "Top"}], "body_name": "Prism"}));
    let types = face_types(&mut s, "Prism");
    assert_eq!(types["plane"].as_u64(), Some(6), "{types}");
}
