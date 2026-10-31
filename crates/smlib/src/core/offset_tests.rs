//! Shells, face offsets, thickening, sheet offsets and drafts on solidcore, against closed forms:
//! every result exports to a record that validates. With SMLib built, the same case on the C++
//! kernel gives the same volume (or, for a sheet, area), counts and faces by surface kind, or the
//! same error (parity only: the expected values are the analytic ones).
//!
//! The bodies are solidcore's own boxes, cylinders and linear sweeps (and booleans of them).

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::mpsc;
use std::time::Duration;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;

use super::offset::{draft, mark_moved, moved_faces, offset_faces, offset_sheet, shell, thicken};
use super::{Body, area, bounds, counts, make_box, make_cylinder, make_sphere, run, to_arrays, volume};
use crate::export::native;
use crate::{BooleanOp, Error};

/// A case longer than this is a hang.
const TIMEOUT: Duration = Duration::from_secs(300);

/// How near a pick point must be.
const TOL: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Box([f64; 3], [f64; 3]),
    Cylinder([f64; 3], f64, f64),
    Sphere([f64; 3], f64),
    /// A frustum about +Z: base centre, base radius, top radius, height.
    Frustum([f64; 3], f64, f64, f64),
    /// A sheet: the line from the first point to the second swept along `dir` by `dist`.
    LineSheet([f64; 3], [f64; 3], [f64; 3], f64),
    /// A sheet: the arc about +Z through `center` of `radius` from 0 to `degrees`, swept along
    /// +Z by `height`.
    ArcSheet([f64; 3], f64, f64, f64),
    /// Rig case shell/4711: a 60 x 40 x 10 block less an r3 through hole at (22.5, 7) and a
    /// 10 x 10 notch on the front edge from x = 25 to 35.
    Notched,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

