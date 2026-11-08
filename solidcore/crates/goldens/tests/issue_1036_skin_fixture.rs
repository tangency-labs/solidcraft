//! Generator of `fixtures/boolean.api_brep/issue_1036_coincident_skins.json`
//! (tangency-labs/solidcore#1036): the two capped degree-1 skins of
//! `crates/solidcore/tests/issue_1036_coincident_skin_sides.rs` as `skin` operands. A goes
//! through sections 0..4 and B through 2..6; each section is that test's closed cubic
//! interpolant, written as its NURB stores it (degree, control points, knots), so both
//! sides of the runner build bit-identical curves. Run with `--ignored` to rewrite the
//! fixture; `serde_json`'s `float_roundtrip` keeps every value exact.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{json, Value};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::math::NOW;

/// Section `i` of the #1036 test: a closed cubic interpolant through 12 points of an
/// ellipse at height `10 i`.
fn section(i: usize) -> BSplineCurve {
    let i = f64::from(u32::try_from(i).unwrap());
    let (a, b, z) = (20.0 * (1.0 - 0.04 * i), 12.0 * (1.0 - 0.03 * i), 10.0 * i);
    let mut pts: Vec<Point3> = (0..12)
        .map(|j| {
            let t = 2.0 * std::f64::consts::PI * f64::from(j) / 12.0;
            Point3::new(a * t.cos(), b * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
        .unwrap()
}

fn curve(i: usize) -> Value {
    let c = section(i);
    assert_eq!(c.dim(), 3);
    let g = c.gw_nurb().expect("section without NURB");
    let pts = g.cpts();
    let rational = pts.iter().any(|q| q.w != NOW);
    let cpts: Vec<[f64; 3]> = pts.iter().map(|q| [q.x, q.y, q.z]).collect();
    let mut v = json!({ "p": g.degree(), "cpts": cpts, "knots": g.knots() });
    if rational {
        v["weights"] = json!(pts.iter().map(|q| q.w).collect::<Vec<f64>>());
    }
    v
}

fn skin(from: usize, to: usize) -> Value {
    json!({
        "kind": "skin",
        "counts": vec![1; to - from],
        "curves": (from..to).map(curve).collect::<Vec<_>>(),
        "degree": 1,
        "cap_ends": true,
    })
}

const NOTE: &str = "#1036 (crates/solidcore/tests/issue_1036_coincident_skin_sides.rs): \
two capped degree-1 skins sharing three section curves (A through sections 0..4, B through \
2..6), so their side faces coincide between z = 20 and z = 40. The port gives an empty \
A - B, all of A for A n B and fails A u B (1001) at c9c5979 and at v1.0.1; this case \
shows what the C++ gives.";

#[test]
#[ignore = "generator: rewrites fixtures/boolean.api_brep/issue_1036_coincident_skins.json"]
fn write_issue_1036_skin_fixture() {
    let (a, b) = (skin(0, 5), skin(2, 7));
    let mut cases = Vec::new();
    for (name, api, operation) in [
        ("difference", "difference", 3),
        ("intersection", "intersection", 2),
        ("union", "union", 1),
    ] {
        cases.push(json!({
            "name": format!("issue1036_coincident_skins_{name}"),
            "note": NOTE,
            "inputs": { "api": api, "a": a.clone(), "b": b.clone() },
        }));
        cases.push(json!({
            "name": format!("issue1036_coincident_skins_{name}_measured"),
            "note": NOTE,
            "inputs": {
                "api": "measured",
                "a": a.clone(),
                "b": b.clone(),
                "operation": operation,
            },
        }));
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("fixtures/boolean.api_brep/issue_1036_coincident_skins.json");
    let text = serde_json::to_string_pretty(&json!({ "cases": cases })).unwrap() + "\n";
    std::fs::write(&path, text).unwrap();
}
