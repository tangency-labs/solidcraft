//! `sweep::screw` (the SolidCraft fork's `smlib_screw_sweep`, #1162, #1163): a profile in
//! the half-plane `y = 0, x > 0` carried about `+z` through the origin.
//!
//! - Every B-spline wall, sampled five points across each knot span both ways, lies within
//!   the tolerance asked of its true screw surface, end spans included (#1162). A point's
//!   distance is taken in its meridian plane to the profile the motion carries there,
//!   which bounds its distance to the screw surface from above.
//! - A profile line parallel to the axis sweeps exact cylinders of its radius (#1163).
//! - A screw motion keeps volume as a revolution does: a square tooth's sweep is its area
//!   times `2 pi` times its centroid radius times the turns (Pappus).
//! - Every B-spline wall is its definition's net (#2572): the profile under the screw
//!   motion in closed form, within the certified bound of the wall at parameters off its
//!   knots, the bound at most the tolerance asked and no looser than the wall's true
//!   distance; read through `NetScope` as a blend is, and carried through the net's edits.
//!   Every wall face is a definition or of a kind `data/sweep_definition_ledger/` lists.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::procedural_blend::NetScope;
use solidcore::surface::procedural_sweep::{ProceduralSweep, MAX_HALF_ANGLE};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::screw::{
    screw_sweep, screw_sweep_body, screw_sweep_radial, screw_sweep_radial_body,
};
use solidcore::topology::brep::{p1, p2};
use solidcore::topology::face;
use solidcore::SolidError;
use solidcore::{Brep, Kernel};

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

const O: Point3 = Point3::new(0.0, 0.0, 0.0);
const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// A profile in the meridian half-plane: polygon corners `(r, z)`, or one circle.
enum Meridian {
    Polygon(Vec<(f64, f64)>),
    Circle { r: f64, z: f64, radius: f64 },
}

