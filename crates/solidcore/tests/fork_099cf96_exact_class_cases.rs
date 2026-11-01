//! The SolidCraft fork's tangency-labs/smlib@099cf96 (the first commit of smlib#77, #2244):
//! constant rounds whose rolling ball's centre line (the spine, where the faces' offsets by
//! the radius meet) is a rational curve, so the round is an exact rational surface, and one
//! whose spine is not, which is a B-spline within the fillet's tolerance.
//!
//! Each case is graded as the fork's `exact_round` grades it: the status, the volume (rel
//! 1e-6, at accuracy 1e-7), SMLib's validity check at level 2, the kind and degrees of every
//! round face, and every point of a 13 x 13 grid of those faces' surfaces, moved the radius
//! along its normal (either way), at the radius from each of the faces' exact quadrics (to
//! 1e-9 mm when exact, to the fillet's tolerance, 100 times the body's, when fitted). The
//! constructions and the volumes are the fork's.
//!
//! The marks are the fork's, as `EXACT_CLASS_CASES` has them:
//!
//! | Case | 099cf96 (before the exact classes) | 7ac037e and d01d7b0 |
//! |---|---|---|
//! | `coaxial_cones` | passes | passes |
//! | `cone_cylinder_sharing_a_ball` | traced, (3, 3), 3.96e-4 off | passes |
//! | `cylinders_touching_at_a_point` | status 1001 | status 1001 |
//! | `oblique_plane_cone` | traced, (3, 3), 9.4e-4 off | passes |
//! | `oblique_plane_cylinder` | traced, (3, 3), 2.26e-3 off | passes |
//! | `sphere_coaxial_cone` | traced, (3, 3), 1.47e-3 off | passes |
//! | `sphere_plane` | status 1001 | status 1001 |
//! | `tee_is_fitted` | 1.47e-3 off, tolerance 1e-3 | 1.47e-3 off, tolerance 1e-3 |
//!
//! The fork's later marks say where its 1001s come from: `SmFilletNx1Corner`, after the
//! exact round is built, where the round ends on the cut (the pipes) or at the rim's one
//! vertex on the sphere's seam (the ball). Only the status is checked here: which step of
//! the fillet returned it isn't visible through `fillet_edges`.
//!
//! The left column is checked on the default kernel and the right one with every fix
//! SolidCraft turns on and with `fillet_exact_round_classes` alone. One difference from the
//! fork: the two pipes' union itself fails 1001 here without
//! `ssi_split_curves_at_singular_points` (WR-77), so the pipes get that flag wherever the
//! fork's mark is about their round.
//!
//! How this test drives the kernel differs from the capi: edges are picked as the nearest
//! within 0.1 of each point, and `fillet_edges` runs with section type 1, continuity 1 and
//! thumbweight 1, the arguments `round_edges` gives `SetFilletParameters`, without the capi's
//! overflow handling, which takes more than one edge (each case here rounds one).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::{create_cone, create_cylinder, create_sphere};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, Result};

type V3 = [f64; 3];
const X_AXIS: V3 = [1.0, 0.0, 0.0];
const Y_AXIS: V3 = [0.0, 1.0, 0.0];
const Z_AXIS: V3 = [0.0, 0.0, 1.0];
const DOWN: V3 = [0.0, 0.0, -1.0];

fn pt(a: V3) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

fn vc(a: V3) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// The faces a round rolls on, as exact quadrics.
#[derive(Clone, Copy)]
enum Quadric {
    /// A point and the unit normal.
    Plane(V3, V3),
    /// An axis point, the unit axis and the radius.
    Cylinder(V3, V3, f64),
    /// The apex, the unit axis into its nappe and the half-angle.
    Cone(V3, V3, f64),
    /// The centre and the radius.
    Sphere(V3, f64),
}

