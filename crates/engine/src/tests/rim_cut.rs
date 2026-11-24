//! #882: a cut through the open rim of a shelled case failed once an R1 round went on the outer
//! loop of the other end face. One strip of that round (along a sloped side edge, far from the
//! cut) was never meshed: its boundary was projected onto its surface starting from a corner
//! that a search without a hint misses, so the face was dropped. Every boolean with the body
//! then failed (truck's meshes both operands to classify faces), and the exact fallback refused
//! it for the same face. The owner's design, `battery-tpu-case.sc` (format 18).

use std::path::Path;

use serde_json::json;

use super::run;
use crate::Session;

fn open_at(feature: &str) -> Session {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/battery-tpu-case.sc");
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    run(&mut s, "timeline.roll_to", json!({"feature": feature}));
    s
}

/// The window cut through the front wall and its rim, 48 × 4 × 50 from below the rim.
fn cut_window(s: &mut Session) -> serde_json::Value {
    run(s, "solid.box", json!({"corner": [-24, -0.5, -1], "length": 48, "width": 4, "height": 50, "operation": "cut", "targets": ["Case"]}))
}

/// Every face of the case after the round meshes (the strip had no triangles at any tolerance),
/// and the window cut through the rim then takes its three faces (two sides and the top) in one
/// boolean with no retry, leaving one shell.
#[test]
fn rim_cut_after_the_round_on_the_other_end() {
    let mut s = open_at("Fillet2");
    let case = s.model.state().body("Case").map(|b| b.body.clone()).unwrap();
    let faces = case.faces(0.05).unwrap();
    assert_eq!(faces.len(), case.face_count());
    let unmeshed: Vec<usize> = faces.iter().filter(|f| f.area <= 0.0).map(|f| f.index).collect();
    assert!(unmeshed.is_empty(), "faces with no mesh: {unmeshed:?}");
    let id = cut_window(&mut s)["feature"].as_u64().unwrap();
    let result = s.model.result(id).unwrap();
    assert!(result.error.is_none() && result.warning.is_none(), "{result:?}");
    let case_after = s.model.state().body("Case").map(|b| b.body.clone()).unwrap();
    assert_eq!(case_after.face_count(), case.face_count() + 3);
    assert_eq!(case_after.shell_count(), 1);
}