impl Meridian {
    /// The distance in the meridian plane from `(r, z)` to the profile's boundary.
    fn distance(&self, r: f64, z: f64) -> f64 {
        match self {
            Self::Circle {
                r: cr,
                z: cz,
                radius,
            } => ((r - cr).hypot(z - cz) - radius).abs(),
            Self::Polygon(rz) => {
                let mut best = f64::INFINITY;
                for (&(ax, az), &(bx, bz)) in rz.iter().zip(rz.iter().cycle().skip(1)) {
                    let (dx, dz) = (bx - ax, bz - az);
                    let t = (((r - ax) * dx + (z - az) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
                    best = best.min((r - ax - t * dx).hypot(z - az - t * dz));
                }
                best
            }
        }
    }

    fn curves(&self) -> Vec<Curve> {
        match self {
            Self::Polygon(rz) => rz
                .iter()
                .zip(rz.iter().cycle().skip(1))
                .map(|(&(ar, az), &(br, bz))| {
                    Curve::from(
                        create_line_segment(Point3::new(ar, 0.0, az), Point3::new(br, 0.0, bz))
                            .unwrap(),
                    )
                })
                .collect(),
            Self::Circle { r, z, radius } => {
                let frame = Axis2Placement::new(Point3::new(*r, 0.0, *z), Vec3::X, Vec3::Z);
                let c = BSplineCurve::create_circle_segment(
                    3,
                    &frame,
                    *radius,
                    0.0,
                    360.0,
                    NurbCircleParam::Quadratic,
                )
                .unwrap();
                vec![Curve::from(c)]
            }
        }
    }
}

/// The radial offset of a law at `turn` (piecewise linear, held beyond).
fn law_offset(law: &[(f64, f64)], turn: f64) -> f64 {
    let Some(&(t0, o0)) = law.first() else {
        return 0.0;
    };
    if turn <= t0 {
        return o0;
    }
    for w in law.windows(2) {
        let ((ta, oa), (tb, ob)) = (w[0], w[1]);
        if turn <= tb {
            return oa + (ob - oa) * (turn - ta) / (tb - ta);
        }
    }
    law.last().unwrap().1
}

/// The farthest the B-spline walls of `b` stray from the true screw surfaces of `m`,
/// and the cylinder faces' radii.
fn screw_deviation(
    k: &Kernel,
    b: &Brep,
    m: &Meridian,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
) -> (f64, Vec<f64>) {
    let mut worst: f64 = 0.0;
    let mut cylinders = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let surface = face::p2::surface(b, f).unwrap().unwrap();
        match surface.kind() {
            SurfaceKind::Cone(c) => {
                assert!(c.is_cylinder(), "a cone face");
                cylinders.push(c.radius(0.0));
            }
            SurfaceKind::BSpline(s) => {
                let can = s.canonical().unwrap();
                for u in can.u_knots.windows(2) {
                    for v in can.v_knots.windows(2) {
                        if u[1] <= u[0] || v[1] <= v[0] {
                            continue;
                        }
                        for a in 0..=4 {
                            for c in 0..=4 {
                                let uu = u[0] + (u[1] - u[0]) * f64::from(a) / 4.0;
                                let vv = v[0] + (v[1] - v[0]) * f64::from(c) / 4.0;
                                let q = surface.evaluate_point(k, Point2::new(uu, vv)).unwrap();
                                let r = q.x.hypot(q.y);
                                let mut theta = q.y.atan2(q.x);
                                if theta < 0.0 {
                                    theta += 2.0 * PI;
                                }
                                let mut best = f64::INFINITY;
                                for kk in -1..=(turns as i32 + 1) {
                                    let angle = theta + 2.0 * PI * f64::from(kk);
                                    if !(-1e-9..=2.0 * PI * turns + 1e-9).contains(&angle) {
                                        continue;
                                    }
                                    let turn = angle / (2.0 * PI);
                                    best = best.min(
                                        m.distance(r - law_offset(law, turn), q.z - pitch * turn),
                                    );
                                }
                                worst = worst.max(best);
                            }
                        }
                    }
                }
            }
            // the caps
            SurfaceKind::Plane(_) => {}
            _ => panic!("unexpected face surface"),
        }
    }
    (worst, cylinders)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// SolidCraft's coil of case 2552: a 0.9 square from r 4.1 to 5, pitch 1.5, 10 high.
fn coil() -> Meridian {
    Meridian::Polygon(vec![(4.1, 0.0), (5.0, 0.0), (5.0, 0.9), (4.1, 0.9)])
}

/// The groove of an M10x1.5 thread (ISO 68-1).
fn m10_groove(p: f64) -> Meridian {
    let h = p * 3f64.sqrt() / 2.0;
    let slope = (PI / 6.0).tan();
    let r0 = 5.0 - 5.0 * h / 8.0;
    let w0 = p / 8.0;
    let r1 = 5.0 + 0.05 * p;
    let w1 = (p / 4.0 + 2.0 * (r1 - r0) * slope) / 2.0;
    Meridian::Polygon(vec![
        (r0, -p - w0),
        (r1, -p - w1),
        (r1, -p + w1),
        (r0, -p + w0),
    ])
}

#[test]
fn issue_1162_coil_walls_stay_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 10.0 / 1.5);
    let tol = p * 1e-4;
    let m = coil();
    let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
    assert!(!cylinders.is_empty());
}

#[test]
fn issue_1162_round_wire_walls_stay_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 2.25);
    let tol = p * 1e-4;
    let m = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
    // a curved profile sweeps no cylinder
    assert!(cylinders.is_empty());
}

#[test]
fn issue_1163_axial_lines_sweep_exact_cylinders() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 3.0);
    let tol = p * 1e-4;
    for (m, radii) in [
        (coil(), vec![4.1, 5.0]),
        (m10_groove(p), {
            let h = p * 3f64.sqrt() / 2.0;
            vec![5.0 - 5.0 * h / 8.0, 5.0 + 0.05 * p]
        }),
    ] {
        let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
        assert!(p2::is_manifold_solid(&b).unwrap());
        let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
        assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
        // both lines along the axis, in half turns: at least 2 x 6 cylinder faces
        assert!(cylinders.len() >= 12, "{} cylinder faces", cylinders.len());
        for r in cylinders {
            assert!(
                radii.iter().any(|&x| (r - x).abs() <= 1e-12),
                "cylinder radius {r}, profile lines at {radii:?}"
            );
        }
    }
}

#[test]
fn square_tooth_volume_is_pappus() {
    let mut k = Kernel::default();
    let tooth = Meridian::Polygon(vec![(4.0, 0.0), (4.4, 0.0), (4.4, 0.4), (4.0, 0.4)]);
    let expected = 0.16 * 2.0 * PI * 4.2 * 3.0;
    for pitch in [1.0, -1.0] {
        let mut b = screw_sweep(&mut k, &tooth.curves(), O, Z, pitch, 3.0, 1e-4).unwrap();
        let v = volume(&k, &mut b);
        assert!(
            (v - expected).abs() <= 2e-3 * expected,
            "pitch {pitch}: volume {v}, expected {expected}"
        );
    }
}