impl Shape {
    fn core(self) -> Body {
        match self {
            Shape::Box(o, s) => make_box(o, s).unwrap(),
            Shape::Cylinder(o, r, h) => make_cylinder(o, r, h).unwrap(),
            Shape::Sphere(o, r) => make_sphere(o, r).unwrap(),
            Shape::Frustum(o, a, b, h) => super::make_cone(o, a, b, h).unwrap(),
            Shape::LineSheet(a, b, dir, dist) => run("sheet", |k| {
                let c = Curve::from(create_line_segment(v3(a), v3(b))?);
                create_linear_sweep(k, &[Some(&c)], v3(dir), dist, false)
            })
            .map(Body::new)
            .unwrap(),
            Shape::ArcSheet(o, r, degrees, h) => run("sheet", |k| {
                let frame = Axis2Placement::new(v3(o), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
                let c = Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&Extent1d::from_min_max(0.0, degrees)), None)?);
                create_linear_sweep(k, &[Some(&c)], Vec3::new(0.0, 0.0, 1.0), h, false)
            })
            .map(Body::new)
            .unwrap(),
            Shape::Notched => {
                let block = make_box([0.0; 3], [60.0, 40.0, 10.0]).unwrap();
                let pin = make_cylinder([22.5, 7.0, -5.0], 3.0, 20.0).unwrap();
                let notch = make_box([25.0, -5.0, -5.0], [10.0, 10.0, 20.0]).unwrap();
                let a = super::boolean::boolean(&block, &pin, BooleanOp::Subtract).unwrap().unwrap();
                super::boolean::boolean(&a, &notch, BooleanOp::Subtract).unwrap().unwrap()
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Op {
    /// `Brep::shell`: open faces, thickness.
    Shell(Shape, Vec<[f64; 3]>, f64),
    /// `Brep::offset_faces`: faces, distance.
    OffsetFaces(Shape, Vec<[f64; 3]>, f64),
    /// `Brep::thicken`: thickness, symmetric.
    Thicken(Shape, f64, bool),
    /// `Brep::offset_sheet`: distance.
    OffsetSheet(Shape, f64),
    /// `Brep::draft`: faces, neutral origin, neutral normal, pull, angle.
    Draft(Shape, Vec<[f64; 3]>, [f64; 3], [f64; 3], [f64; 3], f64),
}

/// What a case must give.
#[derive(Clone, Debug)]
enum Expect {
    /// A solid of this volume (relative 1e-9).
    Volume(f64),
    /// A solid of this volume whose bounds along z are these.
    Slab(f64, f64, f64),
    /// A solid of this volume (relative 1e-9) whose bounds are these.
    Solid(f64, [f64; 3], [f64; 3]),
    /// A sheet of this area (relative 1e-9) whose bounds are these.
    Sheet(f64, [f64; 3], [f64; 3]),
    /// This error.
    Fails(Error),
}

struct Case {
    op: Op,
    expect: Expect,
}

/// A 30 x 20 x 10 block from the origin.
const BLOCK: Shape = Shape::Box([0.0; 3], [30.0, 20.0, 10.0]);
const ABC: f64 = 6000.0;
/// A point on its top face.
const TOP: [f64; 3] = [15.0, 10.0, 10.0];
/// A point on its front face (y = 0).
const FRONT: [f64; 3] = [15.0, 0.0, 5.0];
/// The z axis: a neutral plane's normal and a pull.
const Z: [f64; 3] = [0.0, 0.0, 1.0];
/// A 30 x 20 sheet in z = 0: the x axis from 0 to 30 swept 20 along y.
const PLATE: Shape = Shape::LineSheet([0.0; 3], [30.0, 0.0, 0.0], [0.0, 1.0, 0.0], 20.0);
/// A sphere of radius 10 about the origin.
const BALL: Shape = Shape::Sphere([0.0; 3], 10.0);
/// A quarter of a cylinder of radius 5 about the z axis, 10 high.
const QUARTER: Shape = Shape::ArcSheet([0.0; 3], 5.0, 90.0, 10.0);

fn case(name: &str) -> Case {
    let c = |op, expect| Case { op, expect };
    match name {
        // abc - (a - 2t)(b - 2t)(c - t), t = 2.
        "shell_box_open_top" => c(Op::Shell(BLOCK, vec![TOP], -2.0), Expect::Volume(ABC - 26.0 * 16.0 * 8.0)),
        // A void: abc - (a - 2t)(b - 2t)(c - 2t).
        "shell_box_closed" => c(Op::Shell(BLOCK, vec![], -2.0), Expect::Volume(ABC - 26.0 * 16.0 * 6.0)),
        // Closed and outward, the convex edges extended: (a + 2t)(b + 2t)(c + 2t) - abc, t = 1.
        "shell_box_closed_outward" => c(Op::Shell(BLOCK, vec![], 1.0), Expect::Volume(32.0 * 22.0 * 12.0 - ABC)),
        // Outward, the convex edges extended: (a + 2t)(b + 2t)(c + t) - abc, t = 1.
        "shell_box_outward" => c(Op::Shell(BLOCK, vec![TOP], 1.0), Expect::Volume(32.0 * 22.0 * 11.0 - ABC)),
        // r = 5, h = 10, open at the top, t = 1: π r² h - π (r - t)² (h - t).
        "shell_cylinder_open_top" => {
            c(Op::Shell(Shape::Cylinder([0.0; 3], 5.0, 10.0), vec![[0.0, 2.0, 10.0]], -1.0), Expect::Volume(PI * 25.0 * 10.0 - PI * 16.0 * 9.0))
        }
        // A wall of 15 inside a block 20 wide leaves no cavity.
        "shell_too_thick" => c(Op::Shell(BLOCK, vec![TOP], -15.0), Expect::Fails(Error::NoRoom)),
        // The hole's offset (r 3 + 2) is tangent to the front's (y = 2): the walls meet along a
        // line, which bounds no solid.
        "shell_walls_touch" => c(Op::Shell(Shape::Notched, vec![[50.0, 30.0, 0.0]], -2.0), Expect::Fails(Error::WallsTouch)),
        "shell_pick_near_nothing" => c(Op::Shell(BLOCK, vec![TOP, [100.0; 3]], -2.0), Expect::Fails(Error::NoMatch { point: 1 })),
        // The top front edge's midpoint is as near the top as the front.
        "shell_pick_ambiguous" => c(Op::Shell(BLOCK, vec![[15.0, 0.0, 10.0]], -2.0), Expect::Fails(Error::Ambiguous { point: 0 })),
        // A plate of 30 x 20 thickened 2: 1200, 2 thick along z; symmetric, from -1 to 1.
        "thicken_plate" => c(Op::Thicken(PLATE, 2.0, false), Expect::Slab(1200.0, 0.0, 2.0)),
        "thicken_plate_symmetric" => c(Op::Thicken(PLATE, 2.0, true), Expect::Slab(1200.0, -1.0, 1.0)),
        "thicken_a_solid" => c(Op::Thicken(BLOCK, 2.0, false), Expect::Fails(Error::Argument)),
        // The plate moved 3 along its normal (+z: the x axis swept along y): still 600, at z = 3.
        "offset_sheet_plate" => c(Op::OffsetSheet(PLATE, 3.0), Expect::Sheet(600.0, [0.0, 0.0, 3.0], [30.0, 20.0, 3.0])),
        // A sphere's one face closes on itself (tangency-labs/smlib fix #17): r = 10 moved out 2 is
        // the ball of radius 12, (4/3) π 12³; moved in 10, to its centre, nothing is left.
        "offset_sheet_a_sphere" => c(Op::OffsetSheet(BALL, 2.0), Expect::Volume(4.0 / 3.0 * PI * 12.0f64.powi(3))),
        "offset_sheet_a_sphere_to_its_centre" => c(Op::OffsetSheet(BALL, -10.0), Expect::Fails(Error::Empty)),
        // The quarter cylinder moved 2 along its normal (outward: the arc's tangent swept along
        // z): radius 7, area (π/2) 7 · 10.
        "offset_sheet_quarter_cylinder" => c(Op::OffsetSheet(QUARTER, 2.0), Expect::Sheet(PI / 2.0 * 7.0 * 10.0, [0.0, 0.0, 0.0], [7.0, 7.0, 10.0])),
        // A closed sheet offsets as an open one (tangency-labs/smlib fix #17, develop 9d0fb564):
        // each face moved out 1 along its normal, the convex edges extended, gives the block
        // (a + 2d)(b + 2d)(c + 2d) of six planes, from -d to (a, b, c) + d.
        "offset_sheet_a_solid" => c(Op::OffsetSheet(BLOCK, 1.0), Expect::Solid(32.0 * 22.0 * 12.0, [-1.0, -1.0, -1.0], [31.0, 21.0, 11.0])),
        // The top moved out 2: abc + d·ab.
        "offset_faces_top_out" => c(Op::OffsetFaces(BLOCK, vec![TOP], 2.0), Expect::Volume(ABC + 2.0 * 600.0)),
        // The top moved in 10, as deep as the block: nothing left behind it.
        "offset_faces_through" => c(Op::OffsetFaces(BLOCK, vec![TOP], -10.0), Expect::Fails(Error::NoRoom)),
        // The four sides drafted θ = 5° about the bottom (z = 0), pulled along +z, leaning in:
        // the section at height z is (a - 2 z tanθ)(b - 2 z tanθ), so
        // V = abh - (a + b) h² tanθ + (4/3) h³ tan²θ.
        "draft_box_sides" => {
            let s = (5.0f64).to_radians().tan();
            let (a, b, h) = (30.0, 20.0, 10.0);
            let sides = vec![[15.0, 0.0, 5.0], [30.0, 10.0, 5.0], [15.0, 20.0, 5.0], [0.0, 10.0, 5.0]];
            c(
                Op::Draft(BLOCK, sides, [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 5f64.to_radians()),
                Expect::Volume(a * b * h - (a + b) * h * h * s + 4.0 / 3.0 * h * h * h * s * s),
            )
        }
        // The front (y = 0) alone drafted θ = 5° about the bottom: a wedge of a h² tanθ / 2
        // comes off along its length.
        "draft_one_side" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0; 3], Z, Z, 5f64.to_radians()), Expect::Volume(ABC - 30.0 * 100.0 * s / 2.0))
        }
        // A negative angle leans it out: the same wedge goes on.
        "draft_one_side_out" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0; 3], Z, Z, -5f64.to_radians()), Expect::Volume(ABC + 30.0 * 100.0 * s / 2.0))
        }
        // Hinged at the top (z = 10) and leaning in going up, the face leans out below the
        // hinge: the wedge goes on.
        "draft_one_side_hinged_at_the_top" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0, 0.0, 10.0], Z, Z, 5f64.to_radians()), Expect::Volume(ABC + 30.0 * 100.0 * s / 2.0))
        }
        // A cylinder along the pull drafts to a cone (tangency-labs/smlib fix #27, develop
        // 9d0fb564): R = 5, h = 10, θ = 0.1 about the bottom, leaning in: a frustum of radii R
        // and r = R - h tanθ (a cone and its two caps): V = π h (R² + R r + r²) / 3.
        "draft_a_curved_face" => {
            let (big, h) = (5.0, 10.0);
            let small = big - h * 0.1f64.tan();
            c(
                Op::Draft(Shape::Cylinder([0.0; 3], big, h), vec![[5.0, 0.0, 5.0]], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.1),
                Expect::Volume(PI * h * (big * big + big * small + small * small) / 3.0),
            )
        }
        // A cone r 10 at its base to 8 at its top, 20 tall, drafted 5° about its base turns its
        // generators 5° more (fork d51a723): r 10 + 20 tan(atan(-0.1) - 5°) at the top.
        "draft_a_cone_about_the_pull" => {
            let (big, h, a) = (10.0, 20.0, 5f64.to_radians());
            let small = big + h * ((-0.1f64).atan() - a).tan();
            c(
                Op::Draft(Shape::Frustum([0.0; 3], big, 8.0, h), vec![[0.0, 9.0, 10.0]], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], a),
                Expect::Volume(PI * h * (big * big + big * small + small * small) / 3.0),
            )
        }
        // The top is square to the pull, not along it.
        "draft_a_face_across_the_pull" => {
            c(Op::Draft(BLOCK, vec![TOP], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.1), Expect::Fails(Error::Unsupported))
        }
        _ => panic!("{name}"),
    }
}

