//! WR-646 (#1989; the fork's tangency-labs/smlib@4438859): with
//! `fillet_variable_span_follows_fillet`, `SmFilletIntersector::ComputeNextPoint` rejects
//! a variable round's span whose Hermite rails stray from the fillet solved inside it. On
//! planar faces every uv curve lies on its face, so upstream's test passes any span, the
//! step grows to the angle tolerance and the rails cut across the radius law between the
//! traced points: the round removes the wrong volume and its surface leaves the law's
//! circles.
//!
//! The round of a cube's edge between two perpendicular planes is the union of its balls
//! (WR-685; since the G^n section's flag was retired, on every kernel: WR-685 addendum
//! 2026-11-06). Each ball touches the balls' envelope along its touch circle, square to the
//! centre line and moved along it by `-r r'/|c'|`, not along its great circle. The volume
//! removed is the corner less the union of the balls, by quadrature ([`ENVELOPE_REMOVED`]);
//! the great circles' `(1 - pi/4) r(s)^2` per section, integrated along the edge, removes
//! 65.6068, 2.3% more.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge from `p` to `q` (either way).
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
        })
        .expect("edge")
}

/// The cubic Bezier radius law with control radii `r`.
fn cubic_law(r: [f64; 4]) -> FilletLaw {
    let control = r.map(|x| Vec3::new(x, 0.0, 0.0));
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[4, 4],
        knots: &[0.0, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// The law 6, 6, 1, 1 at `t`.
fn law(t: f64) -> f64 {
    let s = 1.0 - t;
    6.0 * s * s * s + 18.0 * t * s * s + 3.0 * t * t * s + t * t * t
}

/// The law's slope `dr/dt` at `t`.
fn law_slope(t: f64) -> f64 {
    let s = 1.0 - t;
    // 3 (6 - 6) s^2 + 6 (1 - 6) s t + 3 (1 - 1) t^2
    -30.0 * s * t
}

/// The point of the ball at `x = 20 t` on its touch circle in the plane of symmetry
/// `y = z`, on the edge's side: the ball of radius `r` centred `c = (x, 20 - r, 20 - r)`
/// touches the balls' envelope along the circle where `<p - c, c'> = -r r'`
/// (`c' = (1, -r', -r')`, `r' = dr/dx`), of radius `r sqrt(1 - r'^2 / |c'|^2)` about
/// `c - (r r' / |c'|^2) c'`.
fn touch_point(x: f64, r: f64, dr_dx: f64) -> Point3 {
    let c = Vec3::new(x, 20.0 - r, 20.0 - r);
    let cd = Vec3::new(1.0, -dr_dx, -dr_dx);
    let n2 = cd.dot(cd);
    let centre = c - cd * (r * dr_dx / n2);
    let radius = r * (1.0 - dr_dx * dr_dx / n2).sqrt();
    let d = Vec3::new(0.0, 1.0, 1.0);
    let d = d - cd * (d.dot(cd) / n2);
    let p = centre + d * (radius / d.length());
    Point3::new(p.x, p.y, p.z)
}

/// The largest distance from the round's surface of the balls' touch points
/// ([`touch_point`]) at `x = 20 t` (taking the law from whichever end of the edge fits it).
fn surface_off_law(k: &Kernel, b: &Brep) -> f64 {
    let rounds: Vec<_> = b
        .faces()
        .ids()
        .filter_map(|f| face::surface(b, f).unwrap())
        .filter(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
        .collect();
    assert_eq!(rounds.len(), 1, "one round face");
    let s = &rounds[0];
    let off = |reversed: bool| {
        (1..40)
            .map(|i| {
                let x = f64::from(i) * 0.5;
                let t = if reversed { 1.0 - x / 20.0 } else { x / 20.0 };
                let r = law(t);
                let sign = if reversed { -1.0 } else { 1.0 };
                let p = touch_point(x, r, sign * law_slope(t) / 20.0);
                let uv = surface_closest_point(k, s, p).unwrap().uvs[0];
                s.evaluate_point(k, uv).unwrap().distance(p)
            })
            .fold(0.0_f64, f64::max)
    };
    off(false).min(off(true))
}

/// The volume of the 20 mm cube after the law 6, 6, 1, 1 rounds its edge y = z = 20,
/// and the largest distance of the law's sections from the round
/// ([`surface_off_law`]).
fn rounded_cube(fixed: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_variable_span_follows_fillet = fixed;
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let e = edge_between(
        &k,
        &b,
        Point3::new(0.0, 20.0, 20.0),
        Point3::new(20.0, 20.0, 20.0),
    );
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], 6.0, cubic_law([6.0, 6.0, 1.0, 1.0]), 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    let off = surface_off_law(&k, &b);
    (volume(&k, &mut b), off)
}

/// The volume the union of the balls of the law 6, 6, 1, 1 takes off the cube's edge: the
/// integral along the edge of the area each plane `x` cuts between the rails
/// (`y, z > 20 - r(x)`) above the union of the balls' discs (Gauss-Legendre, 16 points in
/// each of 40 panels along the edge and 8 across, the union's top over 20001 balls; 40001
/// balls agree to 1.1e-6).
const ENVELOPE_REMOVED: f64 = 64.159_535;

#[test]
fn variable_round_on_planes_removes_the_laws_volume() {
    let expected = 8000.0 - ENVELOPE_REMOVED;
    let (v, off) = rounded_cube(true);
    eprintln!(
        "fixed: {v} expected {expected} diff {} off {off}",
        v - expected
    );
    // the fork's bound: the law's volume to 1e-4 (here 5e-5 of the volume removed)
    assert!(
        (v - expected).abs() < 1e-4 * ENVELOPE_REMOVED,
        "{v} vs {expected}"
    );
    // the round holds its balls' touch circles
    assert!(off < 3e-4, "{off}");
}

/// Without the fix the rails stray between the traced points: the surface is 8.0e-4 off the
/// balls' touch circles, against 2.0e-4 with it. (The union of the balls removes about the
/// same volume either way, within 3e-5 of the envelope's: before the round was the union of
/// its balls, the great circles' volume showed the stray rails too.)
#[test]
fn variable_round_on_planes_strays_off_its_balls_without_the_fix() {
    let (v, off) = rounded_cube(false);
    eprintln!("upstream: {v} off {off}");
    assert!(off > 5e-4, "{off}");
}