impl Quadric {
    /// The distance of `c` from the quadric.
    fn distance(self, c: Point3) -> f64 {
        let parts = |p: V3, d: V3| {
            let w = [c.x - p[0], c.y - p[1], c.z - p[2]];
            let along = w[0] * d[0] + w[1] * d[1] + w[2] * d[2];
            let squared = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
            (along, (squared - along * along).max(0.0).sqrt(), squared)
        };
        match self {
            Self::Plane(p, n) => parts(p, n).0.abs(),
            Self::Cylinder(p, d, r) => (parts(p, d).1 - r).abs(),
            Self::Cone(p, d, half) => {
                let (along, across, _) = parts(p, d);
                (across * half.cos() - along * half.sin()).abs()
            }
            Self::Sphere(p, r) => (parts(p, Z_AXIS).2.sqrt() - r).abs(),
        }
    }
}

/// What the round's surface must be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RoundKind {
    /// A rational B-spline of the given degrees, either way round.
    Rational(usize, usize),
    /// The ball's arc turned about the faces' common axis.
    Revolved,
    /// A B-spline within the fillet's tolerance of the canal surface.
    Fitted,
}

/// One case: the fork's body, edge points, radius, volume, surface kind and faces.
struct Case {
    name: &'static str,
    build: fn(&mut Kernel) -> Result<Brep>,
    points: Vec<V3>,
    r: f64,
    want: f64,
    kind: RoundKind,
    faces: Vec<Quadric>,
}

/// A primitive placed: its x and y axes onto `x`, `y` (its z follows), then moved by `t`.
fn placed(k: &mut Kernel, mut b: Brep, x: V3, y: V3, t: V3) -> Result<Brep> {
    let pl = Axis2Placement::new(pt(t), vc(x), vc(y));
    solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false)?;
    Ok(b)
}

/// A disc of radius 100 and height 100 standing on the plane through `on` with unit normal
/// `n`: cuts away the half-space above it, as far as any case here reaches.
fn cutter(k: &mut Kernel, on: V3, n: V3) -> Result<Brep> {
    let c = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0)?;
    let lx = (n[2] * n[2] + n[0] * n[0]).sqrt();
    let x = [n[2] / lx, 0.0, -n[0] / lx];
    let y = [
        n[1] * x[2] - n[2] * x[1],
        n[2] * x[0] - n[0] * x[2],
        n[0] * x[1] - n[1] * x[0],
    ];
    placed(k, c, x, y, on)
}

fn oblique_cylinder_normal() -> V3 {
    let s = 1.25f64.sqrt();
    [-0.5 / s, 0.0, 1.0 / s]
}

fn oblique_cone_normal() -> V3 {
    let s = 1.09f64.sqrt();
    [-0.3 / s, 0.0, 1.0 / s]
}

/// A cylinder R 10 about z, cut by the plane z = 20 + x / 2.
fn oblique_plane_cylinder(k: &mut Kernel) -> Result<Brep> {
    let cyl = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 40.0)?;
    let cut = cutter(k, [0.0, 0.0, 20.0], oblique_cylinder_normal())?;
    boolean_difference(k, cyl, cut)
}

/// A frustum about z, R 10 at z = 0 narrowing at 10 degrees, 30 high, cut by the plane
/// z = 15 + 0.3 x.
fn oblique_plane_cone(k: &mut Kernel) -> Result<Brep> {
    let ta = 10f64.to_radians().tan();
    let cone = create_cone(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0 - 30.0 * ta, 30.0)?;
    let cut = cutter(k, [0.0, 0.0, 15.0], oblique_cone_normal())?;
    boolean_difference(k, cone, cut)
}

/// A ball R 10 cut flat at z = 4.
fn sphere_plane(k: &mut Kernel) -> Result<Brep> {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0)?;
    let cut = cutter(k, [0.0, 0.0, 4.0], Z_AXIS)?;
    boolean_difference(k, ball, cut)
}

/// Two coaxial frusta joined: R 10 to 6 over z 0..8, then R 6 to 1 over z 8..9.
fn coaxial_cones(k: &mut Kernel) -> Result<Brep> {
    let a = create_cone(k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 8.0)?;
    let b = create_cone(k, Point3::new(0.0, 0.0, 8.0), 6.0, 1.0, 1.0)?;
    boolean_union(k, a, b)
}