/// The tolerance a case picks with: the ambiguous point lies on its two faces.
fn tolerance(name: &str) -> f64 {
    if name == "shell_pick_ambiguous" { 1.0 } else { TOL }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

#[derive(Debug, PartialEq)]
struct Measured {
    /// The volume of a solid, the area of a sheet.
    size: f64,
    bounds: ([f64; 3], [f64; 3]),
    counts: [usize; 3],
    kinds: BTreeMap<String, usize>,
}

fn kinds(rec: &solidcraft_geom::brep::Brep) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in &rec.faces {
        let v = serde_json::to_value(&f.surface).unwrap();
        *out.entry(v["kind"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
    }
    out
}

fn is_sheet(e: &Expect) -> bool {
    matches!(e, Expect::Sheet(..))
}

fn on_core(name: &str, c: &Case) -> crate::Result<Body> {
    let tol = tolerance(name);
    match &c.op {
        Op::Shell(s, points, t) => shell(&s.core(), points, tol, *t),
        Op::OffsetFaces(s, points, d) => offset_faces(&s.core(), points, tol, *d),
        Op::Thicken(s, t, sym) => thicken(&s.core(), *t, *sym),
        Op::OffsetSheet(s, d) => offset_sheet(&s.core(), *d),
        Op::Draft(s, points, o, n, pull, angle) => draft(&s.core(), points, tol, *o, *n, *pull, *angle),
    }
}

/// Runs `f` on its own thread: a hang fails the test instead of stopping the run.
fn timed<T: Send + 'static>(name: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(p) => std::panic::resume_unwind(p),
            Ok(()) => panic!("{name}: no result"),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{name}: solidcore hung (no result after {TIMEOUT:?})"),
    }
}

type Got = std::result::Result<Measured, Error>;

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// Checks a kernel's result against the case's closed form; what it measured, or its error.
fn judge(name: &str, who: &str, expect: &Expect, got: Got) -> Got {
    match (expect, &got) {
        (Expect::Fails(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Expect::Fails(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::Volume(v), Ok(m)) => assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size),
        (Expect::Slab(v, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size);
            let (a, b) = (m.bounds.0[2], m.bounds.1[2]);
            assert!((a - lo).abs() < 1e-6 && (b - hi).abs() < 1e-6, "{name}: {who} z from {a} to {b}, expected {lo} to {hi}");
        }
        (Expect::Solid(v, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size);
            assert!(near(m.bounds.0, *lo) && near(m.bounds.1, *hi), "{name}: {who} bounds {:?}, expected {lo:?} {hi:?}", m.bounds);
        }
        (Expect::Sheet(s, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *s) < 1e-9, "{name}: {who} area {} vs analytic {s}", m.size);
            assert!(near(m.bounds.0, *lo) && near(m.bounds.1, *hi), "{name}: {who} bounds {:?}, expected {lo:?} {hi:?}", m.bounds);
        }
    }
    got
}