#[test]
fn radial_runout_stays_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 4.0);
    let tol = p * 1e-4;
    let m = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    // full depth after a turn and a half, out again over the last
    let law = [(0.0, -0.3), (1.5, 0.0), (2.5, 0.0), (4.0, -0.3)];
    let b = screw_sweep_radial(&mut k, &m.curves(), O, Z, p, turns, &law, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, _) = screw_deviation(&k, &b, &m, p, turns, &law);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
}

#[test]
fn out_of_range_inputs_are_refused() {
    let mut k = Kernel::default();
    let c = coil().curves();
    assert!(screw_sweep(&mut k, &c, O, Z, 0.0, 3.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &c, O, Z, 1.0, 0.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &c, O, Z, 1.0, 3.0, 0.0).is_err());
    assert!(screw_sweep(&mut k, &c, O, Vec3::ZERO, 1.0, 3.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &[], O, Z, 1.0, 3.0, 1e-4).is_err());
    // a law with turns out of order, or past the turns
    let bad = [(1.0, 0.1), (0.5, 0.0)];
    assert!(screw_sweep_radial(&mut k, &c, O, Z, 1.0, 3.0, &bad, 1e-4).is_err());
    let late = [(0.0, 0.1), (4.0, 0.0)];
    assert!(screw_sweep_radial(&mut k, &c, O, Z, 1.0, 3.0, &late, 1e-4).is_err());
}

#[test]
fn body_variants_stop_before_the_solid_check() {
    // The `_body` variants give the same body as the checked entry points where it is a
    // solid, and refuse out-of-range input as `InvalidInput` (the C API's
    // `SMLIB_ERR_ARG`).
    let mut k = Kernel::default();
    let c = coil().curves();
    let checked = screw_sweep(&mut k, &c, O, Z, 1.5, 2.0, 1.5e-4).unwrap();
    let body = screw_sweep_body(&mut k, &c, O, Z, 1.5, 2.0, 1.5e-4).unwrap();
    assert!(p2::is_manifold_solid(&body).unwrap());
    assert_eq!(
        p1::faces(&body, None).unwrap().len(),
        p1::faces(&checked, None).unwrap().len()
    );
    let law = [(0.0, -0.2), (2.0, 0.0)];
    let body = screw_sweep_radial_body(&mut k, &c, O, Z, 1.5, 2.0, &law, 1.5e-4).unwrap();
    assert!(p2::is_manifold_solid(&body).unwrap());
    assert!(matches!(
        screw_sweep_body(&mut k, &c, O, Z, 0.0, 2.0, 1.5e-4),
        Err(SolidError::InvalidInput { .. })
    ));
    assert!(matches!(
        screw_sweep_radial_body(&mut k, &c, O, Z, 1.5, 2.0, &[(3.0, 0.0)], 1.5e-4),
        Err(SolidError::InvalidInput { .. })
    ));
}

// ---- #2572: every wall is its definition's net ------------------------------------------

/// The B-spline wall faces' surfaces of `b`.
fn walls(b: &Brep) -> Vec<Surface> {
    p1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter_map(|f| {
            let s = face::p2::surface(b, f).unwrap().unwrap();
            matches!(s.kind(), SurfaceKind::BSpline(_)).then(|| s.clone())
        })
        .collect()
}

/// The screw surface about `+z` through the origin in closed form, written out here: the
/// profile point `p` moved `offset` along `+x` (the profile's half-plane), turned by
/// `theta`, lifted `pitch theta / (2 pi)`.
fn screw_point(p: Point3, offset: f64, theta: f64, pitch: f64) -> Point3 {
    let (x, y) = (p.x + offset, p.y);
    Point3::new(
        x * theta.cos() - y * theta.sin(),
        x * theta.sin() + y * theta.cos(),
        p.z + pitch * theta / (2.0 * PI),
    )
}

/// The angle the net's arcs turn to at `v` on the span `[v0, v1]` of half angle `half`.
fn arc_angle(v0: f64, v1: f64, half: f64, v: f64) -> f64 {
    let s = 2.0 * (v - v0) / (v1 - v0) - 1.0;
    0.5 * (v0 + v1) + 2.0 * ((0.5 * half).tan() * s).atan()
}

