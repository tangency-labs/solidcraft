//! tangency-labs/solidcore#1979 (WR-647; the fork's tangency-labs/smlib@c2e3d9d): a ring
//! (r 3..5 about (0, 10), z 0..2) unioned with a half-annulus prism (r 4..5 about (e, 10),
//! y >= 10, z 2..20), both linear sweeps of their outlines. The r 5 walls intersect through
//! the curve/curve global solver on the two circles, whose span ends lie within 1e-9 of the
//! other circle: `SmCurve::SimpleCoincidenceChecker` takes the span pair as a coincidence
//! range, although the circles cross inside it, and the range becomes a tiny edge. Upstream
//! (the default) is then non-manifold at e = 1e-4 (`SmTrimmingTools` #0/#2/#3/#4, as the fork
//! reports) and gives no solid (volume 0) at e = 1e-3; with
//! `Kernel::fixes.simple_coincidence_crossing_is_point` the range is the crossing and every
//! offset is a valid manifold solid of the analytic volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(create_line_segment(a, b).unwrap())
}

/// The circle r about `c` in the frame (`x0`, `y0`), from 0 to `degrees`.
fn arc(k: &Kernel, c: Point3, x0: Vec3, y0: Vec3, r: f64, degrees: f64) -> Curve {
    let frame = Axis2Placement::new(c, x0, y0);
    let interval = Extent1d::from_min_max(0.0, degrees);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&interval), None).unwrap())
}

/// The union of the ring and the lip at offset `e`.
fn ring_and_lip(k: &mut Kernel, e: f64) -> Brep {
    let p = Point3::new;
    let o = arc(k, p(0.0, 10.0, 0.0), Vec3::X, Vec3::Y, 5.0, 360.0);
    let i = arc(k, p(0.0, 10.0, 0.0), Vec3::X, Vec3::Y, 3.0, 360.0);
    let outer = create_linear_sweep(k, &[Some(&o)], Vec3::Z, 2.0, true).unwrap();
    let inner = create_linear_sweep(k, &[Some(&i)], Vec3::Z, 2.0, true).unwrap();
    let ring = boolean_difference(k, outer, inner).unwrap();
    let c = p(e, 10.0, 2.0);
    let outline = [
        arc(k, c, Vec3::X, Vec3::Y, 5.0, 180.0),
        line(p(e - 5.0, 10.0, 2.0), p(e - 4.0, 10.0, 2.0)),
        arc(k, c, -Vec3::X, -Vec3::Y, 4.0, -180.0),
        line(p(e + 4.0, 10.0, 2.0), p(e + 5.0, 10.0, 2.0)),
    ];
    let refs: Vec<Option<&Curve>> = outline.iter().map(Some).collect();
    let lip = create_linear_sweep(k, &refs, Vec3::Z, 18.0, true).unwrap();
    boolean_union(k, ring, lip).unwrap()
}

/// Whether the union is a manifold solid, its level-2 check failures and its volume.
fn union_at(e: f64, fix: bool) -> (bool, Vec<String>, f64) {
    let mut k = Kernel::default();
    k.fixes.simple_coincidence_crossing_is_point = fix;
    let mut b = ring_and_lip(&mut k, e);
    let manifold = is_manifold_solid(&b).unwrap();
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    (manifold, check.lines(), volume(&k, &mut b))
}

/// Base pi (25 - 9) 2 plus half of pi (25 - 16) 18: 354.99997.
fn want() -> f64 {
    let pi = core::f64::consts::PI;
    pi * 16.0 * 2.0 + 0.5 * pi * 9.0 * 18.0
}

#[test]
fn issue_1979_ring_lip_union_is_valid_at_every_offset_with_the_fix() {
    let mut report = Vec::new();
    for e in [0.0, 1e-4, 1e-3, 1e-2] {
        let got = union_at(e, true);
        let ok = got.0 && got.1.is_empty() && (got.2 - want()).abs() <= 1e-5 * want();
        if !ok {
            report.push(format!("e = {e}: {got:?}"));
        }
    }
    assert!(report.is_empty(), "want {}: {report:#?}", want());
}

/// Upstream (the default): non-manifold at 1e-4, no solid at 1e-3.
#[test]
fn issue_1979_ring_lip_union_is_invalid_by_default() {
    let (manifold, lines, _) = union_at(1e-4, false);
    assert!(!manifold, "e = 1e-4 is manifold by default: {lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("SmTrimmingTools::Test #4")),
        "{lines:?}"
    );
    let (_, lines, v) = union_at(1e-3, false);
    assert!(!lines.is_empty() && v == 0.0, "e = 1e-3: {lines:?} {v}");
}