fn solidcore_case(name: &'static str) -> Got {
    timed(name, move || {
        let c = case(name);
        let sheet = is_sheet(&c.expect);
        let got = on_core(name, &c).map(|body| {
            let size = if sheet { area(&body, 1e-8) } else { volume(&body, 1e-8) }.unwrap_or_else(|e| panic!("{name}: size: {e}"));
            let n = counts(&body).unwrap();
            let (rec, _) = native(&to_arrays(&body).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
            rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
            let bx = bounds(&body).unwrap();
            Measured { size, bounds: bx, counts: n, kinds: kinds(&rec) }
        });
        judge(name, "solidcore", &c.expect, got)
    })
}

/// The case on the C++ kernel, called directly (whatever the routing).
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Got {
    use crate::ops::pick_error;
    use crate::sweeps::DraftCorner;
    use crate::{Brep, error, ffi};
    crate::init().unwrap();
    let line = |a: [f64; 3], b: [f64; 3]| crate::sweeps::Curve::Line { start: a, end: b };
    let make = |s: Shape| -> Brep {
        match s {
            Shape::Box(o, z) => ffi::make_box(&o, &z).map(Brep::made).unwrap(),
            Shape::Cylinder(o, r, h) => ffi::make_cylinder(&o, r, h).map(Brep::made).unwrap(),
            Shape::Sphere(o, r) => ffi::make_sphere(&o, r).map(Brep::made).unwrap(),
            Shape::Frustum(o, a, b, h) => ffi::make_cone(&o, a, b, h).map(Brep::made).unwrap(),
            Shape::LineSheet(a, b, dir, dist) => Brep::extrude(&[line(a, b)], dir, dist, 0.0, DraftCorner::default(), false).unwrap(),
            Shape::ArcSheet(o, r, degrees, h) => {
                let arc = crate::sweeps::Curve::Arc {
                    center: o,
                    axis: [0.0, 0.0, 1.0],
                    reference: [1.0, 0.0, 0.0],
                    radius: r,
                    start: 0.0,
                    end: degrees.to_radians(),
                };
                Brep::extrude(&[arc], [0.0, 0.0, 1.0], h, 0.0, DraftCorner::default(), false).unwrap()
            }
            Shape::Notched => {
                let block = ffi::make_box(&[0.0; 3], &[60.0, 40.0, 10.0]).unwrap();
                let pin = ffi::make_cylinder(&[22.5, 7.0, -5.0], 3.0, 20.0).unwrap();
                let notch = ffi::make_box(&[25.0, -5.0, -5.0], &[10.0, 10.0, 20.0]).unwrap();
                let a = ffi::ops::boolean(&block, &pin, ffi::ops::BOOLEAN_SUBTRACT).unwrap();
                Brep::made(ffi::ops::boolean(&a, &notch, ffi::ops::BOOLEAN_SUBTRACT).unwrap())
            }
        }
    };
    let c = case(name);
    let tol = tolerance(name);
    let r = match &c.op {
        Op::Shell(s, points, t) => ffi::ops::pick_op(ffi::ops::Pick::Shell, make(*s).cpp().unwrap(), points, tol, *t).map_err(pick_error),
        Op::OffsetFaces(s, points, d) => ffi::ops::pick_op(ffi::ops::Pick::OffsetFaces, make(*s).cpp().unwrap(), points, tol, *d).map_err(pick_error),
        Op::Thicken(s, t, sym) => ffi::ops::thicken(make(*s).cpp().unwrap(), *t, *sym).map_err(error),
        Op::OffsetSheet(s, d) => ffi::sheets::offset_sheet(make(*s).cpp().unwrap(), *d).map_err(error),
        Op::Draft(s, points, o, n, pull, angle) => ffi::ops::draft(make(*s).cpp().unwrap(), points, tol, o, n, pull, *angle).map_err(pick_error),
    };
    let sheet = is_sheet(&c.expect);
    let got = r.map(|h| {
        let size = if sheet { ffi::measure::area(&h, 1e-8) } else { ffi::volume(&h, 1e-8) }.unwrap();
        let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
        let bx = ffi::measure::bounds(&h).unwrap();
        let b = Brep::made(h);
        let (rec, _) = native(&b.cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        Measured { size, bounds: bx, counts: n, kinds: kinds(&rec) }
    });
    judge(name, "C++", &c.expect, got)
}

/// solidcore and C++ agree: volume (area) within 1e-9, the same counts and faces by kind, or the
/// same error.
#[cfg(feature = "native")]
fn parity(name: &'static str) {
    let cpp = cpp_case(name);
    let ours = solidcore_case(name);
    match (ours, cpp) {
        (Ok(a), Ok(b)) => {
            assert!(rel(a.size, b.size) < 1e-9, "{name}: solidcore {} vs C++ {}", a.size, b.size);
            assert_eq!(a.counts, b.counts, "{name}: face, edge, vertex counts (solidcore vs C++)");
            assert_eq!(a.kinds, b.kinds, "{name}: faces by surface kind (solidcore vs C++)");
        }
        (a, b) => assert_eq!(a.err(), b.err(), "{name}: errors (solidcore vs C++)"),
    }
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_closed_form() {
                let _ = super::solidcore_case(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                super::parity(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            fn the_cpp_kernel_matches_the_closed_form() {
                let _cpp = crate::route::on_cpp();
                let _ = super::cpp_case(stringify!($m));
            }
        }
    )*};
}

cases!(
    shell_box_open_top,
    shell_box_closed,
    shell_box_closed_outward,
    shell_box_outward,
    shell_cylinder_open_top,
    shell_too_thick,
    shell_walls_touch,
    shell_pick_near_nothing,
    shell_pick_ambiguous,
    thicken_plate,
    thicken_plate_symmetric,
    thicken_a_solid,
    offset_sheet_plate,
    offset_sheet_quarter_cylinder,
    offset_sheet_a_solid,
    offset_sheet_a_sphere,
    offset_sheet_a_sphere_to_its_centre,
    offset_faces_top_out,
    offset_faces_through,
    draft_box_sides,
    draft_one_side,
    draft_one_side_out,
    draft_one_side_hinged_at_the_top,
    draft_a_curved_face,
    draft_a_cone_about_the_pull,
    draft_a_face_across_the_pull,
);

/// The capi's argument checks, before any kernel call.
#[test]
fn hostile_arguments_are_refused() {
    let b = BLOCK.core();
    for t in [0.0, f64::NAN, -1e7, f64::INFINITY] {
        assert_eq!(shell(&b, &[TOP], TOL, t).err(), Some(Error::Argument), "thickness {t}");
        assert_eq!(thicken(&b, t, false).err(), Some(Error::Argument), "thickness {t}");
        assert_eq!(offset_sheet(&b, t).err(), Some(Error::Argument), "distance {t}");
        assert_eq!(offset_faces(&b, &[TOP], TOL, t).err(), Some(Error::Argument), "distance {t}");
    }
    assert_eq!(shell(&b, &[[f64::NAN; 3]], TOL, -1.0).err(), Some(Error::Argument));
    assert_eq!(shell(&b, &[TOP], 0.0, -1.0).err(), Some(Error::Argument));
    assert_eq!(offset_faces(&b, &[], TOL, 1.0).err(), Some(Error::Argument));
    let z = [0.0, 0.0, 1.0];
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], [0.0; 3], z, 0.1).err(), Some(Error::Argument), "no neutral normal");
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], z, z, 0.0).err(), Some(Error::Argument), "no angle");
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], z, z, PI / 2.0).err(), Some(Error::Argument), "a right angle");
    assert_eq!(draft(&b, &[TOP], TOL, [f64::NAN; 3], z, z, 0.1).err(), Some(Error::Argument));
}