/// The face's `(u, v)` of the definition's `(w, t)`, through its map.
fn face_uv(d: &ProceduralSweep, w: f64, t: f64) -> Point2 {
    let ([a0, a1], [b0, b1], swapped) = d.definition_map();
    let (p, q) = ((w - a0) / b0, (t - a1) / b1);
    if swapped {
        Point2::new(q, p)
    } else {
        Point2::new(p, q)
    }
}

/// What one wall's checks found: its bound and the farthest its net strays from its
/// definition at the parameters sampled.
struct WallCheck {
    bound: f64,
    farthest: f64,
    half: f64,
}

/// Checks a wall against its definition over every span of its net, at parameters off
/// its knots (the lift's error is largest at `s = +-1 / sqrt 3` of a span, the offset's at
/// its middle), read through the definition's map (the face's construction may have
/// swapped or reparametrised its net): the net lies within the bound of S, and S is the
/// closed-form screw surface of the profile at the angle the net's arc turns to (with the
/// law's offset), to rounding.
fn check_wall(wall: &Surface, pitch: f64, law: &[(f64, f64)]) -> WallCheck {
    let sweep = wall
        .procedural_sweep()
        .expect("a screw wall is its definition");
    let d: &ProceduralSweep = sweep.definition();
    let net = sweep.net();
    assert_eq!(d.screw().unwrap().pitch(), pitch);
    assert_eq!(d.screw().unwrap().axis(), Z);
    assert_eq!(d.screw().unwrap().origin(), O);
    assert_eq!(d.screw().unwrap().law().is_some(), !law.is_empty());
    assert!(d.half_angle() > 0.0 && d.half_angle() <= MAX_HALF_ANGLE);
    let ([_, a1], [_, b1], swapped) = d.definition_map();
    // the definition's stations are the net's knots across the turns, through the map
    let can = net.canonical().unwrap();
    let across = if swapped { &can.u_knots } else { &can.v_knots };
    let mut knots: Vec<f64> = across.iter().map(|&q| a1 + b1 * q).collect();
    knots.sort_by(f64::total_cmp);
    assert_eq!(knots.len(), d.stations().len());
    for (q, t) in knots.iter().zip(d.stations()) {
        assert!(
            (q - t).abs() <= 1e-12 * (1.0 + t.abs()),
            "knot {q}, station {t}"
        );
    }
    let bound = sweep.bound();
    assert!(bound > 0.0 && bound == d.bound());
    let profile = d.profile().natural_interval();
    let r = 1.0 / 3f64.sqrt();
    let mut farthest: f64 = 0.0;
    for span in d.stations().windows(2) {
        for s in [-0.93, -r, -0.31, 0.0, 0.17, r, 0.88] {
            let t = span[0] + 0.5 * (s + 1.0) * (span[1] - span[0]);
            let theta = arc_angle(span[0], span[1], d.half_angle(), t);
            assert_eq!(d.turn_at(t).0, theta);
            for a in [0.13, 0.5, 0.87] {
                let w = profile.min() + a * (profile.max() - profile.min());
                let uv = face_uv(d, w, t);
                let at = d.definition_parameters(uv);
                assert!((at.x - w).abs() <= 1e-12 && (at.y - t).abs() <= 1e-12 * (1.0 + t));
                let b = net.evaluate_point(uv).unwrap();
                let def = d.evaluate_definition_point(uv).unwrap();
                let gap = (b - def).length();
                assert!(gap <= bound, "the net strays {gap} from S, bound {bound}");
                farthest = farthest.max(gap);
                let (theta, _) = d.turn_at(at.y);
                let p = d.profile().evaluate_point(at.x).unwrap();
                let offset = law_offset(law, theta / (2.0 * PI));
                let expected = screw_point(p, offset, theta, pitch);
                assert!(
                    (def - expected).length() <= 1e-12 * (1.0 + expected.length()),
                    "S {def:?} is not the screw surface's {expected:?}"
                );
            }
        }
    }
    WallCheck {
        bound,
        farthest,
        half: d.half_angle(),
    }
}