/// A ball R 10 and a frustum about z through its centre, R 5 at z = 0 to R 3 at z = 20.
fn sphere_coaxial_cone(k: &mut Kernel) -> Result<Brep> {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0)?;
    let cone = create_cone(k, Point3::new(0.0, 0.0, 0.0), 5.0, 3.0, 20.0)?;
    boolean_union(k, ball, cone)
}

/// A cone about z (apex (0, 0, 10 sqrt 2), half-angle 45 degrees, base at z = -10) and a post
/// R 10 whose axis, tilted 20 degrees from z in the xz plane, passes through the origin.
fn cone_cylinder_sharing_a_ball(k: &mut Kernel) -> Result<Brep> {
    let g = 20f64.to_radians();
    let a = 10.0 * 2f64.sqrt();
    let cone = create_cone(k, Point3::new(0.0, 0.0, -10.0), a + 10.0, 0.0, a + 10.0)?;
    let post = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0)?;
    let post = placed(k, post, [g.cos(), 0.0, -g.sin()], Y_AXIS, [0.0, 0.0, 0.0])?;
    boolean_union(k, cone, post)
}

/// Two pipes that touch at one point, joined, then cut at z = -2, which takes their touching
/// point away.
fn cylinders_touching_at_a_point(k: &mut Kernel) -> Result<Brep> {
    let a = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 40.0)?;
    let b = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 40.0)?;
    let a = placed(k, a, DOWN, Y_AXIS, [-20.0, 0.0, -10.0])?;
    let b = placed(k, b, X_AXIS, DOWN, [0.0, -20.0, -5.0])?;
    let both = boolean_union(k, a, b)?;
    let cut = cutter(k, [0.0, 0.0, -2.0], Z_AXIS)?;
    boolean_difference(k, both, cut)
}

/// A tee: a branch R 8 about z from z = 0 to 40 standing on a pipe R 20 about x.
fn tee(k: &mut Kernel) -> Result<Brep> {
    let pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 20.0, 100.0)?;
    let branch = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 8.0, 40.0)?;
    let pipe = placed(k, pipe, DOWN, Y_AXIS, [-50.0, 0.0, 0.0])?;
    boolean_union(k, pipe, branch)
}