/// The body offset is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let r = timed("operand", || {
        let b = BLOCK.core();
        let _ = shell(&b, &[TOP], TOL, -2.0);
        (counts(&b).unwrap(), volume(&b, 1e-8).unwrap())
    });
    assert_eq!(r.0, [6, 12, 8]);
    assert!(rel(r.1, ABC) < 1e-12, "{}", r.1);
}

/// The distance from `p` to each face of `b` (`SmApiFaceClosestPoint`), under `identity`.
fn face_distances(
    k: &mut solidcore::kernel::Kernel,
    b: &mut solidcore::topology::Brep,
    identity: &solidcore::query::brep::BrepIdentity<'_>,
    p: Vec3,
) -> solidcore::Result<Vec<(f64, Vec3)>> {
    use solidcore::geom::object::Shape as Sh;
    use solidcore::query::api_queries::face_closest_point_in_shape;
    use solidcore::query::brep::LiveBrepObjects;
    use solidcore::topology::BrepId;
    let one = BrepId::from_index(0);
    let mut out = Vec::new();
    for f in solidcore::topology::brep::p1::faces(b, None)? {
        let shape = Sh::new(k, &(one, &*b), &[], &[(one, f)], &[], &[])?;
        let objects = LiveBrepObjects::new(b, identity, None)?;
        let c = face_closest_point_in_shape(k, &shape, &objects, (one, f), p)?;
        out.push((c.distance, c.point));
    }
    Ok(out)
}

/// The closed form for (15, 10, 20) above the 30 x 20 x 10 box at the origin, face by face of
/// `create_box`: bottom 20, top 10, the x sides sqrt(15^2 + 10^2), the y sides sqrt(10^2 + 10^2).
fn box_face_distances_are_exact(what: &str, d: &[(f64, Vec3)]) {
    let want = [20.0, 10.0, 325f64.sqrt(), 325f64.sqrt(), 200f64.sqrt(), 200f64.sqrt()];
    assert_eq!(d.len(), want.len(), "{what}: faces");
    for (i, (&(got, _), want)) in d.iter().zip(want).enumerate() {
        assert!((got - want).abs() < 1e-9, "{what}: face {i} at {got}, want {want}");
    }
    let top = d[1].1;
    assert!((top.x - 15.0).abs() < 1e-9 && (top.y - 10.0).abs() < 1e-9 && (top.z - 10.0).abs() < 1e-9, "{what}: top {top:?}");
}

/// Picking faces on a solidcore box: on the body itself, on the working copy every operation
/// makes (`copy_of`), and by solidcore's recommended usage, each face's distance is the closed
/// form and the top face is picked. The copy names its geometry owners in its own key domain
/// with no naming scheme of its own, so the body's identities (`BrepIdentity::of_brep`: the
/// arena's) name none of them; the copy's proven owners (`BrepQuery::effective_identity`) do
/// (solidcore#1440 was this). Under the arena's identities the solve fails, and the failure is
/// an error, not a skipped face.
#[test]
fn faces_are_picked_on_the_box_and_on_its_working_copy() {
    use solidcore::SolidError;
    use solidcore::query::brep::{BrepIdentity, BrepQuery};
    let p = Vec3::new(15.0, 10.0, 20.0);
    let on_top = [[15.0, 10.0, 10.0]];
    run("pick", |k| {
        let mut b = solidcore::primitive::api_primitives::create_box(k, Vec3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0)?;
        let mut w = super::boolean::copy_of(k, &b, 1)?;
        let mut faces = Vec::new();
        for (what, x) in [("the body", &mut b), ("its copy", &mut w)] {
            let query = BrepQuery::of_brep(x);
            let identity = query.effective_identity();
            box_face_distances_are_exact(what, &face_distances(k, x, &identity, p)?);
            let picked = super::offset::pick_faces(k, x, &on_top, TOL)?;
            faces.push(picked.ok());
        }
        assert!(faces[0].as_ref().is_some_and(|f| f.len() == 1), "the body: {faces:?}");
        assert_eq!(faces[0], faces[1], "the body and its copy pick the same face");
        // `BrepIdentity::of_brep` honours a copy's owner keys since solidcore#1445: the body
        // and its copy both resolve.
        for (what, x) in [("of_brep on the body", &mut b), ("of_brep on its copy", &mut w)] {
            let proven = BrepIdentity::of_brep(x);
            box_face_distances_are_exact(what, &face_distances(k, x, &proven.identity(), p)?);
        }
        // The copy under its naming scheme alone (the arena's identities): no face's geometry
        // owner resolves, and the query says so rather than answering from the vertices. Since
        // solidcore#1452 that is a port-side invariant, not an SMLib status.
        let arena = BrepIdentity::of_naming_with_id(solidcore::topology::BrepId::from_index(0), &w);
        match face_distances(k, &mut w, &arena, p) {
            Err(e @ SolidError::PortInvariant { .. }) if e.sm_status().is_none() => {}
            other => panic!("the copy under the arena's identities: {other:?}"),
        }
        Ok(())
    })
    .unwrap();
}