#[test]
fn issue_2572_m10_groove_walls_are_their_definitions() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 3.0);
    let tol = p * 1e-4;
    let b = screw_sweep(&mut k, &m10_groove(p).curves(), O, Z, p, turns, tol).unwrap();
    let walls = walls(&b);
    // the two flanks, in half-turn pieces (the groove's axial lines make cylinders)
    assert!(walls.len() >= 12, "{} walls", walls.len());
    let mut bounds = Vec::new();
    for wall in &walls {
        let c = check_wall(wall, p, &[]);
        // the wall was built within the tolerance of its screw surface, so its certified
        // bound is too; and it is the wall's own distance, not a looser number
        assert!(c.bound <= tol, "bound {} past the tolerance {tol}", c.bound);
        assert!(
            c.farthest >= 0.99 * c.bound,
            "bound {} loose: the wall strays {}",
            c.bound,
            c.farthest
        );
        bounds.push(c.bound);
    }
    println!("M10x1.5 groove walls: bounds {bounds:?}");
}

/// A caller reads through the net at a tolerance of at least the bound, and through the
/// definition tighter than that or outside every scope.
#[test]
fn issue_2572_net_scope_reads_the_net_at_or_above_the_bound() {
    let mut k = Kernel::default();
    let p = 1.5;
    let b = screw_sweep(&mut k, &m10_groove(p).curves(), O, Z, p, 2.0, p * 1e-4).unwrap();
    let wall = walls(&b).into_iter().next().unwrap();
    let sweep = wall.procedural_sweep().unwrap();
    let bound = sweep.bound();
    let d = sweep.definition();
    let profile = d.profile().natural_interval();
    let uv = face_uv(
        d,
        profile.min() + 0.3 * (profile.max() - profile.min()),
        0.6 * d.stations()[0] + 0.4 * d.stations()[1],
    );
    let net = sweep.net().evaluate_point(uv).unwrap();
    let def = sweep.definition().evaluate_definition_point(uv).unwrap();
    assert_ne!(net, def);
    assert_eq!(sweep.evaluate_point(&k, uv).unwrap(), def);
    {
        let _at = NetScope::within(&k, bound);
        assert_eq!(sweep.evaluate_point(&k, uv).unwrap(), net);
        let _below = NetScope::within(&k, 0.5 * bound);
        assert_eq!(sweep.evaluate_point(&k, uv).unwrap(), def);
    }
    let _any = NetScope::new(&k);
    assert_eq!(sweep.evaluate_point(&k, uv).unwrap(), net);
    // the definition's partials are those of its points
    let _exact = NetScope::exact(&k);
    let d = sweep.evaluate(&k, uv, 1, 1).unwrap();
    assert_eq!(d[0][0], def);
    let step = 1e-6;
    let at = |du: f64, dv: f64| {
        sweep
            .evaluate_point(&k, Point2::new(uv.x + du, uv.y + dv))
            .unwrap()
    };
    let su = (at(step, 0.0) - at(-step, 0.0)) * (0.5 / step);
    let sv = (at(0.0, step) - at(0.0, -step)) * (0.5 / step);
    let wide = 1e-4;
    let suv = (at(wide, wide) - at(wide, -wide) - at(-wide, wide) + at(-wide, -wide))
        * (0.25 / (wide * wide));
    assert!(
        (d[1][0] - su).length() <= 1e-6 * su.length(),
        "{:?} {su:?}",
        d[1][0]
    );
    assert!(
        (d[0][1] - sv).length() <= 1e-6 * sv.length(),
        "{:?} {sv:?}",
        d[0][1]
    );
    assert!(
        (d[1][1] - suv).length() <= 1e-3 * suv.length(),
        "{:?} {suv:?}",
        d[1][1]
    );
}

