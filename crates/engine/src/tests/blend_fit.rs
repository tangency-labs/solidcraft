//! #1080: a fillet or chamfer running past the faces beside its edge into an opening is built
//! as if the faces went on and cut by the opening (the blend-overflow decision, 237f0ae74), so it
//! never bridges it; one that stops short builds as it always did.

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
/// edge, so the chamfer runs over their rims: it is cut by them. Of its section, the triangle
/// y + (20 − z) <= 9 (area 40.5, along 20: 810), the two openings crossing it hold 80 each
/// (the rig's fillets/1080-chamfer-runs-into-openings-refused): 4000 − (810 − 160) = 3350.
#[test]
fn chamfer_through_the_openings_is_cut_by_them() {
    let mut s = open_fixture();
    assert_eq!(chamfer_error(&s), None);
    assert!((volume(&mut s) - 3350.0).abs() < 1e-6, "{}", volume(&mut s));
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

/// A rib 4 mm wide: blends of its two top edges meet in its top's middle at 2 mm and past that
/// would cross each other. Refused as too large, naming the largest size that fits.
#[test]
fn blends_crossing_each_other_are_refused_naming_the_largest_that_fits() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 4, "height": 20}));
    let edges = json!([[20, 0, 20], [20, 4, 20]]);
    let e = s.execute("solid.chamfer", &json!({"edges": edges, "distance": 3})).unwrap_err().to_string();
    assert!(e.contains("the largest distance that fits is 2 mm"), "{e}");
    let e = s.execute("solid.fillet", &json!({"edges": edges, "radius": 3})).unwrap_err().to_string();
    assert!(e.contains("the largest radius that fits is 2 mm"), "{e}");
    assert!((volume(&mut s) - 3200.0).abs() < 1e-6, "the part stays as it was");
}