/// A pick skips an entity whose query returns an SMLib status, as the capi skips any status
/// other than `SM_SUCCESS`; any other solidcore error fails the pick.
#[test]
fn a_solidcore_error_in_a_pick_is_an_error() {
    use super::answer;
    use super::blend::pick;
    use solidcore::SolidError;
    let all = [0usize, 1];
    let points = [[0.0; 3]];
    let status = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::sm_err("q")) } else { Ok(0.5) }));
    assert_eq!(status.unwrap().unwrap().0, vec![1], "a status skips the entity");
    let other = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::invalid_input("q", "owner")) } else { Ok(0.5) }));
    assert!(matches!(other, Err(SolidError::InvalidInput { .. })), "{other:?}");
    let unported = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::unported("Sm::X", "u")) } else { Ok(0.5) }));
    assert!(matches!(unported, Err(SolidError::Unported { .. })), "{unported:?}");
}

/// The capi's `moved_faces` reading: no mark, no move (SMLIB_ERR_NO_EFFECT); a mark is read once,
/// and gone.
#[test]
fn a_draft_with_no_marked_face_left_has_no_effect() {
    let body = make_box([0.0; 3], [10.0; 3]).unwrap();
    super::with_body("marks", &body, |_, w| {
        assert!(!moved_faces(w)?, "no face marked");
        let f = w.face_list()[0];
        mark_moved(w, &[f])?;
        assert!(moved_faces(w)?, "a face marked");
        assert!(!moved_faces(w)?, "the mark removed");
        Ok(())
    })
    .unwrap();
}

/// A draft leaves no mark of its own on what it returns, on a plane or on a cylinder turned cone.
#[test]
fn a_draft_leaves_no_mark() {
    let z = [0.0, 0.0, 1.0];
    let bx = make_box([0.0; 3], [20.0, 30.0, 10.0]).unwrap();
    let plane = draft(&bx, &[[10.0, 0.0, 5.0]], TOL, [0.0; 3], z, z, 0.1).unwrap();
    let cyl = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let cone = draft(&cyl, &[[5.0, 0.0, 5.0]], TOL, [0.0; 3], z, z, 0.1).unwrap();
    for b in [plane, cone] {
        super::with_body("marks", &b, |_, w| {
            assert!(!moved_faces(w)?);
            Ok(())
        })
        .unwrap();
    }
}

/// `moved_faces` on both kernels: drafts so small they leave a plane or cylinder where it was
/// still moved a face (SMLib's mark and solidcore's both stay on it), an angle that brings a wall
/// onto the plane of the wall above it leaves the two faces apart, and no draft here is
/// SMLIB_ERR_NO_EFFECT on either: the same answer, volume and counts. The C++ check holds a local
/// operation to what it promised and no body found so far trips it.
#[cfg(feature = "native")]
#[test]
fn a_draft_moves_a_face_as_on_cpp() {
    let _cpp = crate::route::on_cpp();
    use crate::sweeps::{Curve, DraftCorner};
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let z = [0.0, 0.0, 1.0];
    let make = |h: ffi::Handle| Brep::made(h);
    let plate = make(ffi::make_box(&[0.0; 3], &[40.0, 40.0, 10.0]).unwrap());
    let rod = make(ffi::make_cylinder(&[20.0, 20.0, -5.0], 5.0, 30.0).unwrap());
    let holed = make(ffi::ops::boolean(plate.cpp().unwrap(), rod.cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap());
    let pinned = make(ffi::ops::boolean(plate.cpp().unwrap(), rod.cpp().unwrap(), ffi::ops::BOOLEAN_UNION).unwrap());
    let t = 5f64.to_radians().tan();
    // A wall x = 20 up to z = 10, then a wall leaning in 5 degrees: the lower wall turned 5
    // degrees about z = 10 lies on the upper one's plane.
    let p = [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 0.0, 10.0], [20.0 - 10.0 * t, 0.0, 20.0], [0.0, 0.0, 20.0]];
    let profile: Vec<Curve> = (0..5).map(|i| Curve::Line { start: p[i], end: p[(i + 1) % 5] }).collect();
    crate::sweeps::ON_CPP.with(|c| c.set(true));
    let leaning = Brep::extrude(&profile, [0.0, 1.0, 0.0], 10.0, 0.0, DraftCorner::default(), true).unwrap();
    crate::sweeps::ON_CPP.with(|c| c.set(false));
    let five = 5f64.to_radians();
    let cases: [(&str, &Brep, [f64; 3], [f64; 3], f64); 7] = [
        ("tiny on a plane", &plate, [20.0, 0.0, 5.0], [0.0; 3], 1e-14),
        ("tiny on a hole", &holed, [25.0, 20.0, 5.0], [0.0, 0.0, 5.0], 1e-14),
        ("tiny on a pin", &pinned, [25.0, 20.0, 20.0], [0.0, 0.0, 5.0], -1e-14),
        ("small on a hole", &holed, [25.0, 20.0, 5.0], [0.0, 0.0, 5.0], 1e-6),
        ("onto the wall above", &leaning, [20.0, 5.0, 5.0], [0.0, 0.0, 10.0], five),
        ("away from the wall above", &leaning, [20.0, 5.0, 5.0], [0.0, 0.0, 10.0], -five),
        ("hinged far off", &plate, [20.0, 0.0, 5.0], [0.0, 0.0, 1000.0], five),
    ];
    for (name, b, at, origin, angle) in cases {
        let cpp = ffi::ops::draft(b.cpp().unwrap(), &[at], TOL, &origin, &z, &z, angle).unwrap();
        let (v, n) = (ffi::volume(&cpp, 1e-8).unwrap(), ffi::counts(&cpp).unwrap().map(|x| usize::try_from(x).unwrap()));
        let core = draft(b.core().unwrap(), &[at], TOL, origin, z, z, angle).unwrap();
        let w = volume(&core, 1e-8).unwrap();
        assert!(rel(w, v) < 1e-9, "{name}: solidcore {w} vs C++ {v}");
        assert_eq!(counts(&core).unwrap(), n, "{name}");
    }
}