/// A tapered thread: the groove moved in along a radial law over the whole length. Its
/// walls (the axial lines' too, the offset changing along them) are definitions with the
/// law, the law's error in their bound.
#[test]
fn issue_2572_tapered_thread_walls_carry_their_law() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 3.0);
    let tol = p * 1e-4;
    let taper = [(0.0, 0.0), (turns, -0.4)];
    let curves = m10_groove(p).curves();
    let b = screw_sweep_radial(&mut k, &curves, O, Z, p, turns, &taper, tol).unwrap();
    let walls = walls(&b);
    // no cylinders: every one of the four profile lines makes B-spline walls
    let faces = p1::faces(&b, None).unwrap().len();
    assert_eq!(
        walls.len() + 2,
        faces,
        "{} walls of {faces} faces",
        walls.len()
    );
    for wall in &walls {
        let law = wall
            .procedural_sweep()
            .unwrap()
            .definition()
            .screw()
            .unwrap()
            .law()
            .unwrap();
        assert_eq!(law.direction(), Vec3::X);
        assert_eq!(law.offsets(), [0.0, -0.4]);
        assert_eq!(law.angles(), [0.0, 2.0 * PI * turns]);
        let c = check_wall(wall, p, &taper);
        assert!(c.bound <= tol, "bound {} past the tolerance {tol}", c.bound);
        assert!(
            c.farthest >= 0.9 * c.bound,
            "bound {} loose: {}",
            c.bound,
            c.farthest
        );
        // the wall strays past what its lift alone could make it (the rational lift's
        // largest error on a span of its half angle): the law's error is real, and the
        // bound holds it
        let tau = (0.5 * c.half).tan();
        let kk = 2.0 * (1.0 + tau * tau) * tau.atan() / tau;
        let x = ((kk - 2.0) / (kk + 2.0)).sqrt();
        let lift = p / (2.0 * PI) * (kk * x / (1.0 + x * x) - 2.0 * x.atan());
        assert!(
            c.farthest > 2.0 * lift,
            "strays {}, the lift {lift}",
            c.farthest
        );
    }
}

#[test]
fn issue_2572_runout_and_round_wire_walls_are_their_definitions() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 4.0);
    let tol = p * 1e-4;
    let wire = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    let law = [(0.0, -0.3), (1.5, 0.0), (2.5, 0.0), (4.0, -0.3)];
    let b = screw_sweep_radial(&mut k, &wire.curves(), O, Z, p, turns, &law, tol).unwrap();
    for wall in &walls(&b) {
        let c = check_wall(wall, p, &law);
        assert!(c.bound <= tol, "bound {} past the tolerance {tol}", c.bound);
    }
    // a left-handed coil
    let b = screw_sweep(&mut k, &coil().curves(), O, Z, -p, 2.0, tol).unwrap();
    for wall in &walls(&b) {
        assert!(check_wall(wall, -p, &[]).bound <= tol);
    }
}

