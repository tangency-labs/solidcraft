//! STEP export regressions found by round-tripping the robustness designs: a body the app built
//! must export, and read back as the same solid.

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_kernel::{measure, step_export, step_import};

fn run(s: &mut Session, script: Value) {
    for c in script.as_array().into_iter().flatten() {
        let (id, p) = (c["command"].as_str().unwrap_or_default(), &c["params"]);
        if let Err(e) = s.execute(id, p) {
            panic!("{id} {p}: {e}");
        }
    }
}

/// The design's one body exports, and reads back closed with the same volume and area.
fn round_trips(s: &Session) {
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1);
    let body = &st.bodies[0].body;
    let text = step_export(&[body], "test").unwrap_or_else(|e| panic!("export: {e}"));
    let imp = step_import(&text).unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed && imp.warnings.is_empty(), "{:?}", imp.warnings);
    let (a, b) = (measure(body).unwrap(), measure(&imp.bodies[0].body).unwrap());
    assert!((a.volume - b.volume).abs() < 1e-3 * a.volume, "volume {} -> {}", a.volume, b.volume);
    assert!((a.area - b.area).abs() < 1e-3 * a.area, "area {} -> {}", a.area, b.area);
}

/// Robustness seed 19, shrunk: a three-quarter turn of a rectangle, a sphere cut, a 1 mm shell.
/// The shell moves revolved faces by a scale across their axis; export refused them and the
/// refusal surfaced as a panic ("a Display implementation returned an error").
#[test]
fn shelled_revolve_with_a_sphere_cut_exports() {
    let mut s = Session::default();
    run(
        &mut s,
        json!([
            {"command":"sketch.create","params":{"plane":"XZ"}},
            {"command":"sketch.rectangle.two_point","params":{"p0":[5.0,0],"p1":[25.0,5.0]}},
            {"command":"sketch.finish","params":{}},
            {"command":"solid.revolve","params":{"angle":"270 deg","axis":"Z"}},
            {"command":"solid.sphere","params":{"center":[-2.5,5.0,-2.5],"operation":"cut","radius":10.0}},
            {"command":"solid.shell","params":{"faces":[[-2.7554552980815448e-15,-15.0,-2.5000000000000004]],"thickness":1.0}}
        ]),
    );
    round_trips(&s);
}

/// Robustness seed 24, shrunk: a three-quarter turn of a rectangle, a chamfer of two edges, a
/// 1.5 mm shell. The same refusal as seed 19's, reached through a chamfer.
#[test]
fn shelled_chamfered_revolve_exports() {
    let mut s = Session::default();
    run(
        &mut s,
        json!([
            {"command":"sketch.create","params":{"plane":"XZ"}},
            {"command":"sketch.rectangle.two_point","params":{"p0":[10.0,0],"p1":[27.5,22.5]}},
            {"command":"sketch.finish","params":{}},
            {"command":"solid.revolve","params":{"angle":"270 deg","axis":"Z"}},
            {"command":"solid.chamfer","params":{"distance":2.0,"edges":[[7.0710678118654755,7.0710678118654755,0.0],[-19.445436482630054,19.44543648263006,0.0]]}},
            {"command":"solid.shell","params":{"faces":[[18.75,0.0,-11.358616634594824]],"thickness":1.5}}
        ]),
    );
    round_trips(&s);
}