/// The fork's eight cases, in its order.
fn cases() -> Vec<Case> {
    let h = 0.5f64.sqrt();
    let al = 10f64.to_radians();
    let (ta, za) = (al.tan(), 10.0 / al.tan());
    let rho = (za - 15.0) * ta / (1.0 + 0.3 * ta * h);
    let ze = (1.0 + (1.0 + 4.0 * 1.01 * 75.0f64).sqrt()) / (2.0 * 1.01);
    let g = 20f64.to_radians();
    let a = 10.0 * 2f64.sqrt();
    let u = [g.sin(), 0.0, g.cos()];
    let s = (a * g.cos() - (a * a * g.cos() * g.cos() - 100.0 * (2.0 * g).cos()).sqrt())
        / (2.0 * g).cos();
    let zq = -5.0 - 5.0 * 1f64.cos();
    let yq = (100.0 - (zq + 10.0) * (zq + 10.0)).sqrt();
    vec![
        Case {
            name: "coaxial_cones",
            build: coaxial_cones,
            points: vec![[0.0, 6.0, 8.0]],
            r: 2.0,
            want: 1681.969921328409,
            kind: RoundKind::Revolved,
            faces: vec![
                Quadric::Cone([0.0, 0.0, 20.0], DOWN, 0.5f64.atan()),
                Quadric::Cone([0.0, 0.0, 9.2], DOWN, 5f64.atan()),
            ],
        },
        Case {
            name: "cone_cylinder_sharing_a_ball",
            build: cone_cylinder_sharing_a_ball,
            points: vec![[s * u[0], 10.0, s * u[2]]],
            r: 1.0,
            want: 21765.707385442460,
            kind: RoundKind::Rational(6, 2),
            faces: vec![
                Quadric::Cone([0.0, 0.0, a], DOWN, PI / 4.0),
                Quadric::Cylinder([0.0, 0.0, 0.0], u, 10.0),
            ],
        },
        Case {
            name: "cylinders_touching_at_a_point",
            build: cylinders_touching_at_a_point,
            points: vec![[5.0 * 1f64.sin(), yq, zq], [-5.0 * 1f64.sin(), yq, zq]],
            r: 1.0,
            want: 13442.703130586538,
            kind: RoundKind::Rational(12, 2),
            faces: vec![
                Quadric::Cylinder([0.0, 0.0, -10.0], X_AXIS, 10.0),
                Quadric::Cylinder([0.0, 0.0, -5.0], Y_AXIS, 5.0),
            ],
        },
        Case {
            name: "oblique_plane_cone",
            build: oblique_plane_cone,
            points: vec![[rho * h, rho * h, 15.0 + 0.3 * rho * h]],
            r: 1.5,
            want: 3549.917409442432,
            kind: RoundKind::Rational(4, 2),
            faces: vec![
                Quadric::Plane([0.0, 0.0, 15.0], oblique_cone_normal()),
                Quadric::Cone([0.0, 0.0, za], DOWN, al),
            ],
        },
        Case {
            name: "oblique_plane_cylinder",
            build: oblique_plane_cylinder,
            points: vec![[10.0 * h, 10.0 * h, 20.0 + 5.0 * h]],
            r: 2.0,
            want: 6214.410264258520,
            kind: RoundKind::Rational(4, 2),
            faces: vec![
                Quadric::Plane([0.0, 0.0, 20.0], oblique_cylinder_normal()),
                Quadric::Cylinder([0.0, 0.0, 0.0], Z_AXIS, 10.0),
            ],
        },
        Case {
            name: "sphere_coaxial_cone",
            build: sphere_coaxial_cone,
            points: vec![[0.0, 5.0 - ze / 10.0, ze]],
            r: 3.0,
            want: 4607.541826113212,
            kind: RoundKind::Revolved,
            faces: vec![
                Quadric::Sphere([0.0, 0.0, 0.0], 10.0),
                Quadric::Cone([0.0, 0.0, 50.0], DOWN, 0.1f64.atan()),
            ],
        },
        Case {
            name: "sphere_plane",
            build: sphere_plane,
            points: vec![[0.0, 84f64.sqrt(), 4.0]],
            r: 2.0,
            want: 3261.518762521006,
            kind: RoundKind::Revolved,
            faces: vec![
                Quadric::Plane([0.0, 0.0, 4.0], Z_AXIS),
                Quadric::Sphere([0.0, 0.0, 0.0], 10.0),
            ],
        },
        Case {
            name: "tee_is_fitted",
            build: tee,
            points: vec![[8.0 * h, 8.0 * h, 368f64.sqrt()]],
            r: 3.0,
            want: 129829.952905668950,
            kind: RoundKind::Fitted,
            faces: vec![
                Quadric::Cylinder([0.0, 0.0, 0.0], X_AXIS, 20.0),
                Quadric::Cylinder([0.0, 0.0, 0.0], Z_AXIS, 8.0),
            ],
        },
    ]
}

fn pick(k: &Kernel, b: &Brep, p: V3) -> Option<EdgeId> {
    let q = pt(p);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1
}

fn volume(k: &Kernel, b: &mut Brep) -> Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs)
}

/// The largest distance of the round surface's grid points from a ball touching the faces.
fn off_ball(
    k: &Kernel,
    s: &solidcore::surface::surface::Surface,
    r: f64,
    faces: &[Quadric],
) -> f64 {
    let Ok(d) = s.natural_uv_domain_with_nurb(k) else {
        return f64::INFINITY;
    };
    let mut worst = 0.0f64;
    for a in 0..=12 {
        for c in 0..=12 {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(a) / 12.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(c) / 12.0,
            );
            let Ok((x, du, dv)) = s.evaluate_1st_derivatives(k, uv, true, true) else {
                return f64::INFINITY;
            };
            let n = du.cross(dv);
            let len = n.length();
            if len == 0.0 {
                return f64::INFINITY;
            }
            let mut best = f64::INFINITY;
            for side in [1.0, -1.0] {
                let m = side * r / len;
                let centre = Point3::new(x.x + n.x * m, x.y + n.y * m, x.z + n.z * m);
                let off = faces
                    .iter()
                    .map(|f| (f.distance(centre) - r).abs())
                    .fold(0.0, f64::max);
                best = best.min(off);
            }
            worst = worst.max(best);
        }
    }
    worst
}