/// The definition follows its net: a motion moves both (the bound taking in the motion's
/// rounding), a reversal, a swap and a reparametrisation move its map, a piece keeps it;
/// a scale that is not uniform leaves no screw motion, and the net moves without it.
#[test]
fn issue_2572_the_definition_follows_its_nets_edits() {
    let mut k = Kernel::default();
    let p = 1.5;
    let b = screw_sweep(&mut k, &m10_groove(p).curves(), O, Z, p, 2.0, p * 1e-4).unwrap();
    let wall = walls(&b).into_iter().next().unwrap();
    let bound = wall.procedural_sweep().unwrap().bound();
    // the net within the bound of S at a grid of the face's (u, v)
    let within = |s: &Surface| -> f64 {
        let sweep = s.procedural_sweep().expect("still a definition");
        let dom = sweep.net().natural_uv_domain();
        let (u, v) = (dom.u_interval(), dom.v_interval());
        let mut far: f64 = 0.0;
        // a square grid, symmetric both ways, so a reversal or a swap samples the same points
        for i in 1..48 {
            for j in 1..48 {
                let uv = Point2::new(
                    u.min() + (u.max() - u.min()) * f64::from(i) / 48.0,
                    v.min() + (v.max() - v.min()) * f64::from(j) / 48.0,
                );
                let gap = (sweep.net().evaluate_point(uv).unwrap()
                    - sweep.definition().evaluate_definition_point(uv).unwrap())
                .length();
                assert!(gap <= sweep.bound(), "{gap} past {}", sweep.bound());
                far = far.max(gap);
            }
        }
        far
    };
    let base = within(&wall);
    assert!(base > 0.5 * bound);
    let place = Axis2Placement::new(
        Point3::new(10.0, -3.0, 7.0),
        Vec3::new(0.6, 0.8, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let mut moved = wall.copy(&k).unwrap();
    moved
        .transform(&k, &place, Some(Vec3::new(2.0, 2.0, 2.0)))
        .unwrap();
    let m = moved
        .procedural_sweep()
        .unwrap()
        .definition()
        .screw()
        .unwrap()
        .clone();
    assert_eq!(m.pitch(), 2.0 * p);
    assert!((m.axis() - place.z_axis()).length() <= 1e-15);
    assert!((m.origin() - place.origin()).length() <= 1e-15);
    let moved_bound = moved.procedural_sweep().unwrap().bound();
    assert!(moved_bound >= 2.0 * bound && moved_bound <= 2.0 * bound * (1.0 + 1e-9));
    assert!((within(&moved) - 2.0 * base).abs() <= 1e-9 * base);
    for edit in 0..4 {
        let mut e = wall.copy(&k).unwrap();
        match edit {
            0 => e.reverse(&k, SurfParamType::U).unwrap(),
            1 => e.reverse(&k, SurfParamType::V).unwrap(),
            2 => e.swap_uv(&k).unwrap(),
            _ => {
                let dom = solidcore::geom::extent2d::Extent2d::from_bounds(-1.0, 3.0, 2.0, 9.0);
                e.reparameterize(&k, &dom).unwrap();
            }
        }
        assert!((within(&e) - base).abs() <= 1e-9 * base, "edit {edit}");
    }
    let dom = wall.procedural_sweep().unwrap().net().natural_uv_domain();
    let at = 0.5 * (dom.v_interval().min() + dom.v_interval().max());
    let (l, r) = wall.split_at(&k, at, SurfParamType::V).unwrap();
    within(&l);
    within(&r);
    let mut squashed = wall.copy(&k).unwrap();
    squashed
        .transform(
            &k,
            &Axis2Placement::default(),
            Some(Vec3::new(1.0, 1.0, 2.0)),
        )
        .unwrap();
    assert!(squashed.procedural_sweep().is_none());
    assert!(matches!(squashed.kind(), SurfaceKind::BSpline(_)));
}

/// The ledger: each kind of wall face that is not a definition yet, with its issue (the
/// first `#n` of its file).
fn sweep_ledger() -> BTreeMap<String, u32> {
    // (the `path-` kinds are the general-path sweeps', tests/general_path_sweep.rs)
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sweep_definition_ledger"
    );
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if stem.starts_with("path-") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let issue = text
            .split('#')
            .nth(1)
            .and_then(|t| {
                t.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("{stem}: the ledger entry names no issue"));
        out.insert(stem, issue);
    }
    out
}

/// Every wall face of every screw sweep case (the caps aside) is a definition, or of a
/// kind the ledger lists; and every listed kind still appears.
#[test]
fn issue_2572_every_wall_is_a_definition_or_in_the_ledger() {
    let mut k = Kernel::default();
    let (p, tol) = (1.5, 1.5e-4);
    let wire = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    let runout = [(0.0, -0.3), (1.5, 0.0), (2.5, 0.0), (4.0, -0.3)];
    let taper = [(0.0, 0.0), (3.0, -0.4)];
    let cases = vec![
        screw_sweep(&mut k, &coil().curves(), O, Z, p, 10.0 / p, tol).unwrap(),
        screw_sweep(&mut k, &m10_groove(p).curves(), O, Z, p, 3.0, tol).unwrap(),
        screw_sweep(&mut k, &m10_groove(p).curves(), O, Z, -p, 3.0, tol).unwrap(),
        screw_sweep(&mut k, &wire.curves(), O, Z, p, 2.25, tol).unwrap(),
        screw_sweep_radial(&mut k, &wire.curves(), O, Z, p, 4.0, &runout, tol).unwrap(),
        screw_sweep_radial(&mut k, &m10_groove(p).curves(), O, Z, p, 3.0, &taper, tol).unwrap(),
    ];
    let ledger = sweep_ledger();
    let mut bare: BTreeSet<String> = BTreeSet::new();
    let mut definitions = 0;
    for b in &cases {
        for f in p1::faces(b, None).unwrap() {
            let s = face::p2::surface(b, f).unwrap().unwrap();
            if s.procedural_definition().is_some() {
                definitions += 1;
                continue;
            }
            let kind = match s.kind() {
                // the caps
                SurfaceKind::Plane(_) => continue,
                SurfaceKind::Cone(c) if c.is_cylinder() => "screw-cylinder",
                SurfaceKind::BSpline(_) => "screw-b-spline",
                _ => "screw-other",
            };
            bare.insert(kind.to_owned());
        }
    }
    assert!(definitions > 0);
    let unlisted: Vec<&String> = bare.iter().filter(|kd| !ledger.contains_key(*kd)).collect();
    assert!(
        unlisted.is_empty(),
        "bare wall faces of kinds the ledger does not list: {unlisted:?}"
    );
    let fixed: Vec<&String> = ledger.keys().filter(|kd| !bare.contains(*kd)).collect();
    assert!(
        fixed.is_empty(),
        "listed kinds no case makes bare any more (fixed? delete their ledger files): {fixed:?}"
    );
}