/// The tip cut off a triangular prism, drafted about a plane 100 mm off so that the whole face goes
/// out past where its neighbours meet: its edges vanish, `SM_ERR_OFFSET_EDGE_VANISHES` in SMLib,
/// which the capi's `status` answers as `SMLIB_ERR_UNSUPPORTED`, on both kernels.
#[cfg(feature = "native")]
#[test]
fn a_draft_past_the_neighbours_meeting_is_unsupported_as_on_cpp() {
    let _cpp = crate::route::on_cpp();
    use crate::sweeps::{Curve, DraftCorner};
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let p = [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [11.0, 18.0, 0.0], [9.0, 18.0, 0.0]];
    let profile: Vec<Curve> = (0..4).map(|i| Curve::Line { start: p[i], end: p[(i + 1) % 4] }).collect();
    crate::sweeps::ON_CPP.with(|c| c.set(true));
    let prism = Brep::extrude(&profile, [0.0, 0.0, 1.0], 10.0, 0.0, DraftCorner::default(), true).unwrap();
    crate::sweeps::ON_CPP.with(|c| c.set(false));
    let (tip, z, origin, angle) = ([[10.0, 18.0, 5.0]], [0.0, 0.0, 1.0], [0.0, 0.0, -100.0], (-2f64).to_radians());
    let cpp = ffi::ops::draft(prism.cpp().unwrap(), &tip, TOL, &origin, &z, &z, angle);
    assert_eq!(cpp.err(), Some((ffi::ops::ERR_UNSUPPORTED, None)));
    assert_eq!(draft(prism.core().unwrap(), &tip, TOL, origin, z, z, angle).err(), Some(Error::Unsupported));
}

/// A flat L of two faces thickened by 2 with mitred walls: 2 x 10 x (20 + 22), as the C++ kernel
/// makes it.
#[cfg(feature = "native")]
#[test]
fn an_l_sheet_thickens_as_on_cpp() {
    let _cpp = crate::route::on_cpp();
    use crate::sweeps::{Curve, DraftCorner};
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let l = [Curve::Line { start: [0.0; 3], end: [20.0, 0.0, 0.0] }, Curve::Line { start: [20.0, 0.0, 0.0], end: [20.0, 20.0, 0.0] }];
    crate::sweeps::ON_CPP.with(|c| c.set(true));
    let sheet = Brep::extrude(&l, [0.0, 0.0, 1.0], 10.0, 0.0, DraftCorner::default(), false).unwrap();
    crate::sweeps::ON_CPP.with(|c| c.set(false));
    let cpp = ffi::ops::thicken(sheet.cpp().unwrap(), 2.0, false).unwrap();
    assert!(rel(ffi::volume(&cpp, 1e-8).unwrap(), 840.0) < 1e-9);
    let core = thicken(sheet.core().unwrap(), 2.0, false).unwrap();
    assert!(rel(volume(&core, 1e-8).unwrap(), 840.0) < 1e-9);
}

/// Face and edge tags (`Brep::tagged`) through offsets routed to solidcore, as on C++: the box is
/// tagged on the C++ kernel, its solidcore body converted from that and tagged the same way, and
/// the two results' histories must be the same.
#[cfg(feature = "native")]
mod history {
    use crate::{Brep, ffi};