/// What grading a case found.
#[derive(Debug)]
enum Graded {
    /// The body before the round did not build.
    Body(String),
    /// The round returned an error.
    Round(String),
    /// The round built: what's wrong with it, and how far it lies off the ball.
    Built { problems: Vec<String>, off: f64 },
}

fn grade(case: &Case, k: &mut Kernel) -> Graded {
    let mut b = match (case.build)(k) {
        Ok(b) => b,
        Err(e) => return Graded::Body(format!("{e:?}")),
    };
    let fitted_tol = 100.0 * b.tolerance();
    let mut edges: Vec<EdgeId> = Vec::new();
    for &p in &case.points {
        let e = pick(k, &b, p).unwrap_or_else(|| panic!("{}: no edge near {p:?}", case.name));
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    if let Err(e) = fillet_edges(k, &mut b, &edges, case.r, 1, 1, 1.0) {
        return Graded::Round(format!("{e:?}"));
    }
    let v = volume(k, &mut b).unwrap();
    let mut problems = Vec::new();
    if (v - case.want).abs() > 1e-6 * case.want.abs() {
        problems.push(format!("volume {v:.9}, want {:.9}", case.want));
    }
    let lines = check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines();
    if !lines.is_empty() {
        problems.push(format!("{} failed validity tests", lines.len()));
    }
    let allowed = if case.kind == RoundKind::Fitted {
        fitted_tol
    } else {
        1e-9
    };
    let mut rounds = 0;
    let mut off = 0.0f64;
    let ids: Vec<_> = b.faces().ids().collect();
    for f in ids {
        let Some(s) = face::surface(&b, f).unwrap() else {
            continue;
        };
        let (spline, revolved, degrees, rational) = match s.kind() {
            SurfaceKind::BSpline(bs) => (
                true,
                false,
                (bs.degree(SurfParamType::U), bs.degree(SurfParamType::V)),
                bs.is_rational(),
            ),
            SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_) => {
                (false, true, (0, 0), false)
            }
            _ => continue,
        };
        rounds += 1;
        let kind_ok = match case.kind {
            RoundKind::Rational(p, q) => {
                spline && rational && (degrees == (p, q) || degrees == (q, p))
            }
            RoundKind::Revolved => revolved,
            RoundKind::Fitted => spline,
        };
        if !kind_ok {
            problems.push(format!(
                "round face of the wrong kind: degrees {degrees:?}, rational {rational}"
            ));
        }
        off = off.max(off_ball(k, s, case.r, &case.faces));
    }
    if rounds == 0 {
        problems.push("no round face".to_string());
    }
    if off > allowed {
        problems.push(format!("{off:.3e} off the ball (allowed {allowed:.3e})"));
    }
    Graded::Built { problems, off }
}

/// A mark of the fork's.
#[derive(Clone, Copy, Debug)]
enum Mark {
    Passes,
    /// Status 1001 from the round.
    Status1001,
    /// A polynomial (3, 3) B-spline, this far off the ball, where an exact round is wanted.
    Traced(f64),
    /// The kind is right, but it lies this far off the ball, over the tolerance.
    OverTolerance(f64),
}

/// The fork's figures have three digits.
fn about(got: f64, mark: f64) -> bool {
    (got - mark).abs() <= 0.01 * mark
}

