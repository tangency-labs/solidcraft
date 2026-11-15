//! #1080: a fillet or chamfer running past the faces beside its edge into an opening is built
//! as if the faces went on and cut by the opening (the blend-overflow decision, 237f0ae74), so it
//! never bridges it; one that stops short builds as it always did.

use std::path::Path;

use serde_json::json;

use super::{run, volume};
use crate::Session;

fn open_fixture() -> Session {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/cubeish-chamfer-too-large.sc");
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
/// (the rig's fillets/1080-chamfer-runs-into-openings-cut-by-them): 4000 − (810 − 160) = 3350.
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

/// #199's bar, 14 x 6 x 10, its four upright edges rounded: past a full round of its 6 mm ends
/// the two rounds on each end would cross. Refused, naming 3 mm: the kernel's refusal carries a
/// size and the edge it was found for (`SolidError::FilletRefused`), and the message names the
/// edge.
#[test]
fn a_round_past_a_full_round_is_refused_naming_the_largest_that_fits_and_its_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 14, "width": 6, "height": 10, "corner": [14, 0, 0]}));
    let uprights = json!([[14, 0, 5], [28, 0, 5], [14, 6, 5], [28, 6, 5]]);
    for r in [3.001, 3.5] {
        let e = s.execute("solid.fillet", &json!({"edges": uprights, "radius": r})).unwrap_err().to_string();
        assert!(e.contains("the largest radius that fits is 3 mm"), "{r}: {e}");
        assert!(e.contains("allow, at the edge at ("), "{r}: {e}");
        assert_eq!(e.matches("that fits is").count(), 1, "{r}: {e}");
    }
    assert!((volume(&mut s) - 840.0).abs() < 1e-6, "the part stays as it was");
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

/// #17263: a 12.2 x 2.2 bar swept parallel along an arc of radius about 330 (centre 330 above
/// the middle of a 30 mm chord), its lower long edge at y = 0 rounded at 0.2 mm. The faces
/// beside the edge are 2.2 and 12.2 mm wide, yet a constant round was refused, naming 0.0715
/// mm (about 23.6 / R), while a variable round of 0.2 at both ends built. It builds. The round
/// takes off its corner section A = (1 − π/4) r², whose centroid lies d = r(10 − 3π)/(12 − 3π)
/// inside the convex lower face, swept about the arc's axis through the arc's angle θ
/// (Pappus), and at each end the wedge between the upright end face and the radial plane
/// through the edge's end, A · 15 d / 330.
#[test]
fn a_round_on_a_bar_swept_along_a_wide_arc_is_not_refused_as_too_large() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Path"}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [15, -330], "start": [30, 0], "end": [0, 0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Profile"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [12.2, 2.2]}));
    run(&mut s, "sketch.finish", json!({}));
    let sweep = json!({"sketch": "Profile", "path_sketch": "Path", "path": ["a1"], "operation": "new", "orientation": "parallel"});
    run(&mut s, "solid.sweep", sweep);
    let (r, c) = (0.2, 330.0);
    let big = f64::hypot(15.0, c);
    let edge = json!([[15.0, 0.0, c - big]]);
    run(&mut s, "solid.fillet", json!({"edges": edge, "radius": format!("{r} mm")}));
    let (area, d) = ((1.0 - std::f64::consts::FRAC_PI_4) * r * r, r * (10.0 - 3.0 * std::f64::consts::PI) / (12.0 - 3.0 * std::f64::consts::PI));
    let theta = 2.0 * (15.0 / big).asin();
    let want = 12.2 * 2.2 * 30.0 - area * (big - d) * theta - 2.0 * area * 15.0 * d / c;
    assert!((volume(&mut s) - want).abs() < 1e-6, "{} (want {want})", volume(&mut s));
}
