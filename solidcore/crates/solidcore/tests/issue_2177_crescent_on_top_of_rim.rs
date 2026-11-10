//! Regression test for #2177 (WR-528, the fork's tangency-labs/smlib@e881cdb): a
//! crescent prism standing on top of a cylinder's rim, touching it only along the r 10
//! arc that runs through the cylinder's seam vertex at (10, 0, 10), united with it.
//!
//! Upstream `CloseIntersectionLoop` relates the rim's long arc, the sole cylinder edge
//! between the crescent's corners, with the crescent's bottom r 6 arc, so the result
//! has an edge 38.16 off its face (level-2 check `#59`/`#82`), a UV trim of degree 3
//! with 2 points and `brep_volume` fails with 1001. With
//! `Kernel::fixes.close_loop_relates_only_coincident_edges` the union passes the
//! level-2 check, every UV trim has at least order points and the volume is exact.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::edgeuses;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
use solidcore::topology::Brep;
use solidcore::{Kernel, Point3, Vec3};

const R: f64 = 10.0;
const R_CRESCENT: f64 = 6.0;
const HEIGHT: f64 = 10.0;
const PRISM: f64 = 5.0;

/// The crescent's area: the r 6 disc centred at (10, 0) less the r 10 disc.
fn crescent_area() -> f64 {
    let (d, r) = (R, R_CRESCENT);
    let lens = r * r * ((d * d + r * r - R * R) / (2.0 * d * r)).acos()
        + R * R * ((d * d + R * R - r * r) / (2.0 * d * R)).acos()
        - 0.5 * ((-d + r + R) * (d + r - R) * (d - r + R) * (d + r + R)).sqrt();
    std::f64::consts::PI * r * r - lens
}

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-8, &attrs)
}

/// The union of the cylinder and the crescent prism on its top rim.
fn union(fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.close_loop_relates_only_coincident_edges = fixed;
    let y = (R * R - 8.2 * 8.2).sqrt();
    let big = y.atan2(8.2).to_degrees();
    let small = y.atan2(8.2 - R).to_degrees();
    let inner = Curve::from(create_arc(Vec3::new(0.0, 0.0, 0.0), R, -big, big).unwrap());
    let outer = Curve::from(create_arc(Vec3::new(R, 0.0, 0.0), R_CRESCENT, -small, small).unwrap());
    let mut tool = create_linear_sweep(
        &mut k,
        &[Some(&inner), Some(&outer)],
        Vec3::new(0.0, 0.0, 1.0),
        PRISM,
        true,
    )
    .unwrap();
    let up = Axis2Placement::new(
        Point3::new(0.0, 0.0, HEIGHT),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    transform(&mut k, &mut tool, &up, None, false).unwrap();
    let tool_volume = volume(&k, &mut tool).unwrap();
    let want = crescent_area() * PRISM;
    assert!((tool_volume - want).abs() <= 1e-6 * want, "{tool_volume}");
    let base = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), R, HEIGHT).unwrap();
    let mut u = boolean_union(&mut k, base, tool).unwrap();
    u.refresh_cached_topology();
    (k, u)
}

/// The edge uses whose UV trim has fewer than order points.
fn short_trims(k: &mut Kernel, u: &mut Brep) -> Vec<String> {
    let mut bad = Vec::new();
    for eu in edgeuses(u, None).unwrap() {
        if eu1::faceuse(u, eu).unwrap().is_none() {
            continue;
        }
        if eu1::uv_trim_curve_pointer(u, eu).unwrap().is_none() {
            eu3::get_or_create_uv_trim_curve(k, u, eu, false, false, true)
                .unwrap()
                .status
                .unwrap();
        }
        if let Some(CurveKind::BSpline(b)) = u
            .edgeuse(eu)
            .unwrap()
            .uv_trim_curve
            .as_ref()
            .map(Curve::kind)
        {
            let n = b.gw_nurb().unwrap();
            if n.degree() < 1 || n.cpts().len() < n.degree() + 1 {
                bad.push(format!(
                    "{eu:?} degree {} points {}",
                    n.degree(),
                    n.cpts().len()
                ));
            }
        }
    }
    bad
}

#[test]
fn crescent_on_top_of_the_rim_is_exact_with_the_fix() {
    let (mut k, mut u) = union(true);
    let c = check_brep(&mut k, &mut u, CheckLevel::Level2).unwrap();
    assert_eq!(c.failure_count(), 0, "{:?}", c.lines());
    let bad = short_trims(&mut k, &mut u);
    assert!(bad.is_empty(), "{bad:?}");
    let want = std::f64::consts::PI * R * R * HEIGHT + crescent_area() * PRISM;
    let v = volume(&k, &mut u).unwrap();
    assert!(
        (v - want).abs() <= 1e-6 * want,
        "volume {v}, expected {want}"
    );
}

#[test]
fn crescent_on_top_of_the_rim_keeps_upstream_behaviour_without_the_fix() {
    let (mut k, mut u) = union(false);
    // at the default fixes the check stops at the short trim (1001 from
    // `BSplineCurve::evaluate_point`); with the check-status fixes on it reports
    // SmBrep test #59 (edge offset 38.16)
    let clean =
        check_brep(&mut k, &mut u, CheckLevel::Level2).is_ok_and(|c| c.failure_count() == 0);
    assert!(!clean, "upstream's union now checks clean");
    assert!(volume(&k, &mut u).is_err());
}