    fn tagged_box() -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_box(&[0.0; 3], &[20.0, 30.0, 10.0]).unwrap()).tagged(0).unwrap()
    }

    /// The box hollowed to 2 mm walls, its top open: the five walls offset inward stand for the
    /// faces they offset.
    #[test]
    fn tags_cross_a_shell() {
        let _cpp = crate::route::on_cpp();
        let a = tagged_box();
        let top = [[10.0, 15.0, 10.0]];
        let cpp = Brep::made(ffi::ops::pick_op(ffi::ops::Pick::Shell, a.cpp().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        let core = Brep::made_core(super::shell(a.core().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    /// The top moved down 2 mm: it stands for itself, its four neighbours are trimmed to meet it.
    #[test]
    fn tags_cross_a_face_offset() {
        let _cpp = crate::route::on_cpp();
        let a = tagged_box();
        let top = [[10.0, 15.0, 10.0]];
        let cpp = Brep::made(ffi::ops::pick_op(ffi::ops::Pick::OffsetFaces, a.cpp().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        let core = Brep::made_core(super::offset_faces(a.core().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    /// A flat sheet and a half-round open extrusion, tagged on the C++ kernel.
    fn tagged_sheet(curved: bool) -> Brep {
        use std::f64::consts::PI;

        use crate::sweeps::{Curve, DraftCorner};
        crate::init().unwrap();
        crate::sweeps::ON_CPP.with(|c| c.set(true));
        let b = if curved {
            let arc = [Curve::Arc { center: [0.0; 3], axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius: 10.0, start: 0.0, end: PI }];
            Brep::extrude(&arc, [0.0, 0.0, 1.0], 20.0, 0.0, DraftCorner::default(), false)
        } else {
            let c = [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 10.0, 0.0], [0.0, 10.0, 0.0]];
            let l: Vec<Curve> = (0..4).map(|i| Curve::Line { start: c[i], end: c[(i + 1) % 4] }).collect();
            Brep::planar_faces(&l)
        };
        crate::sweeps::ON_CPP.with(|c| c.set(false));
        b.unwrap().tagged(0).unwrap()
    }

    /// A sheet thickened to one side and to both: both faces stand for the sheet's (the histories
    /// `offset_backend` reads), its edges and their offsets for its edges, the walls for nothing.
    fn thicken_parity(curved: bool) {
        for symmetric in [false, true] {
            let a = tagged_sheet(curved);
            let cpp = Brep::made(ffi::ops::thicken(a.cpp().unwrap(), 2.0, symmetric).unwrap()).history().unwrap();
            let core = Brep::made_core(super::thicken(a.core().unwrap(), 2.0, symmetric).unwrap()).history().unwrap();
            assert_eq!(core.canonical(), cpp.canonical(), "symmetric {symmetric}");
        }
    }

    #[test]
    fn tags_cross_a_thicken_of_a_curved_sheet() {
        let _cpp = crate::route::on_cpp();
        thicken_parity(true);
    }

    #[test]
    #[ignore = "solidcore#1973 (PR #1977): the stitch in the shell drops the flat sheet's edge tags"]
    fn tags_cross_a_thicken_of_a_flat_sheet() {
        let _cpp = crate::route::on_cpp();
        thicken_parity(false);
    }

    /// A sheet moved along its normal, flat and curved: each face stands for itself.
    #[test]
    fn tags_cross_a_sheet_offset() {
        let _cpp = crate::route::on_cpp();
        for curved in [false, true] {
            let a = tagged_sheet(curved);
            let cpp = Brep::made(ffi::sheets::offset_sheet(a.cpp().unwrap(), 1.5).unwrap()).history().unwrap();
            let core = Brep::made_core(super::offset_sheet(a.core().unwrap(), 1.5).unwrap()).history().unwrap();
            assert_eq!(core.canonical(), cpp.canonical(), "curved {curved}");
        }
    }

    /// A cylinder hollowed, its top open: the walls round closed edges, on a curved face.
    #[test]
    fn tags_cross_a_shell_of_a_cylinder() {
        let _cpp = crate::route::on_cpp();
        crate::init().unwrap();
        let a = Brep::made(ffi::make_cylinder(&[0.0; 3], 10.0, 20.0).unwrap()).tagged(0).unwrap();
        let top = [[0.0, 0.0, 20.0]];
        let cpp = Brep::made(ffi::ops::pick_op(ffi::ops::Pick::Shell, a.cpp().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        let core = Brep::made_core(super::shell(a.core().unwrap(), &top, super::TOL, -2.0).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    /// A 5 degree draft of the front about the bottom plane: the front tilts, its neighbours'
    /// edges move with it.
    #[test]
    fn tags_cross_a_draft() {
        let _cpp = crate::route::on_cpp();
        let a = tagged_box();
        let front = [[10.0, 0.0, 5.0]];
        let (origin, normal, pull, angle) = ([0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 5f64.to_radians());
        let cpp = ffi::ops::draft(a.cpp().unwrap(), &front, super::TOL, &origin, &normal, &pull, angle).unwrap();
        let cpp = Brep::made(cpp).history().unwrap();
        let core = Brep::made_core(super::draft(a.core().unwrap(), &front, super::TOL, origin, normal, pull, angle).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }
}

/// A stepped block, 40 x 30 x 10 with its half x < 20 raised to 20 (rig case
/// shell/cov-replace-face-step-onto-the-higher-face, the fork's `offset_faces_close_a_step_wall`):
/// a top moved onto the other's plane closes the step wall between them to nothing, and the tops
/// are one face of a 6-faced box.
#[test]
fn offset_faces_close_a_step_wall() {
    use super::boolean::boolean;
    let stepped = boolean(&make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(), &make_box([0.0; 3], [20.0, 30.0, 20.0]).unwrap(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    assert_eq!(counts(&stepped).unwrap()[0], 8);
    for (at, d, v, a) in [([30.0, 15.0, 10.0], 10.0, 24000.0, 5200.0), ([10.0, 15.0, 20.0], -10.0, 12000.0, 3800.0)] {
        let r = offset_faces(&stepped, &[at], 0.1, d).unwrap();
        let (got, n, area) = (volume(&r, 1e-8).unwrap(), counts(&r).unwrap(), super::area(&r, 1e-8).unwrap());
        assert_eq!(n[0], 6, "d {d}");
        assert!(rel(got, v) < 1e-9 && rel(area, a) < 1e-9, "d {d}: V {got} A {area}");
    }
}

/// Rig case shell/4492's rounded box: four mitred rounds, r 3, come off a 40 x 30 x 10 box's top
/// together and follow it. From height 0 the outline is the 34 x 24 top moved out by the round's
/// w(z) = sqrt(9 - (z - axis)^2), or 3 below the axis. With the fork's values at +19, and a body
/// whose rounds go all the way down at -7. With SMLib built, the C++ kernel gives the same.
#[test]
fn mitred_rounds_follow_a_moved_top() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    use super::blend::{Corners, blend};
    use crate::Blend;
    let edges = [[20.0, 0.0, 10.0], [20.0, 30.0, 10.0], [0.0, 15.0, 10.0], [40.0, 15.0, 10.0]];
    let top = [20.0, 15.0, 10.0];
    let rounded = blend(&make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(), Blend::Round, &edges, &[3.0; 4], TOL, &[], &[], Corners::Filled).unwrap();
    let analytic = |d: f64| {
        let (height, axis, n) = (10.0 + d, 7.0 + d, 20000);
        (0..n)
            .map(|i| {
                let z = height * (f64::from(i) + 0.5) / f64::from(n);
                let w = if z < axis { 3.0 } else { (9.0 - (z - axis) * (z - axis)).max(0.0).sqrt() };
                (34.0 + 2.0 * w) * (24.0 + 2.0 * w) * height / f64::from(n)
            })
            .sum::<f64>()
    };
    let moves = [(19.0, [10, 20, 12]), (1.0, [10, 20, 12]), (-1.0, [10, 20, 12]), (-6.9, [10, 20, 12]), (-7.0, [6, 12, 8])];
    for (d, n) in moves {
        let moved = offset_faces(&rounded, &[top], 0.1, d).unwrap();
        let v = volume(&moved, 1e-8).unwrap();
        assert_eq!(counts(&moved).unwrap(), n, "d {d}");
        assert!(rel(v, analytic(d)) < 1e-6, "d {d}: {v} vs {}", analytic(d));
    }
    #[cfg(feature = "native")]
    {
        use crate::ffi;
        crate::init().unwrap();
        let b = ffi::make_box(&[0.0; 3], &[40.0, 30.0, 10.0]).unwrap();
        let round = ffi::ops::blend(ffi::ops::BLEND_ROUND, &b, &edges, &[3.0; 4], TOL).unwrap();
        for (d, _) in moves {
            let cpp = ffi::ops::pick_op(ffi::ops::Pick::OffsetFaces, &round, &[top], 0.1, d).unwrap();
            let ours = offset_faces(&rounded, &[top], 0.1, d).unwrap();
            assert!(rel(volume(&ours, 1e-8).unwrap(), ffi::volume(&cpp, 1e-8).unwrap()) < 1e-9, "d {d}");
        }
    }
}