fn holds(mark: Mark, got: &Graded) -> bool {
    match (mark, got) {
        (Mark::Passes, Graded::Built { problems, .. }) => problems.is_empty(),
        (Mark::Status1001, Graded::Round(e)) => e.contains("code: 1001"),
        (Mark::Traced(m), Graded::Built { problems, off }) => {
            about(*off, m)
                && problems
                    .iter()
                    .any(|p| p.contains("degrees (3, 3), rational false"))
        }
        (Mark::OverTolerance(m), Graded::Built { problems, off }) => {
            about(*off, m) && problems.len() == 1 && problems[0].contains("off the ball")
        }
        _ => false,
    }
}

/// The fork's mark at 099cf96, before the exact classes.
fn mark_before(name: &str) -> Mark {
    match name {
        "cone_cylinder_sharing_a_ball" => Mark::Traced(3.96e-4),
        "oblique_plane_cone" => Mark::Traced(9.4e-4),
        "oblique_plane_cylinder" => Mark::Traced(2.26e-3),
        "sphere_coaxial_cone" => Mark::Traced(1.47e-3),
        "cylinders_touching_at_a_point" | "sphere_plane" => Mark::Status1001,
        "tee_is_fitted" => Mark::OverTolerance(1.47e-3),
        _ => Mark::Passes,
    }
}

/// The fork's mark at 7ac037e (smlib#77 merged) and still at d01d7b0.
fn mark_after(name: &str) -> Mark {
    match name {
        "cylinders_touching_at_a_point" | "sphere_plane" => Mark::Status1001,
        "tee_is_fitted" => Mark::OverTolerance(1.47e-3),
        _ => Mark::Passes,
    }
}

const PIPES: &str = "cylinders_touching_at_a_point";

fn check_all(mark: fn(&str) -> Mark, kernel: fn() -> Kernel) {
    let mut wrong = Vec::new();
    for case in cases() {
        let mut k = kernel();
        if case.name == PIPES {
            k.fixes.ssi_split_curves_at_singular_points = true;
        }
        let got = grade(&case, &mut k);
        println!("{}: {got:?}", case.name);
        if !holds(mark(case.name), &got) {
            wrong.push(format!(
                "{}: want {:?}, got {got:?}",
                case.name,
                mark(case.name)
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// SolidCraft's fixes: every `UpstreamFixes` field on except `geometric_g2_continuity`.
fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn exact_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = true;
    k
}

#[test]
fn without_the_flag_the_cases_have_the_fork_s_marks_before_the_exact_classes() {
    check_all(mark_before, Kernel::default);
}

#[test]
fn with_solidcraft_s_fixes_the_cases_have_the_fork_s_marks_after_the_exact_classes() {
    check_all(mark_after, solidcraft_kernel);
}

#[test]
fn the_flag_alone_gives_the_fork_s_marks_after_the_exact_classes() {
    check_all(mark_after, exact_kernel);
}

#[test]
fn the_pipes_join_only_with_wr77() {
    let case = cases().into_iter().find(|c| c.name == PIPES).unwrap();
    let got = grade(&case, &mut Kernel::default());
    assert!(
        matches!(&got, Graded::Body(e) if e.contains("code: 1001")),
        "{got:?}"
    );
}

/// The grading sees what it's for: on a case that passes, a volume one part in 1e5 off and
/// each wrong surface kind are refused, and the fork's marks on it don't hold.
#[test]
fn the_grading_refuses_a_wrong_volume_and_a_wrong_kind() {
    let graded = |want_factor: f64, kind: RoundKind| {
        let mut case = cases()
            .into_iter()
            .find(|c| c.name == "coaxial_cones")
            .unwrap();
        case.want *= want_factor;
        case.kind = kind;
        grade(&case, &mut exact_kernel())
    };
    assert!(holds(Mark::Passes, &graded(1.0, RoundKind::Revolved)));
    for got in [
        graded(1.00001, RoundKind::Revolved),
        graded(1.0, RoundKind::Rational(4, 2)),
        graded(1.0, RoundKind::Fitted),
    ] {
        assert!(!holds(Mark::Passes, &got), "{got:?}");
        assert!(!holds(Mark::Status1001, &got), "{got:?}");
    }
}
