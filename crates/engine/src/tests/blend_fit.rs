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

/// A size that is no length (zero, negative, too large) is refused by name before any blend is
/// tried, and the part is left as it was.
#[test]
fn chamfer_of_no_length_is_refused() {
    let mut s = open_fixture();
    for d in ["0 mm", "-2 mm", "2e6 mm"] {
        let err =
            s.execute("timeline.edit", &json!({"feature": "Chamfer1", "set": {"distance": d}})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("the blend size must be between 1e-6 and 1e6 mm"), "{d}: {err}");
        assert!((volume(&mut s) - 4000.0).abs() < 1e-6, "{d}");
    }
}
